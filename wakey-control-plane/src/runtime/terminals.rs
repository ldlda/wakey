use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::{Mutex, mpsc};
use uuid::Uuid;

pub const TERMINAL_RELAY_QUEUE: usize = 32;
pub const TERMINAL_MAX_FRAME_BYTES: usize = 64 * 1024;
pub const TERMINAL_REPLAY_BYTES: usize = 256 * 1024;
pub const TERMINAL_MAX_SESSIONS_PER_AGENT: usize = 2;
pub const TERMINAL_ATTACH_TIMEOUT: Duration = Duration::from_secs(10);
pub const TERMINAL_DISCONNECT_GRACE: Duration = Duration::from_secs(15);
pub const TERMINAL_ABSOLUTE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const TERMINAL_TOMBSTONE_TTL: Duration = Duration::from_secs(5 * 60);
const TERMINAL_MAX_TOMBSTONES: usize = 1024;

#[derive(Clone, Debug)]
pub enum TerminalRelayFrame {
    Binary(Vec<u8>),
    Text(String),
    Close,
}

#[derive(Clone)]
pub struct TerminalRegistry {
    inner: Arc<Mutex<HashMap<String, TerminalSession>>>,
    closed: Arc<Mutex<HashMap<String, Instant>>>,
}

struct TerminalSession {
    agent_id: String,
    created_at_unix: u64,
    expires_at: Instant,
    relay_token: Option<String>,
    attachment_token: Option<String>,
    agent_tx: Option<mpsc::Sender<TerminalRelayFrame>>,
    pending_agent: VecDeque<TerminalRelayFrame>,
    pending_agent_bytes: usize,
    operator_tx: Option<mpsc::Sender<TerminalRelayFrame>>,
    operator_detached_at: Option<Instant>,
    replay: VecDeque<TerminalRelayFrame>,
    replay_bytes: usize,
}

pub struct CreatedTerminal {
    pub terminal_id: String,
    pub relay_token: String,
    pub attachment_token: String,
    pub created_at_unix: u64,
}

impl Default for TerminalRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            closed: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn create(&self, agent_id: String) -> Result<CreatedTerminal, &'static str> {
        let mut sessions = self.inner.lock().await;
        if sessions
            .values()
            .filter(|session| session.agent_id == agent_id)
            .count()
            >= TERMINAL_MAX_SESSIONS_PER_AGENT
        {
            return Err("agent_terminal_limit_reached");
        }

        let terminal_id = Uuid::new_v4().to_string();
        let relay_token = new_token();
        let attachment_token = new_token();
        let created_at_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        sessions.insert(
            terminal_id.clone(),
            TerminalSession {
                agent_id,
                created_at_unix,
                expires_at: Instant::now() + TERMINAL_ABSOLUTE_TIMEOUT,
                relay_token: Some(relay_token.clone()),
                attachment_token: Some(attachment_token.clone()),
                agent_tx: None,
                pending_agent: VecDeque::new(),
                pending_agent_bytes: 0,
                operator_tx: None,
                operator_detached_at: None,
                replay: VecDeque::new(),
                replay_bytes: 0,
            },
        );

        Ok(CreatedTerminal {
            terminal_id,
            relay_token,
            attachment_token,
            created_at_unix,
        })
    }

    pub async fn remove(&self, terminal_id: &str) -> Option<String> {
        let session = self.inner.lock().await.remove(terminal_id)?;
        self.remember_closed(terminal_id).await;
        if let Some(tx) = session.agent_tx {
            let _ =
                tokio::time::timeout(Duration::from_secs(1), tx.send(TerminalRelayFrame::Close))
                    .await;
        }
        if let Some(tx) = session.operator_tx {
            let _ =
                tokio::time::timeout(Duration::from_secs(1), tx.send(TerminalRelayFrame::Close))
                    .await;
        }
        Some(session.agent_id)
    }

    /// Reports whether a session ID was recently removed. Tombstones make
    /// idempotent DELETE distinguishable from a completely unknown ID.
    pub async fn was_closed(&self, terminal_id: &str) -> bool {
        let mut closed = self.closed.lock().await;
        prune_tombstones(&mut closed);
        closed.contains_key(terminal_id)
    }

    async fn remember_closed(&self, terminal_id: &str) {
        let mut closed = self.closed.lock().await;
        prune_tombstones(&mut closed);
        closed.insert(terminal_id.to_string(), Instant::now());
        if closed.len() > TERMINAL_MAX_TOMBSTONES
            && let Some(oldest) = closed
                .iter()
                .min_by_key(|(_, closed_at)| **closed_at)
                .map(|(terminal_id, _)| terminal_id.clone())
        {
            closed.remove(&oldest);
        }
    }

    pub async fn remove_agent(&self, agent_id: &str) {
        let terminal_ids = {
            let sessions = self.inner.lock().await;
            sessions
                .iter()
                .filter(|(_, session)| session.agent_id == agent_id)
                .map(|(terminal_id, _)| terminal_id.clone())
                .collect::<Vec<_>>()
        };
        for terminal_id in terminal_ids {
            self.remove(&terminal_id).await;
        }
    }

    pub async fn issue_attachment_token(&self, terminal_id: &str) -> Result<String, &'static str> {
        let mut sessions = self.inner.lock().await;
        let session = active_session(&mut sessions, terminal_id)?;
        if session.operator_tx.is_some() {
            return Err("terminal_operator_already_attached");
        }
        let token = new_token();
        session.attachment_token = Some(token.clone());
        Ok(token)
    }

    pub async fn attach_agent(
        &self,
        terminal_id: &str,
        agent_id: &str,
        relay_token: &str,
    ) -> Result<(mpsc::Receiver<TerminalRelayFrame>, Vec<TerminalRelayFrame>), &'static str> {
        let mut sessions = self.inner.lock().await;
        let session = active_session(&mut sessions, terminal_id)?;
        if session.agent_id != agent_id {
            return Err("terminal_agent_mismatch");
        }
        if session.agent_tx.is_some() {
            return Err("terminal_agent_already_attached");
        }
        if session.relay_token.as_deref() != Some(relay_token) {
            return Err("terminal_relay_token_invalid");
        }
        session.relay_token = None;
        let (tx, rx) = mpsc::channel(TERMINAL_RELAY_QUEUE);
        session.agent_tx = Some(tx);
        let pending = session.pending_agent.drain(..).collect();
        session.pending_agent_bytes = 0;
        Ok((rx, pending))
    }

    pub async fn attach_operator(
        &self,
        terminal_id: &str,
        attachment_token: &str,
    ) -> Result<(mpsc::Receiver<TerminalRelayFrame>, Vec<TerminalRelayFrame>), &'static str> {
        let mut sessions = self.inner.lock().await;
        let session = active_session(&mut sessions, terminal_id)?;
        if session.operator_tx.is_some() {
            return Err("terminal_operator_already_attached");
        }
        if session.attachment_token.as_deref() != Some(attachment_token) {
            return Err("terminal_attachment_token_invalid");
        }
        session.attachment_token = None;
        session.operator_detached_at = None;
        let replay = session.replay.drain(..).collect();
        session.replay_bytes = 0;
        let (tx, rx) = mpsc::channel(TERMINAL_RELAY_QUEUE);
        session.operator_tx = Some(tx);
        Ok((rx, replay))
    }

    pub async fn relay_to_agent(
        &self,
        terminal_id: &str,
        frame: TerminalRelayFrame,
    ) -> Result<(), &'static str> {
        let tx = self
            .inner
            .lock()
            .await
            .get(terminal_id)
            .and_then(|session| session.agent_tx.clone());
        if let Some(tx) = tx {
            return tx
                .send(frame)
                .await
                .map_err(|_| "terminal_agent_disconnected");
        }

        let mut sessions = self.inner.lock().await;
        let session = active_session(&mut sessions, terminal_id)?;
        session.pending_agent_bytes += relay_frame_size(&frame);
        session.pending_agent.push_back(frame);
        while session.pending_agent_bytes > TERMINAL_REPLAY_BYTES {
            if let Some(dropped) = session.pending_agent.pop_front() {
                session.pending_agent_bytes -= relay_frame_size(&dropped);
            } else {
                break;
            }
        }
        Ok(())
    }

    pub async fn relay_from_agent(
        &self,
        terminal_id: &str,
        frame: TerminalRelayFrame,
    ) -> Result<(), &'static str> {
        let operator_tx = self
            .inner
            .lock()
            .await
            .get(terminal_id)
            .and_then(|session| session.operator_tx.clone());

        if let Some(tx) = operator_tx {
            match tx.send(frame).await {
                Ok(()) => return Ok(()),
                Err(err) => {
                    // The browser task may not have marked itself detached yet.
                    // Preserve this frame so that race does not kill the PTY.
                    let mut sessions = self.inner.lock().await;
                    let session = active_session(&mut sessions, terminal_id)?;
                    session.operator_tx = None;
                    session
                        .operator_detached_at
                        .get_or_insert_with(Instant::now);
                    push_replay(session, err.0);
                    return Ok(());
                }
            }
        }

        let mut sessions = self.inner.lock().await;
        let session = active_session(&mut sessions, terminal_id)?;
        push_replay(session, frame);
        Ok(())
    }

    pub async fn reject(
        &self,
        terminal_id: &str,
        agent_id: &str,
        error_json: String,
    ) -> Result<(), &'static str> {
        let matches_agent = self
            .inner
            .lock()
            .await
            .get(terminal_id)
            .is_some_and(|session| session.agent_id == agent_id);
        if !matches_agent {
            return Err("terminal_agent_mismatch");
        }
        self.relay_from_agent(terminal_id, TerminalRelayFrame::Text(error_json))
            .await?;
        self.relay_from_agent(terminal_id, TerminalRelayFrame::Close)
            .await
    }

    pub async fn detach_operator(&self, terminal_id: &str) -> Option<Instant> {
        let mut sessions = self.inner.lock().await;
        let session = sessions.get_mut(terminal_id)?;
        session.operator_tx = None;
        let detached_at = Instant::now();
        session.operator_detached_at = Some(detached_at);
        Some(detached_at)
    }

    pub async fn remove_if_still_detached(&self, terminal_id: &str, detached_at: Instant) -> bool {
        let should_remove = self
            .inner
            .lock()
            .await
            .get(terminal_id)
            .is_some_and(|session| session.operator_detached_at == Some(detached_at));
        if should_remove {
            self.remove(terminal_id).await;
        }
        should_remove
    }

    pub async fn summary(&self, terminal_id: &str) -> Option<(String, u64, bool, bool)> {
        self.inner.lock().await.get(terminal_id).map(|session| {
            (
                session.agent_id.clone(),
                session.created_at_unix,
                session.agent_tx.is_some(),
                session.operator_tx.is_some(),
            )
        })
    }
}

fn active_session<'a>(
    sessions: &'a mut HashMap<String, TerminalSession>,
    terminal_id: &str,
) -> Result<&'a mut TerminalSession, &'static str> {
    let session = sessions.get_mut(terminal_id).ok_or("terminal_not_found")?;
    if session.expires_at <= Instant::now() {
        return Err("terminal_expired");
    }
    Ok(session)
}

fn new_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn prune_tombstones(closed: &mut HashMap<String, Instant>) {
    closed.retain(|_, closed_at| closed_at.elapsed() < TERMINAL_TOMBSTONE_TTL);
}

fn relay_frame_size(frame: &TerminalRelayFrame) -> usize {
    match frame {
        TerminalRelayFrame::Binary(bytes) => bytes.len(),
        TerminalRelayFrame::Text(text) => text.len(),
        TerminalRelayFrame::Close => 0,
    }
}

fn push_replay(session: &mut TerminalSession, frame: TerminalRelayFrame) {
    session.replay_bytes += relay_frame_size(&frame);
    session.replay.push_back(frame);
    while session.replay_bytes > TERMINAL_REPLAY_BYTES {
        if let Some(dropped) = session.replay.pop_front() {
            session.replay_bytes -= relay_frame_size(&dropped);
        } else {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn credentials_are_scoped_and_single_use() {
        let registry = TerminalRegistry::new();
        let created = registry.create("router".into()).await.expect("create");

        assert!(
            registry
                .attach_agent(&created.terminal_id, "other", &created.relay_token)
                .await
                .is_err()
        );
        let _ = registry
            .attach_agent(&created.terminal_id, "router", &created.relay_token)
            .await
            .expect("attach agent");
        assert_eq!(
            registry
                .attach_agent(&created.terminal_id, "router", &created.relay_token)
                .await
                .expect_err("relay token is single use"),
            "terminal_agent_already_attached"
        );

        registry
            .attach_operator(&created.terminal_id, &created.attachment_token)
            .await
            .expect("attach operator");
        assert!(
            registry
                .attach_operator(&created.terminal_id, &created.attachment_token)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn detached_output_replay_is_bounded() {
        let registry = TerminalRegistry::new();
        let created = registry.create("router".into()).await.expect("create");
        for _ in 0..10 {
            registry
                .relay_from_agent(
                    &created.terminal_id,
                    TerminalRelayFrame::Binary(vec![0; TERMINAL_REPLAY_BYTES / 4]),
                )
                .await
                .expect("buffer output");
        }

        let (_, replay) = registry
            .attach_operator(&created.terminal_id, &created.attachment_token)
            .await
            .expect("attach operator");
        assert!(replay.iter().map(relay_frame_size).sum::<usize>() <= TERMINAL_REPLAY_BYTES);
    }

    #[tokio::test]
    async fn operator_input_waits_for_agent_attachment() {
        let registry = TerminalRegistry::new();
        let created = registry.create("router".into()).await.expect("create");
        let resize = TerminalRelayFrame::Text(r#"{"type":"resize","rows":30,"cols":120}"#.into());
        registry
            .relay_to_agent(&created.terminal_id, resize)
            .await
            .expect("queue resize before agent attachment");

        let (_, pending) = registry
            .attach_agent(&created.terminal_id, "router", &created.relay_token)
            .await
            .expect("attach agent");
        assert_eq!(pending.len(), 1);
        assert!(matches!(pending[0], TerminalRelayFrame::Text(_)));
    }

    #[tokio::test]
    async fn agent_can_hold_two_terminal_sessions() {
        let registry = TerminalRegistry::new();
        registry.create("router".into()).await.expect("first");
        registry.create("router".into()).await.expect("second");
        let third = registry.create("router".into()).await;
        assert_eq!(third.err(), Some("agent_terminal_limit_reached"));
    }

    #[tokio::test]
    async fn removed_session_is_distinct_from_unknown_session() {
        let registry = TerminalRegistry::new();
        let created = registry.create("router".into()).await.expect("create");

        assert!(!registry.was_closed(&created.terminal_id).await);
        assert!(!registry.was_closed("never-existed").await);
        registry.remove(&created.terminal_id).await.expect("remove");
        assert!(registry.was_closed(&created.terminal_id).await);
        assert!(!registry.was_closed("never-existed").await);
    }
}
