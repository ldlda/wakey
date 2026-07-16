use std::collections::HashMap;
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
const TERMINAL_SCROLLBACK_ROWS: usize = 5_000;
const PROCESS_SIGNAL_GRACE: Duration = Duration::from_secs(1);

/// Tracks the terminal's current rendered state while the browser is detached.
///
/// A snapshot is terminal escape output, so the browser can restore the screen
/// through its normal parser without a second state protocol.
struct TerminalState {
    parser: vt100::Parser,
}

impl TerminalState {
    fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: vt100::Parser::new(rows, cols, TERMINAL_SCROLLBACK_ROWS),
        }
    }

    fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.screen_mut().set_size(rows, cols);
    }

    fn snapshot(&self) -> Vec<u8> {
        self.parser
            .screen()
            .snapshot_formatted(TERMINAL_SCROLLBACK_ROWS)
    }
}

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
            let result = run_terminal(&config, &terminal_id, rows, cols, cancel_rx, relay_rx).await;
            // Inventory is authoritative. Remove the stopped worker before
            // notifying the control session, which may immediately resync it.
            remove_completed(&active, terminal_id.as_str());
            if let Err(err) = result {
                warn!(terminal_id = %terminal_id, error = %err, "terminal worker failed");
                let _ = events.send(TerminalManagerEvent {
                    terminal_id: terminal_id.clone(),
                    error: err.to_string(),
                });
            }
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
        let mut active = self.active.lock().expect("terminal manager poisoned");
        let session = active
            .get(terminal_id.as_str())
            .with_context(|| format!("terminal session {terminal_id} is not active"))?;
        if session.relay_credentials.send(relay_token).is_err() {
            active.remove(terminal_id.as_str());
            anyhow::bail!("terminal session {terminal_id} has stopped");
        }
        Ok(())
    }

    pub fn sessions(&self) -> Vec<AgentTerminalSession> {
        let mut active = self.active.lock().expect("terminal manager poisoned");
        active.retain(|_, terminal| !terminal.relay_credentials.is_closed());
        active
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
    let mut terminal_state = TerminalState::new(rows, cols);
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
                        terminal_state.resize(rows, cols);
                    }
                    RelayInput::Snapshot { generation } if generation == relay_generation => {
                        send_terminal_snapshot(terminal_state.snapshot(), &mut relay_output).await;
                        if let Err(err) = writer.refresh() {
                            warn!(terminal_id = %terminal_id, error = %err, "terminal redraw signal failed");
                        }
                    }
                    RelayInput::Close { generation } if generation == relay_generation => {
                        requested_close = true;
                        break;
                    }
                    RelayInput::Connected { generation, output } if generation == relay_generation => {
                        relay_output = Some(output);
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
                let config = config.clone();
                let terminal_id = terminal_id.clone();
                let relay_input_tx = relay_input_tx.clone();
                relay_task = Some(tokio::spawn(async move {
                    if let Err(err) = run_terminal_relay(RelayConnection {
                        config,
                        terminal_id: terminal_id.clone(),
                        relay_token,
                        generation,
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
                    Ok(count) => {
                        terminal_state.process(&output[..count]);
                        send_terminal_output(
                            Message::Binary(output[..count].to_vec().into()),
                            &mut relay_output,
                        ).await;
                    }
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
    Snapshot {
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
                        TerminalControl::Snapshot => {
                            relay.input.send(RelayInput::Snapshot {
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

async fn send_terminal_output(frame: Message, relay: &mut Option<mpsc::Sender<Message>>) {
    if let Some(tx) = relay.as_ref()
        && tx.send(frame).await.is_err()
    {
        *relay = None;
    }
}

async fn send_terminal_snapshot(snapshot: Vec<u8>, relay: &mut Option<mpsc::Sender<Message>>) {
    let Some(tx) = relay.as_ref() else {
        return;
    };
    for chunk in snapshot.chunks(MAX_TERMINAL_FRAME_BYTES) {
        if tx
            .send(Message::Binary(chunk.to_vec().into()))
            .await
            .is_err()
        {
            *relay = None;
            return;
        }
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

    fn manager_with_stopped_terminal(terminal_id: &str) -> TerminalManager {
        let (cancel, cancel_rx) = oneshot::channel();
        drop(cancel_rx);
        let (relay_credentials, relay_rx) = mpsc::unbounded_channel();
        drop(relay_rx);
        let (events, event_rx) = mpsc::unbounded_channel();
        drop(event_rx);
        TerminalManager {
            active: Arc::new(Mutex::new(HashMap::from([(
                terminal_id.to_string(),
                ActiveTerminal {
                    cancel,
                    relay_credentials,
                    created_at_unix: 42,
                },
            )]))),
            max_sessions: 2,
            events,
        }
    }

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
    fn failed_resume_removes_stopped_terminal_from_inventory() {
        let manager = manager_with_stopped_terminal("stopped-terminal");
        let terminal_id = TerminalId::new("stopped-terminal").expect("terminal id");

        assert!(manager.resume(&terminal_id, "replacement".into()).is_err());
        assert!(manager.sessions().is_empty());
    }

    #[test]
    fn inventory_prunes_terminal_with_stopped_worker() {
        let manager = manager_with_stopped_terminal("stopped-terminal");

        assert!(manager.sessions().is_empty());
    }

    #[test]
    fn snapshot_reconstructs_screen_content_and_cursor() {
        let mut state = TerminalState::new(24, 80);
        state.process(b"hello\r\n\x1b[31mred\x1b[0m\x1b[10;20Hcursor");

        let mut restored = vt100::Parser::new(24, 80, 0);
        restored.process(b"stale browser contents\x1b[24;80Hjunk");
        restored.process(&state.snapshot());

        assert_eq!(
            restored.screen().contents(),
            state.parser.screen().contents()
        );
        assert_eq!(
            restored.screen().cursor_position(),
            state.parser.screen().cursor_position()
        );
    }

    #[test]
    fn snapshot_reconstructs_terminal_input_modes() {
        let mut state = TerminalState::new(24, 80);
        state.process(b"\x1b[?1h\x1b[?1000h\x1b[?2004h");

        let mut restored = vt100::Parser::new(24, 80, 0);
        restored.process(&state.snapshot());

        assert!(restored.screen().application_cursor());
        assert!(restored.screen().bracketed_paste());
        assert_eq!(
            restored.screen().mouse_protocol_mode(),
            state.parser.screen().mouse_protocol_mode()
        );
    }

    #[test]
    fn terminal_state_tracks_resize() {
        let mut state = TerminalState::new(24, 80);
        state.resize(40, 120);
        assert_eq!(state.parser.screen().size(), (40, 120));
    }

    #[test]
    fn snapshot_reconstructs_agent_scrollback() {
        let mut state = TerminalState::new(3, 20);
        for line in 0..10 {
            if line == 0 {
                state.process(b"\x1b[31mhistory 0\x1b[0m\r\n");
            } else {
                state.process(format!("history {line}\r\n").as_bytes());
            }
        }
        let expected_screen = state.parser.screen().contents();

        let mut restored = vt100::Parser::new(3, 20, TERMINAL_SCROLLBACK_ROWS);
        restored.process(&state.snapshot());

        assert_eq!(restored.screen().contents(), expected_screen);
        restored.screen_mut().set_scrollback(usize::MAX);
        assert!(restored.screen().scrollback() >= 8);
        assert!(restored.screen().contents().contains("history 0"));
        assert_eq!(
            restored
                .screen()
                .cell(0, 0)
                .expect("first history cell")
                .fgcolor(),
            vt100::Color::Idx(1)
        );
    }

    #[test]
    fn snapshot_restores_alternate_screen_mode() {
        let mut state = TerminalState::new(3, 20);
        state.process(b"shell\r\n\x1b[?1049hfull-screen");

        let mut restored = vt100::Parser::new(3, 20, TERMINAL_SCROLLBACK_ROWS);
        restored.process(&state.snapshot());

        assert!(restored.screen().alternate_screen());
        assert_eq!(restored.screen().contents(), "full-screen");
    }

    #[test]
    fn alternate_screen_snapshot_restores_hidden_primary_on_exit() {
        let mut state = TerminalState::new(3, 20);
        state.process(b"\x1b[31mshell history\r\n$ btop\x1b[?1049h\x1b[34mbtop dashboard");

        let mut restored = vt100::Parser::new(3, 20, TERMINAL_SCROLLBACK_ROWS);
        restored.process(&state.snapshot());

        assert!(restored.screen().alternate_screen());
        assert_eq!(restored.screen().contents(), "btop dashboard");
        assert_eq!(restored.screen().fgcolor(), vt100::Color::Idx(4));

        // Compare the reattached terminal with the original parser after both
        // receive the application's real alternate-screen exit sequence.
        state.process(b"\x1b[?1049l");
        restored.process(b"\x1b[?1049l");
        assert_eq!(
            restored.screen().contents(),
            state.parser.screen().contents()
        );
        assert_eq!(
            restored.screen().cursor_position(),
            state.parser.screen().cursor_position()
        );
        assert_eq!(restored.screen().fgcolor(), vt100::Color::Idx(1));

        restored.screen_mut().set_scrollback(usize::MAX);
        assert!(restored.screen().contents().contains("shell history"));
        assert!(!restored.screen().contents().contains("btop dashboard"));
    }

    #[test]
    fn snapshot_does_not_change_the_source_viewport_or_state() {
        let mut state = TerminalState::new(3, 20);
        for line in 0..10 {
            state.process(format!("history {line}\r\n").as_bytes());
        }
        state.parser.screen_mut().set_scrollback(4);

        let before_contents = state.parser.screen().contents();
        let before_state = state.parser.screen().state_formatted();
        let before_scrollback = state.parser.screen().scrollback();
        let _ = state.snapshot();

        assert_eq!(state.parser.screen().contents(), before_contents);
        assert_eq!(state.parser.screen().state_formatted(), before_state);
        assert_eq!(state.parser.screen().scrollback(), before_scrollback);
    }

    #[tokio::test]
    async fn snapshot_is_split_at_the_terminal_frame_limit() {
        let snapshot = vec![7; MAX_TERMINAL_FRAME_BYTES * 2 + 1];
        let (tx, mut rx) = mpsc::channel(3);
        let mut relay = Some(tx);

        send_terminal_snapshot(snapshot.clone(), &mut relay).await;

        let mut restored = Vec::new();
        for _ in 0..3 {
            let Message::Binary(chunk) = rx.recv().await.expect("snapshot chunk") else {
                panic!("snapshot chunks must be binary");
            };
            assert!(chunk.len() <= MAX_TERMINAL_FRAME_BYTES);
            restored.extend_from_slice(&chunk);
        }
        assert_eq!(restored, snapshot);
    }
}
