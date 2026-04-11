use anyhow::{Context, Result};
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use wakey_agent::protocol::{ErrorPayload, RequestId, ServerMessage};

use crate::runtime::{AgentReply, AppState};

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum IncomingClientMessage {
    Hello {
        agent_id: String,
    },
    Auth {
        agent_id: String,
        agent_token: String,
    },
    Heartbeat {
        agent_id: String,
    },
    Result {
        request_id: RequestId,
        result: serde_json::Value,
    },
    Error {
        request_id: RequestId,
        error: ErrorPayload,
    },
}

pub async fn agent_ws(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_agent_socket(state, socket))
}

async fn handle_agent_socket(state: AppState, socket: WebSocket) {
    let (mut write, mut read) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMessage>();

    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let encoded = match serde_json::to_string(&msg) {
                Ok(s) => s,
                Err(err) => {
                    warn!(error = %err, "failed to encode server websocket message");
                    continue;
                }
            };
            if let Err(err) = write.send(Message::Text(encoded.into())).await {
                warn!(error = %err, "failed to send websocket message");
                break;
            }
        }
    });

    let mut authed_agent_id: Option<String> = None;

    loop {
        let frame = read.next().await;
        let msg = match frame {
            Some(Ok(msg)) => msg,
            Some(Err(err)) => {
                warn!(error = %err, "agent websocket receive error");
                break;
            }
            None => break,
        };

        match msg {
            Message::Text(text) => {
                if let Err(err) = process_agent_text(&state, &tx, &mut authed_agent_id, &text).await
                {
                    warn!(error = %err, "closing agent websocket due to protocol/auth error");
                    break;
                }
            }
            Message::Ping(_) => {}
            Message::Pong(_) => {}
            Message::Close(_) => break,
            Message::Binary(_) => {
                debug!("ignoring unexpected binary websocket frame");
            }
        }
    }

    if let Some(agent_id) = authed_agent_id {
        info!(agent_id = %agent_id, "agent disconnected");
        state.sessions.write().await.remove(&agent_id);
    }

    writer.abort();
}

async fn process_agent_text(
    state: &AppState,
    tx: &mpsc::UnboundedSender<ServerMessage>,
    authed_agent_id: &mut Option<String>,
    text: &str,
) -> Result<()> {
    let message: IncomingClientMessage =
        serde_json::from_str(text).context("invalid client websocket payload")?;

    match message {
        IncomingClientMessage::Hello { agent_id } => {
            debug!(agent_id = %agent_id, "agent hello received");
        }
        IncomingClientMessage::Auth {
            agent_id,
            agent_token,
        } => {
            if !state
                .store
                .verify_agent_token(&agent_id, &agent_token)
                .await
            {
                anyhow::bail!("agent auth rejected");
            }
            state
                .sessions
                .write()
                .await
                .insert(agent_id.clone(), tx.clone());
            *authed_agent_id = Some(agent_id.clone());
            info!(agent_id = %agent_id, "agent authenticated");
        }
        IncomingClientMessage::Heartbeat { agent_id } => {
            if authed_agent_id.as_deref() != Some(agent_id.as_str()) {
                anyhow::bail!("heartbeat for unauthenticated or mismatched agent");
            }
            debug!(agent_id = %agent_id, "heartbeat received");
        }
        IncomingClientMessage::Result { request_id, result } => {
            if authed_agent_id.is_none() {
                anyhow::bail!("result before auth");
            }
            let key = request_id.as_str().to_string();
            if let Some(waiter) = state.pending.lock().await.remove(&key) {
                let _ = waiter.send(AgentReply::Result(result));
            }
        }
        IncomingClientMessage::Error { request_id, error } => {
            if authed_agent_id.is_none() {
                anyhow::bail!("error before auth");
            }
            let key = request_id.as_str().to_string();
            if let Some(waiter) = state.pending.lock().await.remove(&key) {
                let _ = waiter.send(AgentReply::Error(error));
            }
        }
    }

    Ok(())
}
