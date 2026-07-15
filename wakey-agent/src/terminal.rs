use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use crate::config::AgentConfig;
use crate::protocol::{AgentTerminalSession, TerminalAgentHandshake, TerminalControl, TerminalId};
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

const MAX_TERMINAL_FRAME_BYTES: usize = 64 * 1024;
const TERMINAL_REPLAY_BYTES: usize = 256 * 1024;
const PROCESS_SIGNAL_GRACE: Duration = Duration::from_secs(1);

struct ActiveTerminal {
    cancel: oneshot::Sender<()>,
    relay_credentials: mpsc::UnboundedSender<String>,
    created_at_unix: u64,
}

/// Owns PTY workers independently of any individual control-plane connection.
pub struct TerminalManager {
    active: Arc<Mutex<HashMap<String, ActiveTerminal>>>,
    max_sessions: usize,
    events: mpsc::UnboundedSender<TerminalManagerEvent>,
}

pub struct TerminalManagerEvent {
    pub terminal_id: TerminalId,
    pub error: String,
}

impl TerminalManager {
    pub fn new(config: &AgentConfig) -> (Self, mpsc::UnboundedReceiver<TerminalManagerEvent>) {
        let (events, event_rx) = mpsc::unbounded_channel();
        (
            Self {
                active: Arc::new(Mutex::new(HashMap::new())),
                max_sessions: config.terminal.max_sessions.max(1),
                events,
            },
            event_rx,
        )
    }

    pub fn open(
        &self,
        config: &AgentConfig,
        terminal_id: TerminalId,
        relay_token: String,
        rows: u16,
        cols: u16,
    ) -> Result<()> {
        if !config.terminal.enabled {
            anyhow::bail!("terminal capability is disabled");
        }

        let terminal_key = terminal_id.to_string();
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let (relay_tx, relay_rx) = mpsc::unbounded_channel();
        let created_at_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        {
            let mut active = self.active.lock().expect("terminal manager poisoned");
            if active.contains_key(&terminal_key) {
                anyhow::bail!("terminal session {terminal_key} is already active");
            }
            if active.len() >= self.max_sessions {
                anyhow::bail!("agent terminal session limit reached");
            }
            active.insert(
                terminal_key.clone(),
                ActiveTerminal {
                    cancel: cancel_tx,
                    relay_credentials: relay_tx.clone(),
                    created_at_unix,
                },
            );
        }
        let _ = relay_tx.send(relay_token);

        let config = config.clone();
        let active = Arc::downgrade(&self.active);
        let events = self.events.clone();
        tokio::spawn(async move {
            if let Err(err) =
                run_terminal(&config, &terminal_id, rows, cols, cancel_rx, relay_rx).await
            {
                warn!(terminal_id = %terminal_id, error = %err, "terminal worker failed");
                let _ = events.send(TerminalManagerEvent {
                    terminal_id: terminal_id.clone(),
                    error: err.to_string(),
                });
            }
            remove_completed(&active, terminal_id.as_str());
        });
        Ok(())
    }

    pub fn close(&self, terminal_id: &TerminalId) -> bool {
        self.active
            .lock()
            .expect("terminal manager poisoned")
            .remove(terminal_id.as_str())
            .is_some_and(|active| active.cancel.send(()).is_ok())
    }

    pub fn resume(&self, terminal_id: &TerminalId, relay_token: String) -> Result<()> {
        let active = self.active.lock().expect("terminal manager poisoned");
        let session = active
            .get(terminal_id.as_str())
            .with_context(|| format!("terminal session {terminal_id} is not active"))?;
        session
            .relay_credentials
            .send(relay_token)
            .map_err(|_| anyhow::anyhow!("terminal session {terminal_id} has stopped"))
    }

    pub fn sessions(&self) -> Vec<AgentTerminalSession> {
        self.active
            .lock()
            .expect("terminal manager poisoned")
            .iter()
            .filter_map(|(terminal_id, active)| {
                TerminalId::new(terminal_id.clone())
                    .ok()
                    .map(|terminal_id| AgentTerminalSession {
                        terminal_id,
                        created_at_unix: active.created_at_unix,
                    })
            })
            .collect()
    }
}

impl Drop for TerminalManager {
    fn drop(&mut self) {
        if Arc::strong_count(&self.active) == 1
            && let Ok(mut active) = self.active.lock()
        {
            for (_, active) in active.drain() {
                let _ = active.cancel.send(());
            }
        }
    }
}

fn remove_completed(active: &Weak<Mutex<HashMap<String, ActiveTerminal>>>, terminal_id: &str) {
    if let Some(active) = active.upgrade()
        && let Ok(mut active) = active.lock()
    {
        active.remove(terminal_id);
    }
}

#[cfg(unix)]
async fn run_terminal(
    config: &AgentConfig,
    terminal_id: &TerminalId,
    rows: u16,
    cols: u16,
    mut cancel: oneshot::Receiver<()>,
    mut relay_credentials: mpsc::UnboundedReceiver<String>,
) -> Result<()> {
    let terminal = match wakey::wakey_linux::terminal::TerminalPty::spawn(
        Path::new(&config.terminal.shell),
        rows,
        cols,
    ) {
        Ok(terminal) => terminal,
        Err(err) => {
            return Err(err);
        }
    };
    let (mut reader, mut writer, mut child) = terminal.into_parts();
    let process_group = child.id();
    info!(terminal_id = %terminal_id, shell = %config.terminal.shell.display(), "terminal PTY ready");

    let (relay_input_tx, mut relay_input_rx) = mpsc::unbounded_channel();
    let mut relay_output: Option<mpsc::Sender<Message>> = None;
    let mut relay_task: Option<tokio::task::JoinHandle<()>> = None;
    let mut relay_generation = 0_u64;
    let mut replay = VecDeque::new();
    let mut replay_bytes = 0_usize;
    let mut output = [0_u8; 16 * 1024];
    let mut requested_close = false;
    let mut observed_status = None;
    loop {
        tokio::select! {
            biased;
            _ = &mut cancel => {
                requested_close = true;
                break;
            }
            status = child.wait() => {
                observed_status = Some(status.context("failed waiting for terminal child")?);
                break;
            }
            incoming = relay_input_rx.recv() => {
                let Some(message) = incoming else { break; };
                match message {
                    RelayInput::Binary { generation, bytes } if generation == relay_generation => {
                        if bytes.len() > MAX_TERMINAL_FRAME_BYTES {
                            anyhow::bail!("terminal input frame exceeds size limit");
                        }
                        writer.write_all(&bytes).await.context("failed to write PTY input")?;
                    }
                    RelayInput::Resize { generation, rows, cols } if generation == relay_generation => {
                        validate_size(rows, cols)?;
                        writer.resize(rows, cols)?;
                    }
                    RelayInput::Refresh { generation } if generation == relay_generation => {
                        if let Err(err) = writer.refresh() {
                            warn!(terminal_id = %terminal_id, error = %err, "terminal redraw signal failed");
                        }
                    }
                    RelayInput::Close { generation } if generation == relay_generation => {
                        requested_close = true;
                        break;
                    }
                    RelayInput::Connected { generation, output } if generation == relay_generation => {
                        relay_output = Some(output.clone());
                        while let Some(frame) = replay.pop_front() {
                            replay_bytes = replay_bytes.saturating_sub(message_size(&frame));
                            if output.send(frame).await.is_err() {
                                relay_output = None;
                                break;
                            }
                        }
                    }
                    RelayInput::Disconnected { generation } if generation == relay_generation => {
                        relay_output = None;
                    }
                    _ => {}
                }
            }
            credential = relay_credentials.recv() => {
                let Some(relay_token) = credential else { break; };
                if let Some(task) = relay_task.take() {
                    task.abort();
                }
                relay_generation = relay_generation.wrapping_add(1);
                let generation = relay_generation;
                let (output_tx, output_rx) = mpsc::channel(32);
                relay_output = None;
                let initial_replay = replay.drain(..).collect();
                replay_bytes = 0;
                let config = config.clone();
                let terminal_id = terminal_id.clone();
                let relay_input_tx = relay_input_tx.clone();
                relay_task = Some(tokio::spawn(async move {
                    if let Err(err) = run_terminal_relay(RelayConnection {
                        config,
                        terminal_id: terminal_id.clone(),
                        relay_token,
                        generation,
                        initial_replay,
                        output_tx,
                        output_rx,
                        input: relay_input_tx.clone(),
                    }).await {
                        warn!(terminal_id = %terminal_id, error = %err, "terminal relay disconnected");
                    }
                    let _ = relay_input_tx.send(RelayInput::Disconnected { generation });
                }));
            }
            read = reader.read(&mut output) => {
                match read {
                    Ok(0) => break,
                    Ok(count) => send_terminal_output(
                        Message::Binary(output[..count].to_vec().into()),
                        &mut relay_output,
                        &mut replay,
                        &mut replay_bytes,
                    ).await,
                    // Linux PTY masters commonly report EIO after the slave closes.
                    Err(err) if err.raw_os_error() == Some(5) => break,
                    Err(err) => return Err(err).context("failed to read PTY output"),
                }
            }
        }
    }

    let status = match observed_status {
        Some(status) => status,
        None => terminate_process_group(&mut child, process_group).await?,
    };
    if let Some(output) = relay_output {
        let control = TerminalControl::Exited {
            exit_code: status.code(),
        };
        if let Ok(text) = serde_json::to_string(&control) {
            let _ = output.send(Message::Text(text.into())).await;
        }
    }
    info!(terminal_id = %terminal_id, exit_code = ?status.code(), requested_close, "terminal worker exited");
    Ok(())
}

enum RelayInput {
    Connected {
        generation: u64,
        output: mpsc::Sender<Message>,
    },
    Binary {
        generation: u64,
        bytes: Vec<u8>,
    },
    Resize {
        generation: u64,
        rows: u16,
        cols: u16,
    },
    Refresh {
        generation: u64,
    },
    Close {
        generation: u64,
    },
    Disconnected {
        generation: u64,
    },
}

#[cfg(unix)]
struct RelayConnection {
    config: AgentConfig,
    terminal_id: TerminalId,
    relay_token: String,
    generation: u64,
    initial_replay: Vec<Message>,
    output_tx: mpsc::Sender<Message>,
    output_rx: mpsc::Receiver<Message>,
    input: mpsc::UnboundedSender<RelayInput>,
}

#[cfg(unix)]
async fn run_terminal_relay(relay: RelayConnection) -> Result<()> {
    let ws_url = terminal_websocket_url(&relay.config.server_url, &relay.terminal_id)?;
    let (stream, _) = tokio_tungstenite::connect_async(ws_url.as_str())
        .await
        .context("failed to connect terminal relay websocket")?;
    let (mut sink, mut source) = stream.split();
    send_json(
        &mut sink,
        &TerminalAgentHandshake::Auth {
            agent_id: relay.config.agent_id.clone(),
            relay_token: relay.relay_token,
        },
    )
    .await?;
    send_json(&mut sink, &TerminalControl::Ready).await?;
    for frame in relay.initial_replay {
        sink.send(frame)
            .await
            .context("failed to replay detached terminal output")?;
    }
    relay
        .input
        .send(RelayInput::Connected {
            generation: relay.generation,
            output: relay.output_tx,
        })
        .map_err(|_| anyhow::anyhow!("terminal worker stopped"))?;

    let mut output = relay.output_rx;
    loop {
        tokio::select! {
            biased;
            incoming = source.next() => {
                let Some(message) = incoming else { break; };
                match message.context("terminal relay websocket receive failed")? {
                    Message::Binary(bytes) => {
                        relay.input.send(RelayInput::Binary {
                            generation: relay.generation,
                            bytes: bytes.to_vec(),
                        }).map_err(|_| anyhow::anyhow!("terminal worker stopped"))?;
                    }
                    Message::Text(text) => match serde_json::from_str::<TerminalControl>(&text)
                        .context("invalid terminal control frame")?
                    {
                        TerminalControl::Resize { rows, cols } => {
                            relay.input.send(RelayInput::Resize {
                                generation: relay.generation,
                                rows,
                                cols,
                            })
                                .map_err(|_| anyhow::anyhow!("terminal worker stopped"))?;
                        }
                        TerminalControl::Refresh => {
                            relay.input.send(RelayInput::Refresh {
                                generation: relay.generation,
                            }).map_err(|_| anyhow::anyhow!("terminal worker stopped"))?;
                        }
                        TerminalControl::Close => {
                            let _ = relay.input.send(RelayInput::Close {
                                generation: relay.generation,
                            });
                            break;
                        }
                        _ => anyhow::bail!("terminal control frame has invalid direction"),
                    },
                    Message::Ping(payload) => sink.send(Message::Pong(payload)).await?,
                    Message::Pong(_) => {}
                    // Transport closure only detaches the relay. The agent-owned
                    // PTY remains alive and waits for replacement credentials.
                    Message::Close(_) => break,
                    Message::Frame(_) => {}
                }
            }
            outgoing = output.recv() => {
                let Some(message) = outgoing else { break; };
                sink.send(message).await.context("failed to send terminal relay output")?;
            }
        }
    }
    Ok(())
}

async fn send_terminal_output(
    frame: Message,
    relay: &mut Option<mpsc::Sender<Message>>,
    replay: &mut VecDeque<Message>,
    replay_bytes: &mut usize,
) {
    if let Some(tx) = relay.as_ref() {
        if let Err(error) = tx.send(frame).await {
            *relay = None;
            push_local_replay(error.0, replay, replay_bytes);
        }
    } else {
        push_local_replay(frame, replay, replay_bytes);
    }
}

fn push_local_replay(frame: Message, replay: &mut VecDeque<Message>, replay_bytes: &mut usize) {
    *replay_bytes += message_size(&frame);
    replay.push_back(frame);
    while *replay_bytes > TERMINAL_REPLAY_BYTES {
        if let Some(dropped) = replay.pop_front() {
            *replay_bytes = replay_bytes.saturating_sub(message_size(&dropped));
        } else {
            break;
        }
    }
}

fn message_size(message: &Message) -> usize {
    match message {
        Message::Text(text) => text.len(),
        Message::Binary(bytes) | Message::Ping(bytes) | Message::Pong(bytes) => bytes.len(),
        Message::Close(_) | Message::Frame(_) => 0,
    }
}

#[cfg(not(unix))]
async fn run_terminal(
    _config: &AgentConfig,
    _terminal_id: &TerminalId,
    _rows: u16,
    _cols: u16,
    _cancel: oneshot::Receiver<()>,
    _relay_credentials: mpsc::UnboundedReceiver<String>,
) -> Result<()> {
    anyhow::bail!("terminal sessions are unsupported on this platform")
}

#[cfg(unix)]
async fn terminate_process_group(
    child: &mut tokio::process::Child,
    process_group: Option<u32>,
) -> Result<std::process::ExitStatus> {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;

    let Some(process_group) = process_group else {
        child
            .start_kill()
            .context("failed to kill terminal child")?;
        return child
            .wait()
            .await
            .context("failed waiting for terminal child");
    };
    let pid = Pid::from_raw(process_group as i32);
    let _ = killpg(pid, Signal::SIGHUP);
    if let Ok(status) = tokio::time::timeout(PROCESS_SIGNAL_GRACE, child.wait()).await {
        return status.context("failed waiting for terminal child after SIGHUP");
    }
    let _ = killpg(pid, Signal::SIGTERM);
    if let Ok(status) = tokio::time::timeout(PROCESS_SIGNAL_GRACE, child.wait()).await {
        return status.context("failed waiting for terminal child after SIGTERM");
    }
    let _ = killpg(pid, Signal::SIGKILL);
    child
        .wait()
        .await
        .context("failed waiting for terminal child after SIGKILL")
}

async fn send_json<S, T>(sink: &mut S, value: &T) -> Result<()>
where
    S: futures_util::Sink<Message> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
    T: serde::Serialize,
{
    let json = serde_json::to_string(value).context("failed to encode terminal frame")?;
    sink.send(Message::Text(json.into()))
        .await
        .context("failed to send terminal frame")
}

fn terminal_websocket_url(server_url: &str, terminal_id: &TerminalId) -> Result<url::Url> {
    let mut url = url::Url::parse(server_url).context("invalid server_url")?;
    let scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        "ws" => "ws",
        "wss" => "wss",
        other => anyhow::bail!("unsupported server_url scheme `{other}`"),
    };
    url.set_scheme(scheme)
        .map_err(|_| anyhow::anyhow!("failed to convert server_url scheme"))?;
    url.set_path(&format!(
        "/api/v1/agent/terminals/{}/ws",
        terminal_id.as_str()
    ));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn validate_size(rows: u16, cols: u16) -> Result<()> {
    if !(1..=300).contains(&rows) || !(1..=500).contains(&cols) {
        anyhow::bail!("terminal size is outside supported bounds");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_url_uses_dedicated_agent_path() {
        let id = TerminalId::new("term-1").expect("terminal id");
        let url = terminal_websocket_url("https://example.com/base", &id).expect("url");
        assert_eq!(
            url.as_str(),
            "wss://example.com/api/v1/agent/terminals/term-1/ws"
        );
    }

    #[test]
    fn detached_replay_drops_oldest_output_at_bound() {
        let mut replay = VecDeque::new();
        let mut replay_bytes = 0;
        for marker in 0_u8..10 {
            push_local_replay(
                Message::Binary(vec![marker; TERMINAL_REPLAY_BYTES / 4].into()),
                &mut replay,
                &mut replay_bytes,
            );
        }

        assert!(replay_bytes <= TERMINAL_REPLAY_BYTES);
        assert_eq!(replay.len(), 4);
        assert_eq!(
            replay.front().and_then(|frame| match frame {
                Message::Binary(bytes) => bytes.first().copied(),
                _ => None,
            }),
            Some(6)
        );
    }
}
