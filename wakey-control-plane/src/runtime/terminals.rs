use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::{Mutex, broadcast, mpsc};
use uuid::Uuid;
use wakey_core::protocol::{AgentTerminalSession, TerminalId};

mod operator;

use operator::OperatorAttachment;

pub const TERMINAL_RELAY_QUEUE: usize = 32;
pub const TERMINAL_MAX_FRAME_BYTES: usize = 64 * 1024;
pub const TERMINAL_PENDING_AGENT_BYTES: usize = 256 * 1024;
pub const TERMINAL_ATTACH_TIMEOUT: Duration = Duration::from_secs(10);
const TERMINAL_TOMBSTONE_TTL: Duration = Duration::from_secs(5 * 60);
const TERMINAL_MAX_TOMBSTONES: usize = 1024;
const TERMINAL_EVENTS_CAPACITY: usize = 32;

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
    events: broadcast::Sender<()>,
}

struct TerminalSession {
    agent_id: String,
    created_at_unix: u64,
    expiry: TerminalExpiry,
    agent_confirmed: bool,
    relay_token: Option<String>,
    agent_tx: Option<mpsc::Sender<TerminalRelayFrame>>,
    pending_agent: VecDeque<TerminalRelayFrame>,
    pending_agent_bytes: usize,
    operator: OperatorAttachment,
}

pub struct CreatedTerminal {
    pub terminal_id: String,
    pub relay_token: String,
    pub attachment_token: String,
    pub created_at_unix: u64,
    pub expires_at_unix: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct TerminalSummary {
    pub terminal_id: String,
    pub agent_id: String,
    pub created_at_unix: u64,
    pub expires_at_unix: Option<u64>,
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
        let (events, _) = broadcast::channel(TERMINAL_EVENTS_CAPACITY);
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            closed: Arc::new(Mutex::new(HashMap::new())),
            events,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.events.subscribe()
    }

    fn notify_changed(&self) {
        let _ = self.events.send(());
    }

    #[cfg(test)]
    async fn create(&self, agent_id: String) -> Result<CreatedTerminal, &'static str> {
        self.create_with_limits(
            agent_id,
            wakey_core::protocol::DEFAULT_TERMINAL_MAX_SESSIONS,
            wakey_core::protocol::DEFAULT_TERMINAL_SESSION_TTL_SECONDS,
        )
        .await
    }

    /// Creates a session using the limit advertised by the connected agent.
    /// The caller snapshots the limit with the agent connection so UI hints and
    /// server-side enforcement use the same value.
    pub async fn create_with_limits(
        &self,
        agent_id: String,
        max_sessions: usize,
        session_ttl_seconds: u64,
    ) -> Result<CreatedTerminal, &'static str> {
        let mut sessions = self.inner.lock().await;
        let pruned = prune_expired_sessions(&mut sessions) > 0;
        if sessions
            .values()
            .filter(|session| session.agent_id == agent_id)
            .count()
            >= max_sessions.max(1)
        {
            drop(sessions);
            if pruned {
                self.notify_changed();
            }
            return Err("agent_terminal_limit_reached");
        }

        let terminal_id = Uuid::new_v4().to_string();
        let relay_token = new_token();
        let attachment_token = new_token();
        let created_at_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let Some(expiry) = expiry_for_created_at(created_at_unix, session_ttl_seconds) else {
            drop(sessions);
            if pruned {
                self.notify_changed();
            }
            return Err("terminal_ttl_invalid");
        };
        let mut operator = OperatorAttachment::default();
        operator.issue_token(attachment_token.clone(), None);
        sessions.insert(
            terminal_id.clone(),
            TerminalSession {
                agent_id,
                created_at_unix,
                expiry,
                agent_confirmed: false,
                relay_token: Some(relay_token.clone()),
                agent_tx: None,
                pending_agent: VecDeque::new(),
                pending_agent_bytes: 0,
                operator,
            },
        );
        drop(sessions);
        self.notify_changed();

        Ok(CreatedTerminal {
            terminal_id,
            relay_token,
            attachment_token,
            created_at_unix,
            expires_at_unix: expiry.unix(),
        })
    }

    pub async fn remove(&self, terminal_id: &str) -> Option<String> {
        let session = self.inner.lock().await.remove(terminal_id)?;
        self.notify_changed();
        self.remember_closed(terminal_id).await;
        if let Some(tx) = session.agent_tx {
            let _ =
                tokio::time::timeout(Duration::from_secs(1), tx.send(TerminalRelayFrame::Close))
                    .await;
        }
        if let Some(tx) = session.operator.into_sender() {
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
            .filter(|session| {
                expiry_for_created_at(session.created_at_unix, session.session_ttl_seconds)
                    .is_some()
            })
            .map(|session| session.terminal_id.as_str())
            .collect::<std::collections::HashSet<_>>();
        let (stale_ids, pruned) = {
            let mut sessions = self.inner.lock().await;
            let pruned = prune_expired_sessions(&mut sessions) > 0;
            let stale_ids = sessions
                .iter()
                .filter(|(terminal_id, session)| {
                    session.agent_id == agent_id
                        && session.agent_confirmed
                        && !reported_ids.contains(terminal_id.as_str())
                })
                .map(|(terminal_id, _)| terminal_id.clone())
                .collect::<Vec<_>>();
            (stale_ids, pruned)
        };
        if pruned {
            self.notify_changed();
        }
        for terminal_id in stale_ids {
            self.remove(&terminal_id).await;
        }

        let mut credentials = Vec::new();
        let mut changed = false;
        let mut sessions = self.inner.lock().await;
        for reported_session in reported {
            let Some(expiry) = expiry_for_created_at(
                reported_session.created_at_unix,
                reported_session.session_ttl_seconds,
            ) else {
                continue;
            };
            let terminal_id = reported_session.terminal_id.as_str().to_string();
            let session = sessions.entry(terminal_id).or_insert_with(|| {
                changed = true;
                TerminalSession {
                    agent_id: agent_id.to_string(),
                    created_at_unix: reported_session.created_at_unix,
                    expiry,
                    agent_confirmed: true,
                    relay_token: None,
                    agent_tx: None,
                    pending_agent: VecDeque::new(),
                    pending_agent_bytes: 0,
                    operator: OperatorAttachment::default(),
                }
            });
            if session.agent_id != agent_id || session.agent_tx.is_some() {
                continue;
            }
            session.agent_confirmed = true;
            let token = new_token();
            session.relay_token = Some(token.clone());
            credentials.push((reported_session.terminal_id.clone(), token));
        }
        drop(sessions);
        if changed {
            self.notify_changed();
        }
        credentials
    }

    pub async fn detach_agent(&self, terminal_id: &str) -> Option<String> {
        let mut sessions = self.inner.lock().await;
        let session = sessions.get_mut(terminal_id)?;
        let changed = session.agent_tx.is_some();
        session.agent_tx = None;
        let agent_id = session.agent_id.clone();
        drop(sessions);
        if changed {
            self.notify_changed();
        }
        Some(agent_id)
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
        let target = active_session(&mut sessions, terminal_id, &self.events)?;
        if target.operator.is_held_by_other(operator_id) {
            return Err("terminal_operator_already_attached");
        }

        let detached = detach_operator_sessions(&mut sessions, operator_id);

        let session = active_session(&mut sessions, terminal_id, &self.events)?;
        let token = new_token();
        session
            .operator
            .issue_token(token.clone(), Some(operator_id));
        drop(sessions);
        if detached {
            self.notify_changed();
        }
        Ok(token)
    }

    pub async fn attach_agent(
        &self,
        terminal_id: &str,
        agent_id: &str,
        relay_token: &str,
    ) -> Result<(mpsc::Receiver<TerminalRelayFrame>, Vec<TerminalRelayFrame>), &'static str> {
        let mut sessions = self.inner.lock().await;
        let session = active_session(&mut sessions, terminal_id, &self.events)?;
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
        drop(sessions);
        self.notify_changed();
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
        let session = active_session(&mut sessions, terminal_id, &self.events)?;
        session
            .operator
            .validate_token(attachment_token, operator_id)?;

        // Initial create tokens are not yet bound to an operator. Once the
        // WebSocket presents that token, it still atomically releases any
        // older session held by the same browser tab.
        detach_operator_sessions(&mut sessions, operator_id);
        let session = active_session(&mut sessions, terminal_id, &self.events)?;
        let rx = session.operator.attach(operator_id);
        drop(sessions);
        self.notify_changed();
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
            let session = active_session(&mut sessions, terminal_id, &self.events)?;
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
            let session = active_session(&mut sessions, terminal_id, &self.events)?;
            session.operator.sender()
        };

        if let Some(tx) = operator_tx {
            match tx.send(frame).await {
                Ok(()) => return Ok(()),
                Err(_) => {
                    // The browser task may not have marked itself detached yet.
                    // Detach its stale sender; the agent's parsed screen remains
                    // authoritative and will reconstruct the next attachment.
                    let mut sessions = self.inner.lock().await;
                    let session = active_session(&mut sessions, terminal_id, &self.events)?;
                    let changed = session.operator.detach_sender(&tx);
                    drop(sessions);
                    if changed {
                        self.notify_changed();
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
        if !session.operator.detach_connection(connection_id) {
            return None;
        }
        let detached_at = Instant::now();
        drop(sessions);
        self.notify_changed();
        Some(detached_at)
    }

    pub async fn summary(&self, terminal_id: &str) -> Option<TerminalSummary> {
        let mut sessions = self.inner.lock().await;
        let pruned = prune_expired_sessions(&mut sessions) > 0;
        let summary = sessions
            .get(terminal_id)
            .map(|session| terminal_summary(terminal_id, session));
        drop(sessions);
        if pruned {
            self.notify_changed();
        }
        summary
    }

    pub async fn summaries(&self) -> Vec<TerminalSummary> {
        let mut sessions = self.inner.lock().await;
        let removed = prune_expired_sessions(&mut sessions);
        let mut summaries = sessions
            .iter()
            .map(|(terminal_id, session)| terminal_summary(terminal_id, session))
            .collect::<Vec<_>>();
        summaries.sort_by_key(|session| std::cmp::Reverse(session.created_at_unix));
        drop(sessions);
        if removed > 0 {
            self.notify_changed();
        }
        summaries
    }
}

fn terminal_summary(terminal_id: &str, session: &TerminalSession) -> TerminalSummary {
    TerminalSummary {
        terminal_id: terminal_id.to_string(),
        agent_id: session.agent_id.clone(),
        created_at_unix: session.created_at_unix,
        expires_at_unix: session.expiry.unix(),
        agent_attached: session.agent_tx.is_some(),
        operator_attached: session.operator.is_attached(),
    }
}

fn active_session<'a>(
    sessions: &'a mut HashMap<String, TerminalSession>,
    terminal_id: &str,
    events: &broadcast::Sender<()>,
) -> Result<&'a mut TerminalSession, &'static str> {
    if sessions
        .get(terminal_id)
        .is_some_and(|session| session.expiry.is_expired(Instant::now()))
    {
        sessions.remove(terminal_id);
        let _ = events.send(());
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

fn detach_operator_sessions(
    sessions: &mut HashMap<String, TerminalSession>,
    operator_id: &str,
) -> bool {
    let mut detached = false;
    for session in sessions.values_mut() {
        detached |= session.operator.detach_operator(operator_id);
    }
    detached
}

fn new_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn prune_tombstones(closed: &mut HashMap<String, Instant>) {
    closed.retain(|_, closed_at| closed_at.elapsed() < TERMINAL_TOMBSTONE_TTL);
}

fn prune_expired_sessions(sessions: &mut HashMap<String, TerminalSession>) -> usize {
    let now = Instant::now();
    let old_len = sessions.len();
    sessions.retain(|_, session| !session.expiry.is_expired(now));
    old_len - sessions.len()
}

#[derive(Clone, Copy, Debug)]
enum TerminalExpiry {
    Unlimited,
    Finite { deadline: Instant, unix: u64 },
}

impl TerminalExpiry {
    fn unix(self) -> Option<u64> {
        match self {
            Self::Unlimited => None,
            Self::Finite { unix, .. } => Some(unix),
        }
    }

    fn is_expired(self, now: Instant) -> bool {
        matches!(self, Self::Finite { deadline, .. } if deadline <= now)
    }
}

/// Converts the agent's creation-time policy into CC's monotonic deadline.
/// `None` rejects expired or unrepresentable sessions; `Unlimited` is the
/// explicit zero-TTL policy.
fn expiry_for_created_at(created_at_unix: u64, session_ttl_seconds: u64) -> Option<TerminalExpiry> {
    if session_ttl_seconds == 0 {
        return Some(TerminalExpiry::Unlimited);
    }
    let expires_at_unix = created_at_unix.checked_add(session_ttl_seconds)?;
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if expires_at_unix <= now_unix {
        return None;
    }
    let remaining = Duration::from_secs(expires_at_unix - now_unix);
    Some(TerminalExpiry::Finite {
        deadline: Instant::now().checked_add(remaining)?,
        unix: expires_at_unix,
    })
}

fn relay_frame_size(frame: &TerminalRelayFrame) -> usize {
    match frame {
        TerminalRelayFrame::Binary(bytes) => bytes.len(),
        TerminalRelayFrame::Text(text) => text.len(),
        TerminalRelayFrame::Close => 0,
    }
}

#[cfg(test)]
mod tests;
