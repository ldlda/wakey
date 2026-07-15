use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use crate::config::AgentConfig;
use crate::protocol::{TerminalAgentHandshake, TerminalControl, TerminalId};
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

const MAX_TERMINAL_FRAME_BYTES: usize = 64 * 1024;
const PROCESS_SIGNAL_GRACE: Duration = Duration::from_secs(1);

/// Owns cancellation handles for terminal workers started by the control socket.
pub struct TerminalManager {
    active: Arc<Mutex<HashMap<String, oneshot::Sender<()>>>>,
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
        {
            let mut active = self.active.lock().expect("terminal manager poisoned");
            if active.contains_key(&terminal_key) {
                anyhow::bail!("terminal session {terminal_key} is already active");
            }
            if active.len() >= self.max_sessions {
                anyhow::bail!("agent terminal session limit reached");
            }
            active.insert(terminal_key.clone(), cancel_tx);
        }

        let config = config.clone();
        let active = Arc::downgrade(&self.active);
        let events = self.events.clone();
        tokio::spawn(async move {
            if let Err(err) =
                run_terminal(&config, &terminal_id, &relay_token, rows, cols, cancel_rx).await
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
            .is_some_and(|cancel| cancel.send(()).is_ok())
    }
}

impl Drop for TerminalManager {
    fn drop(&mut self) {
        if Arc::strong_count(&self.active) == 1
            && let Ok(mut active) = self.active.lock()
        {
            for (_, cancel) in active.drain() {
                let _ = cancel.send(());
            }
        }
    }
}

fn remove_completed(active: &Weak<Mutex<HashMap<String, oneshot::Sender<()>>>>, terminal_id: &str) {
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
    relay_token: &str,
    rows: u16,
    cols: u16,
    mut cancel: oneshot::Receiver<()>,
) -> Result<()> {
    let ws_url = terminal_websocket_url(&config.server_url, terminal_id)?;
    let (stream, _) = tokio::select! {
        _ = &mut cancel => return Ok(()),
        result = tokio_tungstenite::connect_async(ws_url.as_str()) => {
            result.context("failed to connect terminal relay websocket")?
        }
    };
    let (mut sink, mut source) = stream.split();
    send_json(
        &mut sink,
        &TerminalAgentHandshake::Auth {
            agent_id: config.agent_id.clone(),
            relay_token: relay_token.to_string(),
        },
    )
    .await?;

    let terminal = match wakey::wakey_linux::terminal::TerminalPty::spawn(
        Path::new(&config.terminal.shell),
        rows,
        cols,
    ) {
        Ok(terminal) => terminal,
        Err(err) => {
            let _ = send_json(
                &mut sink,
                &TerminalControl::Error {
                    code: "terminal_spawn_failed".into(),
                    message: err.to_string(),
                },
            )
            .await;
            let _ = sink.send(Message::Close(None)).await;
            return Err(err);
        }
    };
    let wakey::wakey_linux::terminal::TerminalPty {
        mut reader,
        mut writer,
        mut child,
    } = terminal;
    let process_group = child.id();
    send_json(&mut sink, &TerminalControl::Ready).await?;
    info!(terminal_id = %terminal_id, shell = %config.terminal.shell.display(), "terminal PTY ready");

    let mut output = [0_u8; 16 * 1024];
    let mut requested_close = false;
    let mut observed_status = None;
    loop {
        tokio::select! {
            _ = &mut cancel => {
                requested_close = true;
                break;
            }
            status = child.wait() => {
                observed_status = Some(status.context("failed waiting for terminal child")?);
                break;
            }
            read = reader.read(&mut output) => {
                match read {
                    Ok(0) => break,
                    Ok(count) => sink
                        .send(Message::Binary(output[..count].to_vec().into()))
                        .await
                        .context("failed to send PTY output")?,
                    // Linux PTY masters commonly report EIO after the slave closes.
                    Err(err) if err.raw_os_error() == Some(5) => break,
                    Err(err) => return Err(err).context("failed to read PTY output"),
                }
            }
            incoming = source.next() => {
                let Some(message) = incoming else { break; };
                match message.context("terminal relay websocket receive failed")? {
                    Message::Binary(bytes) => {
                        if bytes.len() > MAX_TERMINAL_FRAME_BYTES {
                            anyhow::bail!("terminal input frame exceeds size limit");
                        }
                        writer.write_all(&bytes).await.context("failed to write PTY input")?;
                    }
                    Message::Text(text) => {
                        match serde_json::from_str::<TerminalControl>(&text)
                            .context("invalid terminal control frame")?
                        {
                            TerminalControl::Resize { rows, cols } => {
                                validate_size(rows, cols)?;
                                wakey::wakey_linux::terminal::resize_terminal(
                                    &writer, rows, cols,
                                )?;
                            }
                            TerminalControl::Close => {
                                requested_close = true;
                                break;
                            }
                            _ => anyhow::bail!("terminal control frame has invalid direction"),
                        }
                    }
                    Message::Ping(payload) => sink.send(Message::Pong(payload)).await?,
                    Message::Pong(_) => {}
                    Message::Close(_) => {
                        requested_close = true;
                        break;
                    }
                    Message::Frame(_) => {}
                }
            }
        }
    }

    let status = match observed_status {
        Some(status) => status,
        None => terminate_process_group(&mut child, process_group).await?,
    };
    let _ = send_json(
        &mut sink,
        &TerminalControl::Exited {
            exit_code: status.code(),
        },
    )
    .await;
    let _ = sink.send(Message::Close(None)).await;
    info!(terminal_id = %terminal_id, exit_code = ?status.code(), requested_close, "terminal worker exited");
    Ok(())
}

#[cfg(not(unix))]
async fn run_terminal(
    _config: &AgentConfig,
    _terminal_id: &TerminalId,
    _relay_token: &str,
    _rows: u16,
    _cols: u16,
    _cancel: oneshot::Receiver<()>,
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
}
