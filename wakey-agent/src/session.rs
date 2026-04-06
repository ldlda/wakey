use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::time::{Duration, MissedTickBehavior, interval, sleep};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

use crate::config::AgentConfig;
use crate::dispatch::dispatch_command;
use crate::protocol::{ClientMessage, ServerMessage};

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
                backoff = (backoff.saturating_mul(2)).min(config.reconnect_max_ms.max(backoff));
            }
        }
    }
}

async fn run_once(config: &AgentConfig) -> Result<()> {
    let ws_url = websocket_url(&config.server_url)?;
    info!(%ws_url, agent_id = %config.agent_id, "connecting agent websocket");
    let (stream, _) = connect_async(ws_url.as_str())
        .await
        .context("failed to connect websocket")?;
    let (mut sink, mut source) = stream.split();

    send_json(&mut sink, &ClientMessage::Hello {
        agent_id: config.agent_id.clone(),
    })
    .await?;
    send_json(
        &mut sink,
        &ClientMessage::Auth {
            agent_id: config.agent_id.clone(),
            agent_token: config.agent_token.clone(),
        },
    )
    .await?;

    let mut heartbeat = interval(Duration::from_secs(30));
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                send_json(&mut sink, &ClientMessage::Heartbeat {
                    agent_id: config.agent_id.clone(),
                }).await?;
            }
            maybe_msg = source.next() => {
                let msg = match maybe_msg {
                    Some(msg) => msg.context("websocket frame failed")?,
                    None => anyhow::bail!("websocket closed by server"),
                };

                match msg {
                    Message::Text(text) => {
                        let message: ServerMessage = serde_json::from_str(&text)
                            .context("failed to decode server message")?;
                        handle_server_message(&mut sink, message).await?;
                    }
                    Message::Ping(payload) => {
                        sink.send(Message::Pong(payload)).await.context("failed to send pong")?;
                    }
                    Message::Pong(_) => {}
                    Message::Close(frame) => {
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

async fn handle_server_message<S>(sink: &mut S, message: ServerMessage) -> Result<()>
where
    S: SinkExt<Message> + Unpin,
    <S as futures_util::Sink<Message>>::Error: std::error::Error + Send + Sync + 'static,
{
    match message {
        ServerMessage::Command {
            request_id,
            command,
        } => match dispatch_command(command).await {
            Ok(result) => {
                send_json(sink, &ClientMessage::Result { request_id, result }).await?;
            }
            Err(err) => {
                error!(request_id, error = %err, "command dispatch failed");
                send_json(
                    sink,
                    &ClientMessage::Error {
                        request_id,
                        error: err.to_string(),
                    },
                )
                .await?;
            }
        },
    }
    Ok(())
}

async fn send_json<S>(sink: &mut S, message: &ClientMessage) -> Result<()>
where
    S: SinkExt<Message> + Unpin,
    <S as futures_util::Sink<Message>>::Error: std::error::Error + Send + Sync + 'static,
{
    let payload = serde_json::to_string(message).context("failed to serialize websocket message")?;
    sink.send(Message::Text(payload))
        .await
        .context("failed to send websocket message")?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::websocket_url;

    #[test]
    fn websocket_url_uses_expected_path() {
        let url = websocket_url("https://example.com/control").expect("url");
        assert_eq!(url.as_str(), "wss://example.com/api/v1/agent/ws");
    }
}
