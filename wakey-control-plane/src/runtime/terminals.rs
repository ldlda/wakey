use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::{Mutex, mpsc};
use uuid::Uuid;
use wakey_agent::protocol::{AgentTerminalSession, TerminalId};

pub const TERMINAL_RELAY_QUEUE: usize = 32;
pub const TERMINAL_MAX_FRAME_BYTES: usize = 64 * 1024;
pub const TERMINAL_PENDING_AGENT_BYTES: usize = 256 * 1024;
pub const TERMINAL_MAX_SESSIONS_PER_AGENT: usize = 2;
pub const TERMINAL_ATTACH_TIMEOUT: Duration = Duration::from_secs(10);
pub const TERMINAL_ABSOLUTE_TIMEOUT: Duration = Duration::from_secs(12 * 60 * 60);
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
    agent_confirmed: bool,
    relay_token: Option<String>,
    attachment_token: Option<String>,
    attachment_operator_id: Option<String>,
    agent_tx: Option<mpsc::Sender<TerminalRelayFrame>>,
    pending_agent: VecDeque<TerminalRelayFrame>,
    pending_agent_bytes: usize,
    operator_tx: Option<mpsc::Sender<TerminalRelayFrame>>,
    operator_id: Option<String>,
    operator_connection_id: Option<String>,
    operator_detached_at: Option<Instant>,
}

pub struct CreatedTerminal {
    pub terminal_id: String,
    pub relay_token: String,
    pub attachment_token: String,
    pub created_at_unix: u64,
}

#[derive(Clone, Debug)]
pub struct TerminalSummary {
    pub terminal_id: String,
    pub agent_id: String,
    pub created_at_unix: u64,
    pub agent_attached: bool,
    pub operator_attached: bool,
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
        prune_expired_sessions(&mut sessions);
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
                agent_confirmed: false,
                relay_token: Some(relay_token.clone()),
                attachment_token: Some(attachment_token.clone()),
                attachment_operator_id: None,
                agent_tx: None,
                pending_agent: VecDeque::new(),
                pending_agent_bytes: 0,
                operator_tx: None,
                operator_id: None,
                operator_connection_id: None,
                operator_detached_at: None,
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

    /// Rebuilds CC's volatile catalog from the PTYs still owned by an agent.
    /// Returned credentials tell those workers to establish fresh relay sockets.
    pub async fn reconcile_agent_sessions(
        &self,
        agent_id: &str,
        reported: &[AgentTerminalSession],
    ) -> Vec<(TerminalId, String)> {
        let reported_ids = reported
            .iter()
            .filter(|session| expires_at_for_created_at(session.created_at_unix).is_some())
            .map(|session| session.terminal_id.as_str())
            .collect::<std::collections::HashSet<_>>();
        let stale_ids = {
            let mut sessions = self.inner.lock().await;
            prune_expired_sessions(&mut sessions);
            sessions
                .iter()
                .filter(|(terminal_id, session)| {
                    session.agent_id == agent_id
                        && session.agent_confirmed
                        && !reported_ids.contains(terminal_id.as_str())
                })
                .map(|(terminal_id, _)| terminal_id.clone())
                .collect::<Vec<_>>()
        };
        for terminal_id in stale_ids {
            self.remove(&terminal_id).await;
        }

        let mut credentials = Vec::new();
        let mut sessions = self.inner.lock().await;
        for reported_session in reported {
            let Some(expires_at) = expires_at_for_created_at(reported_session.created_at_unix)
            else {
                continue;
            };
            let terminal_id = reported_session.terminal_id.as_str().to_string();
            let session = sessions
                .entry(terminal_id)
                .or_insert_with(|| TerminalSession {
                    agent_id: agent_id.to_string(),
                    created_at_unix: reported_session.created_at_unix,
                    expires_at,
                    agent_confirmed: true,
                    relay_token: None,
                    attachment_token: None,
                    attachment_operator_id: None,
                    agent_tx: None,
                    pending_agent: VecDeque::new(),
                    pending_agent_bytes: 0,
                    operator_tx: None,
                    operator_id: None,
                    operator_connection_id: None,
                    operator_detached_at: Some(Instant::now()),
                });
            if session.agent_id != agent_id || session.agent_tx.is_some() {
                continue;
            }
            session.agent_confirmed = true;
            let token = new_token();
            session.relay_token = Some(token.clone());
            credentials.push((reported_session.terminal_id.clone(), token));
        }
        credentials
    }

    pub async fn detach_agent(&self, terminal_id: &str) -> Option<String> {
        let mut sessions = self.inner.lock().await;
        let session = sessions.get_mut(terminal_id)?;
        session.agent_tx = None;
        Some(session.agent_id.clone())
    }

    /// Issues a token while atomically handing this operator's attachment to
    /// the target. A different operator remains protected by the single-viewer
    /// rule, while a stale socket owned by this operator can be replaced.
    pub async fn issue_attachment_token_for_operator(
        &self,
        terminal_id: &str,
        operator_id: &str,
    ) -> Result<String, &'static str> {
        validate_operator_id(operator_id)?;
        let mut sessions = self.inner.lock().await;
        let target = active_session(&mut sessions, terminal_id)?;
        if target.operator_tx.is_some() && target.operator_id.as_deref() != Some(operator_id) {
            return Err("terminal_operator_already_attached");
        }

        detach_operator_sessions(&mut sessions, operator_id);

        let session = active_session(&mut sessions, terminal_id)?;
        let token = new_token();
        session.attachment_token = Some(token.clone());
        session.attachment_operator_id = Some(operator_id.to_string());
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
        session.agent_confirmed = true;
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
        operator_id: &str,
    ) -> Result<mpsc::Receiver<TerminalRelayFrame>, &'static str> {
        validate_operator_id(operator_id)?;
        let mut sessions = self.inner.lock().await;
        let session = active_session(&mut sessions, terminal_id)?;
        if session.operator_tx.is_some() {
            return Err("terminal_operator_already_attached");
        }
        if session.attachment_token.as_deref() != Some(attachment_token) {
            return Err("terminal_attachment_token_invalid");
        }
        if session
            .attachment_operator_id
            .as_deref()
            .is_some_and(|expected| expected != operator_id)
        {
            return Err("terminal_operator_mismatch");
        }

        // Initial create tokens are not yet bound to an operator. Once the
        // WebSocket presents that token, it still atomically releases any
        // older session held by the same browser tab.
        detach_operator_sessions(&mut sessions, operator_id);
        let session = active_session(&mut sessions, terminal_id)?;
        session.attachment_token = None;
        session.attachment_operator_id = None;
        session.operator_detached_at = None;
        let (tx, rx) = mpsc::channel(TERMINAL_RELAY_QUEUE);
        session.operator_tx = Some(tx);
        session.operator_id = Some(operator_id.to_string());
        session.operator_connection_id = Some(attachment_token.to_string());
        Ok(rx)
    }

    pub async fn relay_to_agent(
        &self,
        terminal_id: &str,
        frame: TerminalRelayFrame,
    ) -> Result<(), &'static str> {
        // Select the attached transport or enqueue the frame while holding one
        // lock. Otherwise an agent can attach between those decisions and
        // leave input stranded in the pre-attachment queue.
        let tx = {
            let mut sessions = self.inner.lock().await;
            let session = active_session(&mut sessions, terminal_id)?;
            if let Some(tx) = session.agent_tx.clone() {
                tx
            } else {
                session.pending_agent_bytes += relay_frame_size(&frame);
                session.pending_agent.push_back(frame);
                while session.pending_agent_bytes > TERMINAL_PENDING_AGENT_BYTES {
                    if let Some(dropped) = session.pending_agent.pop_front() {
                        session.pending_agent_bytes -= relay_frame_size(&dropped);
                    } else {
                        break;
                    }
                }
                return Ok(());
            }
        };
        tx.send(frame)
            .await
            .map_err(|_| "terminal_agent_disconnected")
    }

    pub async fn relay_from_agent(
        &self,
        terminal_id: &str,
        frame: TerminalRelayFrame,
    ) -> Result<(), &'static str> {
        let operator_tx = {
            let mut sessions = self.inner.lock().await;
            let session = active_session(&mut sessions, terminal_id)?;
            session.operator_tx.clone()
        };

        if let Some(tx) = operator_tx {
            match tx.send(frame).await {
                Ok(()) => return Ok(()),
                Err(_) => {
                    // The browser task may not have marked itself detached yet.
                    // Detach its stale sender; the agent's parsed screen remains
                    // authoritative and will reconstruct the next attachment.
                    let mut sessions = self.inner.lock().await;
                    let session = active_session(&mut sessions, terminal_id)?;
                    if session
                        .operator_tx
                        .as_ref()
                        .is_some_and(|current| current.same_channel(&tx))
                    {
                        clear_operator_attachment(session);
                    }
                    return Ok(());
                }
            }
        }
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
        self.remove(terminal_id)
            .await
            .map(|_| ())
            .ok_or("terminal_not_found")
    }

    /// Detaches only the socket identified by this attachment token. Delayed
    /// cleanup from an older socket must not clear its replacement.
    pub async fn detach_operator(&self, terminal_id: &str, connection_id: &str) -> Option<Instant> {
        let mut sessions = self.inner.lock().await;
        let session = sessions.get_mut(terminal_id)?;
        if session.operator_connection_id.as_deref() != Some(connection_id) {
            return None;
        }
        let detached_at = Instant::now();
        clear_operator_attachment(session);
        Some(detached_at)
    }

    pub async fn summary(&self, terminal_id: &str) -> Option<(String, u64, bool, bool)> {
        let mut sessions = self.inner.lock().await;
        prune_expired_sessions(&mut sessions);
        sessions.get(terminal_id).map(|session| {
            (
                session.agent_id.clone(),
                session.created_at_unix,
                session.agent_tx.is_some(),
                session.operator_tx.is_some(),
            )
        })
    }

    pub async fn summaries(&self) -> Vec<TerminalSummary> {
        let mut sessions = self.inner.lock().await;
        prune_expired_sessions(&mut sessions);
        let mut summaries = sessions
            .iter()
            .map(|(terminal_id, session)| TerminalSummary {
                terminal_id: terminal_id.clone(),
                agent_id: session.agent_id.clone(),
                created_at_unix: session.created_at_unix,
                agent_attached: session.agent_tx.is_some(),
                operator_attached: session.operator_tx.is_some(),
            })
            .collect::<Vec<_>>();
        summaries.sort_by_key(|session| std::cmp::Reverse(session.created_at_unix));
        summaries
    }
}

fn active_session<'a>(
    sessions: &'a mut HashMap<String, TerminalSession>,
    terminal_id: &str,
) -> Result<&'a mut TerminalSession, &'static str> {
    if sessions
        .get(terminal_id)
        .is_some_and(|session| session.expires_at <= Instant::now())
    {
        sessions.remove(terminal_id);
        return Err("terminal_expired");
    }
    sessions.get_mut(terminal_id).ok_or("terminal_not_found")
}

fn validate_operator_id(operator_id: &str) -> Result<(), &'static str> {
    if operator_id.is_empty() || operator_id.len() > 128 {
        return Err("terminal_operator_id_invalid");
    }
    Ok(())
}

fn detach_operator_sessions(sessions: &mut HashMap<String, TerminalSession>, operator_id: &str) {
    for session in sessions.values_mut() {
        if session.operator_id.as_deref() == Some(operator_id) {
            clear_operator_attachment(session);
        }
    }
}

fn clear_operator_attachment(session: &mut TerminalSession) {
    session.operator_tx = None;
    session.operator_id = None;
    session.operator_connection_id = None;
    session.operator_detached_at = Some(Instant::now());
}

fn new_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn prune_tombstones(closed: &mut HashMap<String, Instant>) {
    closed.retain(|_, closed_at| closed_at.elapsed() < TERMINAL_TOMBSTONE_TTL);
}

fn prune_expired_sessions(sessions: &mut HashMap<String, TerminalSession>) {
    let now = Instant::now();
    sessions.retain(|_, session| session.expires_at > now);
}

/// Converts the agent's durable creation timestamp into CC's monotonic
/// deadline. Reconciliation must not grant an old PTY a fresh twelve hours.
fn expires_at_for_created_at(created_at_unix: u64) -> Option<Instant> {
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let age = Duration::from_secs(now_unix.saturating_sub(created_at_unix));
    if age >= TERMINAL_ABSOLUTE_TIMEOUT {
        return None;
    }
    let remaining = TERMINAL_ABSOLUTE_TIMEOUT - age;
    Some(Instant::now() + remaining)
}

fn relay_frame_size(frame: &TerminalRelayFrame) -> usize {
    match frame {
        TerminalRelayFrame::Binary(bytes) => bytes.len(),
        TerminalRelayFrame::Text(text) => text.len(),
        TerminalRelayFrame::Close => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPERATOR_A: &str = "browser-tab-a";
    const OPERATOR_B: &str = "browser-tab-b";

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
            .attach_operator(&created.terminal_id, &created.attachment_token, OPERATOR_A)
            .await
            .expect("attach operator");
        assert!(
            registry
                .attach_operator(&created.terminal_id, &created.attachment_token, OPERATOR_A,)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn detached_output_is_not_transcribed_by_control_plane() {
        let registry = TerminalRegistry::new();
        let created = registry.create("router".into()).await.expect("create");
        registry
            .relay_from_agent(
                &created.terminal_id,
                TerminalRelayFrame::Binary(b"not retained".to_vec()),
            )
            .await
            .expect("ignore detached output");

        let mut outbound = registry
            .attach_operator(&created.terminal_id, &created.attachment_token, OPERATOR_A)
            .await
            .expect("attach operator");
        assert!(outbound.try_recv().is_err());
    }

    #[tokio::test]
    async fn attached_output_is_delivered_live_only() {
        let registry = TerminalRegistry::new();
        let created = registry.create("router".into()).await.expect("create");
        let mut outbound = registry
            .attach_operator(&created.terminal_id, &created.attachment_token, OPERATOR_A)
            .await
            .expect("attach operator");
        let frame = TerminalRelayFrame::Binary(b"recent prompt".to_vec());
        registry
            .relay_from_agent(&created.terminal_id, frame)
            .await
            .expect("relay output");
        outbound.recv().await.expect("live output");
        registry
            .detach_operator(&created.terminal_id, &created.attachment_token)
            .await;

        let token = registry
            .issue_attachment_token_for_operator(&created.terminal_id, OPERATOR_A)
            .await
            .expect("reattach token");
        let mut remounted = registry
            .attach_operator(&created.terminal_id, &token, OPERATOR_A)
            .await
            .expect("reattach operator");

        assert!(remounted.try_recv().is_err());
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

    #[tokio::test]
    async fn agent_inventory_adopts_and_reconnects_live_session() {
        let registry = TerminalRegistry::new();
        let terminal_id = TerminalId::new("survived-cc").expect("terminal id");
        let created_at_unix = u64::MAX;
        let reported = AgentTerminalSession {
            terminal_id: terminal_id.clone(),
            created_at_unix,
        };

        let credentials = registry
            .reconcile_agent_sessions("router", &[reported])
            .await;
        assert_eq!(credentials.len(), 1);
        let (_, relay_token) = &credentials[0];
        registry
            .attach_agent(terminal_id.as_str(), "router", relay_token)
            .await
            .expect("attach adopted agent session");

        let summary = registry
            .summary(terminal_id.as_str())
            .await
            .expect("adopted summary");
        assert_eq!(summary.0, "router");
        assert_eq!(summary.1, created_at_unix);
        assert!(summary.2);
    }

    #[tokio::test]
    async fn expired_sessions_release_agent_quota() {
        let registry = TerminalRegistry::new();
        let first = registry.create("router".into()).await.expect("first");
        registry.create("router".into()).await.expect("second");
        registry
            .inner
            .lock()
            .await
            .get_mut(&first.terminal_id)
            .expect("first session")
            .expires_at = Instant::now();

        registry
            .create("router".into())
            .await
            .expect("expired session no longer consumes quota");
        assert!(registry.summary(&first.terminal_id).await.is_none());
    }

    #[tokio::test]
    async fn reconciliation_does_not_revive_expired_agent_session() {
        let registry = TerminalRegistry::new();
        let terminal_id = TerminalId::new("expired-agent-session").expect("terminal id");
        let reported = AgentTerminalSession {
            terminal_id: terminal_id.clone(),
            created_at_unix: 0,
        };

        assert!(
            registry
                .reconcile_agent_sessions("router", &[reported])
                .await
                .is_empty()
        );
        assert!(registry.summary(terminal_id.as_str()).await.is_none());
    }

    #[tokio::test]
    async fn operator_detach_does_not_remove_session() {
        let registry = TerminalRegistry::new();
        let created = registry.create("router".into()).await.expect("create");
        registry
            .attach_operator(&created.terminal_id, &created.attachment_token, OPERATOR_A)
            .await
            .expect("attach operator");

        registry
            .detach_operator(&created.terminal_id, &created.attachment_token)
            .await;

        let summary = registry
            .summary(&created.terminal_id)
            .await
            .expect("session remains after detach");
        assert!(!summary.3);
        registry
            .issue_attachment_token_for_operator(&created.terminal_id, OPERATOR_A)
            .await
            .expect("detached session can be reattached");
    }

    #[tokio::test]
    async fn attachment_handoff_releases_previous_operator_atomically() {
        let registry = TerminalRegistry::new();
        let previous = registry.create("router".into()).await.expect("previous");
        let next = registry.create("router".into()).await.expect("next");
        registry
            .attach_operator(
                &previous.terminal_id,
                &previous.attachment_token,
                OPERATOR_A,
            )
            .await
            .expect("attach previous");

        let next_token = registry
            .issue_attachment_token_for_operator(&next.terminal_id, OPERATOR_A)
            .await
            .expect("handoff token");

        assert!(!registry.summary(&previous.terminal_id).await.unwrap().3);
        registry
            .attach_operator(&next.terminal_id, &next_token, OPERATOR_A)
            .await
            .expect("attach next");
    }

    #[tokio::test]
    async fn attachment_handoff_does_not_evict_unrelated_operator() {
        let registry = TerminalRegistry::new();
        let previous = registry.create("router".into()).await.expect("previous");
        let occupied = registry.create("router".into()).await.expect("occupied");
        registry
            .attach_operator(
                &previous.terminal_id,
                &previous.attachment_token,
                OPERATOR_A,
            )
            .await
            .expect("attach previous");
        registry
            .attach_operator(
                &occupied.terminal_id,
                &occupied.attachment_token,
                OPERATOR_B,
            )
            .await
            .expect("attach occupied");

        let error = registry
            .issue_attachment_token_for_operator(&occupied.terminal_id, OPERATOR_A)
            .await
            .expect_err("occupied target remains protected");

        assert_eq!(error, "terminal_operator_already_attached");
        assert!(registry.summary(&previous.terminal_id).await.unwrap().3);
        assert!(registry.summary(&occupied.terminal_id).await.unwrap().3);
    }

    #[tokio::test]
    async fn stale_socket_cleanup_does_not_detach_replacement() {
        let registry = TerminalRegistry::new();
        let created = registry.create("router".into()).await.expect("create");
        let old_connection_id = created.attachment_token.clone();
        let _old_outbound = registry
            .attach_operator(&created.terminal_id, &created.attachment_token, OPERATOR_A)
            .await
            .expect("attach old socket");

        let replacement_token = registry
            .issue_attachment_token_for_operator(&created.terminal_id, OPERATOR_A)
            .await
            .expect("replace own socket");
        let _replacement_outbound = registry
            .attach_operator(&created.terminal_id, &replacement_token, OPERATOR_A)
            .await
            .expect("attach replacement socket");

        assert!(
            registry
                .detach_operator(&created.terminal_id, &old_connection_id)
                .await
                .is_none()
        );
        assert!(registry.summary(&created.terminal_id).await.unwrap().3);
    }

    #[tokio::test]
    async fn reconciliation_does_not_remove_open_request_still_in_flight() {
        let registry = TerminalRegistry::new();
        let created = registry.create("router".into()).await.expect("create");

        registry.reconcile_agent_sessions("router", &[]).await;

        assert!(registry.summary(&created.terminal_id).await.is_some());
    }
}
