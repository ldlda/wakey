use std::collections::VecDeque;
use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::config::AgentConfig;
use crate::protocol::{TerminalAgentHandshake, TerminalControl, TerminalId};

pub(super) const MAX_TERMINAL_FRAME_BYTES: usize = 64 * 1024;
pub(super) const RELAY_INPUT_QUEUE: usize = 32;
pub(super) const RELAY_OUTPUT_QUEUE: usize = 32;
pub(super) const PTY_EXIT_DRAIN_TIMEOUT: Duration = Duration::from_secs(1);

/// Buffers ordered terminal output without letting a slow relay grow memory
/// without bound. Once a frame is pending, the PTY read branch is paused until
/// the relay channel has capacity; control input remains independently active.
#[derive(Default)]
pub(super) struct RelayOutputBuffer {
    sender: Option<mpsc::Sender<Message>>,
    pending: VecDeque<Message>,
    snapshot_pending: bool,
    backpressure_wait: Option<wakey_core::telemetry::Operation>,
}

impl RelayOutputBuffer {
    pub(super) fn connect(&mut self, sender: mpsc::Sender<Message>) {
        self.disconnect();
        self.sender = Some(sender);
    }

    pub(super) fn disconnect(&mut self) {
        if let Some(mut wait) = self.backpressure_wait.take() {
            wait.outcome("disconnected");
        }
        self.sender = None;
        self.pending.clear();
        self.snapshot_pending = false;
    }

    pub(super) fn is_connected(&self) -> bool {
        self.sender.is_some()
    }

    pub(super) fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub(super) fn pending_sender(&self) -> Option<mpsc::Sender<Message>> {
        self.has_pending().then(|| self.sender.clone()).flatten()
    }

    pub(super) fn queue_live(&mut self, frame: Message) {
        debug_assert!(
            self.pending.is_empty(),
            "PTY reads must pause while relay output is pending"
        );
        self.queue_frame(frame);
    }

    fn queue_frame(&mut self, frame: Message) {
        let Some(sender) = self.sender.as_ref() else {
            return;
        };
        if !self.pending.is_empty() {
            self.pending.push_back(frame);
            return;
        }
        match sender.try_send(frame) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(frame)) => {
                // One measurement per backlog episode, not per select poll.
                self.backpressure_wait = Some(wakey_core::telemetry::Operation::new(
                    "terminal_backpressure_wait",
                ));
                wakey_core::telemetry::count(
                    wakey_core::telemetry::Counter::TerminalBackpressure,
                    1,
                );
                self.pending.push_back(frame);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => self.disconnect(),
        }
    }

    pub(super) fn queue_snapshot(&mut self, snapshot: Vec<u8>) {
        if self.sender.is_none() || self.snapshot_pending {
            return;
        }
        for chunk in snapshot.chunks(MAX_TERMINAL_FRAME_BYTES) {
            self.queue_frame(Message::Binary(chunk.to_vec().into()));
            if self.sender.is_none() {
                return;
            }
        }
        self.snapshot_pending = self.has_pending();
    }

    pub(super) fn send_reserved(&mut self, permit: mpsc::OwnedPermit<Message>) {
        let frame = self
            .pending
            .pop_front()
            .expect("relay permit requires pending output");
        permit.send(frame);
        if self.pending.is_empty() {
            self.snapshot_pending = false;
            if let Some(mut wait) = self.backpressure_wait.take() {
                wait.outcome("ok");
            }
        }
    }

    fn live_sender(&self) -> Option<mpsc::Sender<Message>> {
        self.sender.clone()
    }
}

pub(super) enum RelayInput {
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
pub(super) struct RelayConnection {
    pub(super) config: AgentConfig,
    pub(super) terminal_id: TerminalId,
    pub(super) relay_token: String,
    pub(super) generation: u64,
    pub(super) output_tx: mpsc::Sender<Message>,
    pub(super) output_rx: mpsc::Receiver<Message>,
    pub(super) input: mpsc::Sender<RelayInput>,
}

#[cfg(unix)]
pub(super) async fn run_terminal_relay(relay: RelayConnection) -> Result<()> {
    let ws_url = terminal_websocket_url(&relay.config.server_url, &relay.terminal_id)?;
    let (stream, _) = tokio_tungstenite::connect_async(ws_url.as_str())
        .await
        .context("failed to connect terminal relay websocket")?;
    let (mut sink, source) = stream.split();
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
        .await
        .map_err(|_| anyhow::anyhow!("terminal worker stopped"))?;

    relay_loop(source, sink, relay.input, relay.generation, relay.output_rx).await
}

async fn relay_loop<S, St>(
    mut source: St,
    mut sink: S,
    input: mpsc::Sender<RelayInput>,
    generation: u64,
    mut output: mpsc::Receiver<Message>,
) -> Result<()>
where
    S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
    St: futures_util::Stream<
            Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>,
        > + Unpin,
{
    let mut worker_stopped = false;
    let mut worker_stop_deadline: Option<tokio::time::Instant> = None;
    // Reserve at most one control so a full worker queue cannot block output.
    let mut pending_input = None;
    let mut close_after_input = false;
    loop {
        tokio::select! {
            biased;
            _ = async {
                match worker_stop_deadline {
                    Some(deadline) => tokio::time::sleep_until(deadline).await,
                    None => std::future::pending().await,
                }
            } => break,
            incoming = source.next(), if !worker_stopped && pending_input.is_none() => {
                let Some(message) = incoming else { break; };
                match message.context("terminal relay websocket receive failed")? {
                    Message::Binary(bytes) => {
                        wakey_core::telemetry::count(wakey_core::telemetry::Counter::TerminalInputBytes, bytes.len() as u64);
                        pending_input = Some(RelayInput::Binary {
                            generation,
                            bytes: bytes.to_vec(),
                        });
                    }
                    Message::Text(text) => match serde_json::from_str::<TerminalControl>(&text)
                        .context("invalid terminal control frame")?
                    {
                        TerminalControl::Resize { rows, cols } => {
                            pending_input = Some(RelayInput::Resize {
                                generation,
                                rows,
                                cols,
                            });
                        }
                        TerminalControl::Snapshot => {
                            pending_input = Some(RelayInput::Snapshot { generation });
                        }
                        TerminalControl::Close => {
                            pending_input = Some(RelayInput::Close { generation });
                            close_after_input = true;
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
            permit = input.reserve(), if !worker_stopped && pending_input.is_some() => {
                match permit {
                    Ok(permit) => {
                        permit.send(pending_input.take().expect("pending terminal input"));
                        if close_after_input {
                            break;
                        }
                    }
                    Err(_) => {
                        worker_stopped = true;
                        pending_input = None;
                        worker_stop_deadline =
                            Some(tokio::time::Instant::now() + PTY_EXIT_DRAIN_TIMEOUT);
                    }
                }
            }
            outgoing = output.recv() => {
                let Some(message) = outgoing else { break; };
                if let Some(deadline) = worker_stop_deadline {
                    tokio::time::timeout_at(deadline, sink.send(message))
                        .await
                        .context("terminal relay output drain timed out")?
                        .context("failed to send terminal relay output")?;
                } else {
                    sink.send(message).await.context("failed to send terminal relay output")?;
                }
            }
        }
    }
    Ok(())
}

pub(super) async fn reserve_relay_output(
    sender: Option<mpsc::Sender<Message>>,
) -> Result<mpsc::OwnedPermit<Message>, ()> {
    match sender {
        Some(sender) => sender.reserve_owned().await.map_err(|_| ()),
        None => std::future::pending().await,
    }
}

/// Flushes frames the select loop left pending, then sends the exit lifecycle
/// frame behind them, bounded by `deadline`: a relay that stays saturated past
/// it drops the backlog rather than holding worker shutdown open. Queuing the
/// exit frame without this would drop it together with the pending buffer when
/// the worker returns.
pub(super) async fn flush_relay_output(
    relay_output: &mut RelayOutputBuffer,
    exit_frame: Option<Message>,
    deadline: Duration,
) {
    let _ = tokio::time::timeout(deadline, async {
        while relay_output.has_pending() {
            let sender = relay_output.pending_sender();
            match reserve_relay_output(sender).await {
                Ok(permit) => relay_output.send_reserved(permit),
                Err(()) => {
                    relay_output.disconnect();
                    return;
                }
            }
        }
        let Some(frame) = exit_frame else {
            return;
        };
        let sender = relay_output
            .pending_sender()
            .or_else(|| relay_output.live_sender());
        match reserve_relay_output(sender).await {
            Ok(permit) => {
                permit.send(frame);
            }
            Err(()) => relay_output.disconnect(),
        }
    })
    .await;
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

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

    #[tokio::test]
    async fn exit_frame_is_delivered_after_saturated_relay_drains() {
        let (tx, mut rx) = mpsc::channel(RELAY_OUTPUT_QUEUE);
        let mut relay_output = RelayOutputBuffer::default();
        relay_output.connect(tx);

        let total = RELAY_OUTPUT_QUEUE * 2;
        for i in 0..total {
            relay_output.queue_frame(Message::Text(format!("out-{i}").into()));
        }
        assert!(relay_output.has_pending());

        let collected = tokio::spawn(async move {
            let mut frames = Vec::new();
            while let Some(frame) = rx.recv().await {
                frames.push(frame);
            }
            frames
        });

        let exit_frame = serde_json::to_string(&TerminalControl::Exited { exit_code: Some(0) })
            .expect("exit frame encodes");
        flush_relay_output(
            &mut relay_output,
            Some(Message::Text(exit_frame.clone().into())),
            PTY_EXIT_DRAIN_TIMEOUT,
        )
        .await;
        relay_output.disconnect();

        let mut frames = collected.await.expect("relay reader completes");
        assert_eq!(frames.len(), total + 1);
        let Some(Message::Text(last)) = frames.pop() else {
            panic!("last frame should be the exit control frame");
        };
        assert_eq!(last.as_str(), exit_frame);
        for (i, frame) in frames.iter().enumerate() {
            let Message::Text(payload) = frame else {
                panic!("expected text frame at position {i}, got {frame:?}");
            };
            assert_eq!(payload.as_str(), format!("out-{i}"));
        }
    }

    #[tokio::test]
    async fn relay_drains_final_output_after_worker_stops_during_concurrent_controls() {
        let (input, input_rx) = mpsc::channel(RELAY_INPUT_QUEUE);
        drop(input_rx);
        let (output_tx, output_rx) = mpsc::channel(RELAY_OUTPUT_QUEUE);
        output_tx
            .send(Message::Binary(vec![1, 2, 3].into()))
            .await
            .expect("live output queues");
        let exit_frame = serde_json::to_string(&TerminalControl::Exited { exit_code: Some(0) })
            .expect("exit frame encodes");
        output_tx
            .send(Message::Text(exit_frame.clone().into()))
            .await
            .expect("exit output queues");
        drop(output_tx);

        let source = futures_util::stream::iter([
            Ok(Message::Binary(vec![4].into())),
            Ok(Message::Text(
                serde_json::to_string(&TerminalControl::Resize {
                    rows: 30,
                    cols: 100,
                })
                .expect("resize encodes")
                .into(),
            )),
            Ok(Message::Text(
                serde_json::to_string(&TerminalControl::Snapshot)
                    .expect("snapshot encodes")
                    .into(),
            )),
        ]);
        let delivered = Arc::new(Mutex::new(Vec::new()));
        let sink = futures_util::sink::unfold(delivered.clone(), |delivered, frame| async move {
            delivered.lock().expect("delivered frames").push(frame);
            Ok::<_, tokio_tungstenite::tungstenite::Error>(delivered)
        });

        relay_loop(source, Box::pin(sink), input, 7, output_rx)
            .await
            .expect("relay drains after worker completion");

        let delivered = delivered.lock().expect("delivered frames");
        assert_eq!(delivered.len(), 2);
        assert_eq!(delivered[0].clone().into_data(), vec![1, 2, 3]);
        let Message::Text(exit) = &delivered[1] else {
            panic!("exit control should follow final bytes");
        };
        assert_eq!(exit.as_str(), exit_frame);
    }

    #[tokio::test]
    async fn snapshot_is_split_at_the_terminal_frame_limit() {
        let snapshot = vec![7; MAX_TERMINAL_FRAME_BYTES * 2 + 1];
        let (tx, mut rx) = mpsc::channel(1);
        let mut relay = RelayOutputBuffer::default();
        relay.connect(tx);
        relay.queue_snapshot(snapshot.clone());

        let mut restored = Vec::new();
        for _ in 0..3 {
            if rx.is_empty() {
                let permit = reserve_relay_output(relay.pending_sender())
                    .await
                    .expect("relay remains connected");
                relay.send_reserved(permit);
            }
            let Message::Binary(chunk) = rx.recv().await.expect("snapshot chunk") else {
                panic!("snapshot chunks must be binary");
            };
            assert!(chunk.len() <= MAX_TERMINAL_FRAME_BYTES);
            restored.extend_from_slice(&chunk);
        }
        assert_eq!(restored, snapshot);
        assert!(!relay.has_pending());
        assert!(relay.is_connected());
    }

    #[tokio::test]
    async fn saturated_relay_output_stays_attached_without_dropping() {
        let (tx, mut rx) = mpsc::channel(1);
        let mut relay = RelayOutputBuffer::default();
        relay.connect(tx);
        relay.queue_live(Message::Binary(vec![1].into()));
        // This read discovers saturation. The worker retains it, then pauses
        // further PTY reads until relay capacity becomes available.
        relay.queue_live(Message::Binary(vec![2].into()));

        assert!(relay.is_connected());
        assert!(relay.has_pending());
        assert!(relay.backpressure_wait.is_some());
        assert_eq!(
            rx.recv().await.expect("original queued frame").into_data(),
            vec![1]
        );
        let permit = reserve_relay_output(relay.pending_sender())
            .await
            .expect("relay remains connected");
        relay.send_reserved(permit);
        assert_eq!(
            rx.recv().await.expect("backpressured frame").into_data(),
            vec![2]
        );
        assert!(!relay.has_pending());
        assert!(relay.backpressure_wait.is_none());
    }

    #[test]
    fn disconnect_finishes_backpressure_episode() {
        let (tx, _rx) = mpsc::channel(1);
        let mut relay = RelayOutputBuffer::default();
        relay.connect(tx);
        relay.queue_live(Message::Binary(vec![1].into()));
        relay.queue_live(Message::Binary(vec![2].into()));
        assert!(relay.backpressure_wait.is_some());
        relay.disconnect();
        assert!(relay.backpressure_wait.is_none());
        assert!(!relay.has_pending());
    }

    #[tokio::test]
    async fn snapshot_waits_behind_pending_live_output() {
        let (tx, mut rx) = mpsc::channel(1);
        let mut relay = RelayOutputBuffer::default();
        relay.connect(tx);
        relay.queue_live(Message::Binary(vec![1].into()));
        relay.queue_live(Message::Binary(vec![2].into()));
        relay.queue_snapshot(vec![3]);

        let mut received = Vec::new();
        for _ in 0..3 {
            if rx.is_empty() {
                let permit = reserve_relay_output(relay.pending_sender())
                    .await
                    .expect("relay remains connected");
                relay.send_reserved(permit);
            }
            received.extend(rx.recv().await.expect("ordered relay frame").into_data());
        }
        assert_eq!(received, vec![1, 2, 3]);
    }

    #[test]
    fn replacement_relay_does_not_receive_old_pending_output() {
        let (old_tx, mut old_rx) = mpsc::channel(1);
        let (new_tx, mut new_rx) = mpsc::channel(1);
        let mut relay = RelayOutputBuffer::default();
        relay.connect(old_tx);
        relay.queue_live(Message::Binary(vec![1].into()));
        relay.queue_live(Message::Binary(vec![2].into()));

        relay.connect(new_tx);
        relay.queue_live(Message::Binary(vec![3].into()));

        assert_eq!(
            old_rx.try_recv().expect("old queued frame").into_data(),
            vec![1]
        );
        assert!(old_rx.try_recv().is_err());
        assert_eq!(
            new_rx.try_recv().expect("new relay frame").into_data(),
            vec![3]
        );
        assert!(!relay.has_pending());
    }
}
