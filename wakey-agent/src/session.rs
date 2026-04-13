use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use std::net::IpAddr;
use std::time::Instant;
use tokio::time::{Duration, MissedTickBehavior, interval, sleep};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, info_span, warn};

use crate::config::AgentConfig;
use crate::dispatch::dispatch_command;
use crate::protocol::{AgentCommand, ClientMessage, ErrorPayload, ServerMessage};

pub async fn run(config: AgentConfig) -> Result<()> {
    let mut backoff = config.reconnect_base_ms.max(100);
    loop {
        match run_once(&config).await {
            Ok(()) => {
                backoff = config.reconnect_base_ms.max(100);
            }
            Err(err) => {
                warn!(error = %err, backoff_ms = backoff, "agent session ended; reconnecting");
                sleep(Duration::from_millis(backoff)).await;
                backoff = next_backoff_ms(backoff, config.reconnect_max_ms);
            }
        }
    }
}

async fn run_once(config: &AgentConfig) -> Result<()> {
    let session_id = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default()
    );
    let span = info_span!("agent_session", session_id = %session_id, agent_id = %config.agent_id);
    let _span_guard = span.enter();

    let ws_url = websocket_url(&config.server_url)?;
    info!(%ws_url, agent_id = %config.agent_id, "connecting agent websocket");
    if let Some((dns_resolve_ms, resolved_addrs)) = dns_resolution_diagnostics(&ws_url).await {
        info!(
            host = %ws_url.host_str().unwrap_or(""),
            dns_resolve_ms,
            resolved_addrs,
            "agent websocket dns resolved"
        );
        if dns_resolve_ms > 5_000 {
            warn!(
                host = %ws_url.host_str().unwrap_or(""),
                dns_resolve_ms,
                resolved_addrs,
                "agent websocket dns resolution was slow"
            );
        }
    }
    let connect_started = Instant::now();
    let (stream, _) = connect_async(ws_url.as_str())
        .await
        .context("failed to connect websocket")?;
    let ws_connect_ms = connect_started.elapsed().as_millis() as u64;
    info!(%ws_url, agent_id = %config.agent_id, ws_connect_ms, "agent websocket connected");
    if ws_connect_ms > 10_000 {
        warn!(%ws_url, agent_id = %config.agent_id, ws_connect_ms, "agent websocket connect was slow");
    }
    let (mut sink, mut source) = stream.split();

    send_json(
        &mut sink,
        &ClientMessage::Hello {
            agent_id: config.agent_id.clone(),
        },
    )
    .await?;
    send_json(
        &mut sink,
        &ClientMessage::Auth {
            agent_id: config.agent_id.clone(),
            agent_token: config.agent_token.clone(),
        },
    )
    .await?;
    info!(agent_id = %config.agent_id, "agent websocket session authenticated");

    let mut heartbeat = interval(Duration::from_secs(30));
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                send_json(&mut sink, &ClientMessage::Heartbeat {
                    agent_id: config.agent_id.clone(),
                }).await?;
                debug!(agent_id = %config.agent_id, "heartbeat sent");
            }
            maybe_msg = source.next() => {
                let msg = match maybe_msg {
                    Some(msg) => msg.context("websocket frame failed")?,
                    None => {
                        info!("websocket stream closed by server");
                        anyhow::bail!("websocket closed by server");
                    }
                };

                match msg {
                    Message::Text(text) => {
                        match serde_json::from_str::<ServerMessage>(&text) {
                            Ok(message) => {
                                handle_server_message(&mut sink, message).await?;
                            }
                            Err(err) => {
                                // Allow the server to introduce extra frame types without
                                // forcing reconnects for older agents.
                                warn!(error = %err, payload = %text, "ignoring unknown server message");
                            }
                        }
                    }
                    Message::Ping(payload) => {
                        sink.send(Message::Pong(payload)).await.context("failed to send pong")?;
                    }
                    Message::Pong(_) => {}
                    Message::Close(frame) => {
                        info!(close = ?frame, "received close frame from server");
                        anyhow::bail!("websocket closed: {:?}", frame);
                    }
                    Message::Binary(_) => {
                        debug!("ignoring unexpected binary websocket frame");
                    }
                    Message::Frame(_) => {}
                }
            }
        }
    }
}

pub fn next_backoff_ms(current_ms: u64, max_ms: u64) -> u64 {
    let cap = max_ms.max(current_ms);
    current_ms.saturating_mul(2).min(cap)
}

async fn handle_server_message<S>(sink: &mut S, message: ServerMessage) -> Result<()>
where
    S: SinkExt<Message> + Unpin,
    <S as futures_util::Sink<Message>>::Error: std::error::Error + Send + Sync + 'static,
{
    match message {
        ServerMessage::Command {
            request_id,
            command,
        } => {
            let kind = command_kind(&command);
            info!(request_id = %request_id, command = %kind, "received command from control-plane");
            match dispatch_command(command).await {
                Ok(result) => {
                    info!(request_id = %request_id, command = %kind, "command execution completed");
                    send_command_result(sink, request_id, kind, result).await?;
                }
                Err(err) => {
                    error!(request_id = %request_id, command = %kind, error = %err, "command dispatch failed");
                    send_json(
                        sink,
                        &ClientMessage::Error {
                            request_id,
                            error: ErrorPayload {
                                code: "command_dispatch_failed".into(),
                                message: err.to_string(),
                                retryable: None,
                            },
                        },
                    )
                    .await?;
                }
            }
        }
    }
    Ok(())
}

async fn send_command_result<S>(
    sink: &mut S,
    request_id: crate::protocol::RequestId,
    kind: &str,
    result: crate::protocol::CommandResult,
) -> Result<()>
where
    S: SinkExt<Message> + Unpin,
    <S as futures_util::Sink<Message>>::Error: std::error::Error + Send + Sync + 'static,
{
    if let Err(err) = send_json(
        sink,
        &ClientMessage::Result {
            request_id: request_id.clone(),
            result,
        },
    )
    .await
    {
        error!(
            request_id = %request_id,
            command = %kind,
            error = %err,
            "failed to send command result; sending explicit error frame"
        );

        send_json(
            sink,
            &ClientMessage::Error {
                request_id,
                error: ErrorPayload {
                    code: "command_result_serialize_failed".into(),
                    message: err.to_string(),
                    retryable: Some(true),
                },
            },
        )
        .await?;
    }

    Ok(())
}

async fn send_json<S>(sink: &mut S, message: &ClientMessage) -> Result<()>
where
    S: SinkExt<Message> + Unpin,
    <S as futures_util::Sink<Message>>::Error: std::error::Error + Send + Sync + 'static,
{
    let payload =
        serde_json::to_string(message).context("failed to serialize websocket message")?;
    debug!(message_type = %client_message_kind(message), "sending websocket message");
    sink.send(Message::Text(payload.into()))
        .await
        .context("failed to send websocket message")?;
    Ok(())
}

fn command_kind(command: &AgentCommand) -> &'static str {
    match command {
        AgentCommand::Status(_) => "status",
        AgentCommand::Leases(_) => "leases",
        AgentCommand::Devs(_) => "devs",
        AgentCommand::Inventory(_) => "inventory",
        AgentCommand::Wake(_) => "wake",
    }
}

fn client_message_kind(message: &ClientMessage) -> &'static str {
    match message {
        ClientMessage::Hello { .. } => "hello",
        ClientMessage::Auth { .. } => "auth",
        ClientMessage::Heartbeat { .. } => "heartbeat",
        ClientMessage::Result { .. } => "result",
        ClientMessage::Error { .. } => "error",
    }
}

pub fn websocket_url(server_url: &str) -> Result<url::Url> {
    let base = url::Url::parse(server_url).context("invalid server_url")?;
    let scheme = match base.scheme() {
        "http" => "ws",
        "https" => "wss",
        "ws" => "ws",
        "wss" => "wss",
        other => anyhow::bail!("unsupported server_url scheme `{other}`"),
    };

    let mut url = base;
    url.set_scheme(scheme)
        .map_err(|_| anyhow::anyhow!("failed to convert server_url scheme"))?;
    url.set_path("/api/v1/agent/ws");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

async fn dns_resolution_diagnostics(ws_url: &url::Url) -> Option<(u64, usize)> {
    let host = ws_url.host_str()?;
    if host.parse::<IpAddr>().is_ok() {
        return None;
    }
    let port = ws_url.port_or_known_default()?;

    let started = Instant::now();
    match tokio::net::lookup_host((host, port)).await {
        Ok(addrs) => Some((started.elapsed().as_millis() as u64, addrs.count())),
        Err(err) => {
            warn!(host, port, error = %err, "agent websocket dns resolution failed");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use futures_util::Sink;
    use serde_json::Value;
    use tokio_tungstenite::tungstenite::Message;

    use super::{next_backoff_ms, send_command_result, websocket_url};
    use crate::protocol::{CommandResult, RequestId};

    struct RecordingSink {
        fail_sends_remaining: usize,
        sent: VecDeque<Message>,
    }

    impl RecordingSink {
        fn new(fail_sends_remaining: usize) -> Self {
            Self {
                fail_sends_remaining,
                sent: VecDeque::new(),
            }
        }
    }

    impl Sink<Message> for RecordingSink {
        type Error = io::Error;

        fn poll_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn start_send(mut self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
            if self.fail_sends_remaining > 0 {
                self.fail_sends_remaining -= 1;
                return Err(io::Error::other("injected send failure"));
            }
            self.sent.push_back(item);
            Ok(())
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
    }

    #[test]
    fn websocket_url_uses_expected_path() {
        let url = websocket_url("https://example.com/control").expect("url");
        assert_eq!(url.as_str(), "wss://example.com/api/v1/agent/ws");
    }

    #[test]
    fn backoff_doubles_until_cap() {
        assert_eq!(next_backoff_ms(1_000, 30_000), 2_000);
        assert_eq!(next_backoff_ms(16_000, 30_000), 30_000);
        assert_eq!(next_backoff_ms(30_000, 30_000), 30_000);
    }

    #[test]
    fn backoff_never_shrinks_when_max_is_lower() {
        assert_eq!(next_backoff_ms(8_000, 1_000), 8_000);
    }

    #[tokio::test]
    async fn send_command_result_emits_result_on_success() {
        let mut sink = RecordingSink::new(0);
        let request_id = RequestId::try_from("req-success".to_string()).expect("request id");

        send_command_result(
            &mut sink,
            request_id,
            "wake",
            CommandResult::Wake(wakey_core::WakeResult { result: vec![] }),
        )
        .await
        .expect("send should succeed");

        assert_eq!(sink.sent.len(), 1);
        let Some(Message::Text(payload)) = sink.sent.pop_front() else {
            panic!("expected text websocket message");
        };
        let v: Value = serde_json::from_str(payload.as_ref()).expect("json");
        assert_eq!(v["type"], "result");
    }

    #[tokio::test]
    async fn send_command_result_falls_back_to_error_frame() {
        let mut sink = RecordingSink::new(1);
        let request_id = RequestId::try_from("req-fallback".to_string()).expect("request id");

        send_command_result(
            &mut sink,
            request_id,
            "devs",
            CommandResult::Devs { rows: vec![] },
        )
        .await
        .expect("fallback error frame should be sent");

        assert_eq!(sink.sent.len(), 1);
        let Some(Message::Text(payload)) = sink.sent.pop_front() else {
            panic!("expected text websocket message");
        };
        let v: Value = serde_json::from_str(payload.as_ref()).expect("json");
        assert_eq!(v["type"], "error");
        assert_eq!(v["error"]["code"], "command_result_serialize_failed");
    }
}
