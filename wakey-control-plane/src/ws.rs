use anyhow::{Context, Result};
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{debug, info, info_span, warn};
use uuid::Uuid;
use wakey_agent::protocol::{AgentObservation, ErrorPayload, RequestId, ServerMessage};

use crate::runtime::{AgentReply, AgentSession, AppState, SessionEvent};
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
    Observations {
        agent_id: String,
        observations: Vec<AgentObservation>,
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
    let connected_at = Instant::now();

    let (mut write, mut read) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<SessionEvent>();

    let writer = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                SessionEvent::Close => {
                    let _ = write.send(Message::Close(None)).await;
                    break;
                }
                SessionEvent::Message(msg) => {
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
            }
        }
        debug!("websocket writer loop ended");
    });

    let mut authed_agent_id: Option<String> = None;
    let mut hello_at: Option<Instant> = None;

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
                if let Err(err) = process_agent_text(
                    &state,
                    &tx,
                    &connection_id,
                    &mut authed_agent_id,
                    &mut hello_at,
                    connected_at,
                    &text,
                )
                .await
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
        let mut sessions = state.sessions.write().await;
        let should_remove = sessions
            .get(&agent_id)
            .map(|session| session.connection_id == connection_id)
            .unwrap_or(false);
        if should_remove {
            sessions.remove(&agent_id);
        }
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
    tx: &mpsc::UnboundedSender<SessionEvent>,
    connection_id: &str,
    authed_agent_id: &mut Option<String>,
    hello_at: &mut Option<Instant>,
    connected_at: Instant,
    text: &str,
) -> Result<()> {
    let message: IncomingClientMessage =
        serde_json::from_str(text).context("invalid client websocket payload")?;

    match message {
        IncomingClientMessage::Hello { agent_id } => {
            let now = Instant::now();
            if hello_at.is_none() {
                *hello_at = Some(now);
            }
            let connect_to_hello_ms = connected_at.elapsed().as_millis() as u64;
            info!(agent_id = %agent_id, connect_to_hello_ms, "agent hello received");
        }
        IncomingClientMessage::Auth {
            agent_id,
            agent_token,
        } => {
            let connect_to_auth_ms = connected_at.elapsed().as_millis() as u64;
            let hello_to_auth_ms = hello_at.map(|t| now_duration_ms(t.elapsed()));
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
                        latency_ms: Some(connect_to_auth_ms),
                        message: "agent auth rejected".into(),
                        metadata: serde_json::json!({
                            "connect_to_auth_ms": connect_to_auth_ms,
                            "hello_to_auth_ms": hello_to_auth_ms,
                        }),
                    })
                    .await
                {
                    warn!(error = %err, "failed to append audit event for auth rejection");
                }
                anyhow::bail!("agent auth rejected");
            }
            state.sessions.write().await.insert(
                agent_id.clone(),
                AgentSession {
                    connection_id: connection_id.to_string(),
                    tx: tx.clone(),
                },
            );
            *authed_agent_id = Some(agent_id.clone());
            info!(agent_id = %agent_id, connect_to_auth_ms, hello_to_auth_ms = hello_to_auth_ms.unwrap_or(0), "agent authenticated");
            if let Err(err) = state
                .store
                .append_audit_event(AuditEventInput {
                    actor_type: "agent".into(),
                    actor_id: Some(agent_id.clone()),
                    agent_id: Some(agent_id),
                    request_id: None,
                    event_type: "agent_ws_auth".into(),
                    outcome: "ok".into(),
                    latency_ms: Some(connect_to_auth_ms),
                    message: "agent websocket authenticated".into(),
                    metadata: serde_json::json!({
                        "connect_to_auth_ms": connect_to_auth_ms,
                        "hello_to_auth_ms": hello_to_auth_ms,
                    }),
                })
                .await
            {
                warn!(error = %err, "failed to append audit event for auth success");
            }
            let _ = tx.send(SessionEvent::Message(ServerMessage::SyncObservations));
        }
        IncomingClientMessage::Heartbeat { agent_id } => {
            if authed_agent_id.as_deref() != Some(agent_id.as_str()) {
                anyhow::bail!("heartbeat for unauthenticated or mismatched agent");
            }
            ensure_current_session(state, &agent_id, connection_id).await?;
            debug!(agent_id = %agent_id, "heartbeat received");
        }
        IncomingClientMessage::Observations {
            agent_id,
            observations,
        } => {
            if authed_agent_id.as_deref() != Some(agent_id.as_str()) {
                anyhow::bail!("observations for unauthenticated or mismatched agent");
            }
            ensure_current_session(state, &agent_id, connection_id).await?;
            let mut by_kind: BTreeMap<String, Vec<crate::state::AgentDeviceObservationInput>> =
                BTreeMap::new();
            for observation in observations {
                let kind = observation.kind.trim().to_ascii_lowercase();
                by_kind
                    .entry(kind.clone())
                    .or_default()
                    .push(crate::state::AgentDeviceObservationInput {
                        kind,
                        action: observation.action,
                        mac: observation.mac,
                        ip: observation.ip.map(|ip| ip.to_string()),
                        hostname: observation.hostname,
                        first_seen_unix: observation.first_seen_unix,
                        last_seen_unix: observation.last_seen_unix,
                    });
            }
            let mut accepted = 0usize;
            for (kind, inputs) in by_kind {
                match state
                    .store
                    .upsert_agent_observations_snapshot(&agent_id, &kind, inputs)
                    .await
                {
                    Ok(written) => {
                        accepted = accepted.saturating_add(written);
                    }
                    Err(err) => {
                        warn!(agent_id = %agent_id, error = %err, kind = %kind, "failed to store websocket observations");
                        anyhow::bail!("failed to store observations: {err}");
                    }
                }
            }
            debug!(agent_id = %agent_id, accepted, "agent websocket observations accepted");
        }
        IncomingClientMessage::Result { request_id, result } => {
            let agent_id = authed_agent_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("result before auth"))?;
            ensure_current_session(state, agent_id, connection_id).await?;
            let key = request_id.as_str().to_string();
            if let Some(waiter) = state.pending.lock().await.remove(&key) {
                let _ = waiter.send(AgentReply::Result(result));
            } else {
                debug!(request_id = %key, "dropping unsolicited result from agent");
            }
        }
        IncomingClientMessage::Error { request_id, error } => {
            let agent_id = authed_agent_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("error before auth"))?;
            ensure_current_session(state, agent_id, connection_id).await?;
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

fn now_duration_ms(duration: std::time::Duration) -> u64 {
    duration.as_millis() as u64
}

async fn ensure_current_session(
    state: &AppState,
    agent_id: &str,
    connection_id: &str,
) -> Result<()> {
    let sessions = state.sessions.read().await;
    if is_current_session(&sessions, agent_id, connection_id) {
        Ok(())
    } else if sessions.contains_key(agent_id) {
        anyhow::bail!("stale agent session")
    } else {
        anyhow::bail!("agent session not registered")
    }
}

fn is_current_session(
    sessions: &std::collections::HashMap<String, AgentSession>,
    agent_id: &str,
    connection_id: &str,
) -> bool {
    sessions
        .get(agent_id)
        .map(|session| session.connection_id == connection_id)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tokio::sync::mpsc;

    use crate::runtime::AgentSession;

    use super::is_current_session;

    #[test]
    fn current_session_check_rejects_stale_connection_ids() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut sessions = HashMap::new();
        sessions.insert(
            "agent-a".to_string(),
            AgentSession {
                connection_id: "conn-new".to_string(),
                tx,
            },
        );

        assert!(is_current_session(&sessions, "agent-a", "conn-new"));
        assert!(!is_current_session(&sessions, "agent-a", "conn-old"));
        assert!(!is_current_session(&sessions, "agent-b", "conn-new"));
    }
}
