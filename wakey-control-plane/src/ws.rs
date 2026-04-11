use anyhow::{Context, Result};
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{debug, info, info_span, warn};
use uuid::Uuid;
use wakey_agent::protocol::{ErrorPayload, RequestId, ServerMessage};

use crate::runtime::{AgentReply, AppState};
use crate::state::AuditEventInput;

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
    let connection_id = Uuid::new_v4().to_string();
    let span = info_span!("agent_ws_connection", connection_id = %connection_id);
    let _span_guard = span.enter();
    info!("agent websocket upgraded");

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
        debug!("websocket writer loop ended");
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
            None => {
                info!("agent websocket stream ended by peer");
                break;
            }
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
            Message::Close(frame) => {
                info!(close = ?frame, "agent websocket close frame received");
                break;
            }
            Message::Binary(_) => {
                debug!("ignoring unexpected binary websocket frame");
            }
        }
    }

    if let Some(agent_id) = authed_agent_id {
        info!(agent_id = %agent_id, "agent disconnected");
        state.sessions.write().await.remove(&agent_id);
        if let Err(err) = state
            .store
            .append_audit_event(AuditEventInput {
                actor_type: "agent".into(),
                actor_id: Some(agent_id.clone()),
                agent_id: Some(agent_id),
                request_id: None,
                event_type: "agent_ws_disconnect".into(),
                outcome: "ok".into(),
                latency_ms: None,
                message: "agent websocket disconnected".into(),
                metadata: serde_json::json!({}),
            })
            .await
        {
            warn!(error = %err, "failed to append audit event for ws disconnect");
        }
    }

    writer.abort();
    debug!("agent websocket connection cleanup complete");
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
                warn!(agent_id = %agent_id, "agent auth rejected");
                if let Err(err) = state
                    .store
                    .append_audit_event(AuditEventInput {
                        actor_type: "agent".into(),
                        actor_id: Some(agent_id.clone()),
                        agent_id: Some(agent_id),
                        request_id: None,
                        event_type: "agent_ws_auth".into(),
                        outcome: "rejected".into(),
                        latency_ms: None,
                        message: "agent auth rejected".into(),
                        metadata: serde_json::json!({}),
                    })
                    .await
                {
                    warn!(error = %err, "failed to append audit event for auth rejection");
                }
                anyhow::bail!("agent auth rejected");
            }
            state
                .sessions
                .write()
                .await
                .insert(agent_id.clone(), tx.clone());
            *authed_agent_id = Some(agent_id.clone());
            info!(agent_id = %agent_id, "agent authenticated");
            if let Err(err) = state
                .store
                .append_audit_event(AuditEventInput {
                    actor_type: "agent".into(),
                    actor_id: Some(agent_id.clone()),
                    agent_id: Some(agent_id),
                    request_id: None,
                    event_type: "agent_ws_auth".into(),
                    outcome: "ok".into(),
                    latency_ms: None,
                    message: "agent websocket authenticated".into(),
                    metadata: serde_json::json!({}),
                })
                .await
            {
                warn!(error = %err, "failed to append audit event for auth success");
            }
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
            } else {
                debug!(request_id = %key, "dropping unsolicited result from agent");
            }
        }
        IncomingClientMessage::Error { request_id, error } => {
            if authed_agent_id.is_none() {
                anyhow::bail!("error before auth");
            }
            let key = request_id.as_str().to_string();
            if let Some(waiter) = state.pending.lock().await.remove(&key) {
                let _ = waiter.send(AgentReply::Error(error));
            } else {
                debug!(request_id = %key, "dropping unsolicited error from agent");
            }
        }
    }

    Ok(())
}
