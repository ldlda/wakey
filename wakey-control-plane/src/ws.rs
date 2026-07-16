use anyhow::{Context, Result};
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{debug, info, info_span, warn};
use uuid::Uuid;
use wakey_agent::protocol::{
    AgentCapability, AgentCapabilityOptions, AgentTerminalSession, DEFAULT_TERMINAL_MAX_SESSIONS,
    ErrorPayload, RequestId, ServerMessage, TerminalCapabilityOptions, TerminalControl, TerminalId,
};
use wakey_core::Device;

use crate::runtime::{AgentReply, AgentSession, AppState, SessionEvent};
use crate::state::AuditEventInput;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum IncomingClientMessage {
    Hello {
        agent_id: String,
        #[serde(default)]
        capabilities: Vec<AgentCapability>,
        #[serde(default)]
        capability_options: AgentCapabilityOptions,
    },
    Auth {
        agent_id: String,
        agent_token: String,
    },
    Heartbeat {
        agent_id: String,
    },
    DeviceSnapshot {
        agent_id: String,
        devices: Vec<Device>,
    },
    Result {
        request_id: RequestId,
        result: serde_json::Value,
    },
    Error {
        request_id: RequestId,
        error: ErrorPayload,
    },
    TerminalRejected {
        terminal_id: TerminalId,
        error: ErrorPayload,
    },
    TerminalSessions {
        sessions: Vec<AgentTerminalSession>,
    },
}

#[derive(Default)]
struct AgentConnectionState {
    authed_agent_id: Option<String>,
    hello_agent_id: Option<String>,
    hello_at: Option<Instant>,
    capabilities: Vec<AgentCapability>,
    capability_options: AgentCapabilityOptions,
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

    let mut connection = AgentConnectionState::default();

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
                    &mut connection,
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

    if let Some(agent_id) = connection.authed_agent_id {
        info!(agent_id = %agent_id, "agent disconnected");
        let mut sessions = state.sessions.write().await;
        let should_remove = sessions
            .get(&agent_id)
            .map(|session| session.connection_id == connection_id)
            .unwrap_or(false);
        if should_remove {
            sessions.remove(&agent_id);
        }
        drop(sessions);
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
    connection: &mut AgentConnectionState,
    connected_at: Instant,
    text: &str,
) -> Result<()> {
    let message: IncomingClientMessage =
        serde_json::from_str(text).context("invalid client websocket payload")?;

    match message {
        IncomingClientMessage::Hello {
            agent_id,
            capabilities,
            capability_options,
        } => {
            if connection
                .hello_agent_id
                .as_deref()
                .is_some_and(|hello_agent_id| hello_agent_id != agent_id)
            {
                anyhow::bail!("hello changed agent identity");
            }
            let now = Instant::now();
            if connection.hello_at.is_none() {
                connection.hello_at = Some(now);
            }
            let connect_to_hello_ms = connected_at.elapsed().as_millis() as u64;
            info!(agent_id = %agent_id, connect_to_hello_ms, "agent hello received");
            connection.hello_agent_id = Some(agent_id);
            connection.capability_options = AgentCapabilityOptions {
                terminal: capabilities.contains(&AgentCapability::Terminal).then_some(
                    TerminalCapabilityOptions {
                        max_sessions: capability_options
                            .terminal
                            .map(|terminal| terminal.max_sessions)
                            .unwrap_or(DEFAULT_TERMINAL_MAX_SESSIONS)
                            .max(1),
                    },
                ),
            };
            connection.capabilities = capabilities;
        }
        IncomingClientMessage::Auth {
            agent_id,
            agent_token,
        } => {
            validate_auth_identity(connection, &agent_id)?;
            let connect_to_auth_ms = connected_at.elapsed().as_millis() as u64;
            let hello_to_auth_ms = connection
                .hello_at
                .map(|time| now_duration_ms(time.elapsed()));
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
                    capabilities: connection.capabilities.clone(),
                    capability_options: connection.capability_options.clone(),
                },
            );
            connection.authed_agent_id = Some(agent_id.clone());
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
            let _ = tx.send(SessionEvent::Message(ServerMessage::SyncDeviceSnapshot));
        }
        IncomingClientMessage::Heartbeat { agent_id } => {
            if connection.authed_agent_id.as_deref() != Some(agent_id.as_str()) {
                anyhow::bail!("heartbeat for unauthenticated or mismatched agent");
            }
            ensure_current_session(state, &agent_id, connection_id).await?;
            debug!(agent_id = %agent_id, "heartbeat received");
        }
        IncomingClientMessage::DeviceSnapshot { agent_id, devices } => {
            if connection.authed_agent_id.as_deref() != Some(agent_id.as_str()) {
                anyhow::bail!("device_snapshot for unauthenticated or mismatched agent");
            }
            ensure_current_session(state, &agent_id, connection_id).await?;
            match state
                .store
                .replace_agent_device_snapshot(&agent_id, &devices)
                .await
            {
                Ok(accepted) => {
                    debug!(agent_id = %agent_id, accepted, "agent websocket device snapshot accepted");
                }
                Err(err) => {
                    warn!(agent_id = %agent_id, error = %err, "failed to store websocket device snapshot");
                    anyhow::bail!("failed to store device snapshot: {err}");
                }
            }
        }
        IncomingClientMessage::Result { request_id, result } => {
            let agent_id = connection
                .authed_agent_id
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
            let agent_id = connection
                .authed_agent_id
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
        IncomingClientMessage::TerminalRejected { terminal_id, error } => {
            let agent_id = connection
                .authed_agent_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("terminal rejection before auth"))?;
            ensure_current_session(state, agent_id, connection_id).await?;
            let error_json = serde_json::to_string(&TerminalControl::Error {
                code: error.code,
                message: error.message,
            })?;
            if let Err(code) = state
                .terminals
                .reject(terminal_id.as_str(), agent_id, error_json)
                .await
            {
                debug!(terminal_id = %terminal_id, agent_id, code, "dropping rejection for inactive terminal");
                return Ok(());
            }
            warn!(terminal_id = %terminal_id, agent_id, "agent rejected terminal request");
        }
        IncomingClientMessage::TerminalSessions { sessions } => {
            let agent_id = connection
                .authed_agent_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("terminal inventory before auth"))?;
            ensure_current_session(state, agent_id, connection_id).await?;
            let credentials = state
                .terminals
                .reconcile_agent_sessions(agent_id, &sessions)
                .await;
            for (terminal_id, relay_token) in credentials {
                let _ = tx.send(SessionEvent::Message(ServerMessage::ResumeTerminal {
                    terminal_id,
                    relay_token,
                }));
            }
            info!(
                agent_id,
                sessions = sessions.len(),
                "agent terminal sessions reconciled"
            );
        }
    }

    Ok(())
}

fn now_duration_ms(duration: std::time::Duration) -> u64 {
    duration.as_millis() as u64
}

fn validate_auth_identity(connection: &AgentConnectionState, agent_id: &str) -> Result<()> {
    let hello_agent_id = connection
        .hello_agent_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("auth before hello"))?;
    if hello_agent_id != agent_id {
        anyhow::bail!("auth agent does not match hello agent");
    }
    Ok(())
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
    use wakey_agent::protocol::AgentCapabilityOptions;

    use crate::runtime::AgentSession;

    use super::{
        AgentConnectionState, IncomingClientMessage, is_current_session, validate_auth_identity,
    };

    #[test]
    fn current_session_check_rejects_stale_connection_ids() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut sessions = HashMap::new();
        sessions.insert(
            "agent-a".to_string(),
            AgentSession {
                connection_id: "conn-new".to_string(),
                tx,
                capabilities: Vec::new(),
                capability_options: AgentCapabilityOptions::default(),
            },
        );

        assert!(is_current_session(&sessions, "agent-a", "conn-new"));
        assert!(!is_current_session(&sessions, "agent-a", "conn-old"));
        assert!(!is_current_session(&sessions, "agent-b", "conn-new"));
    }

    #[test]
    fn auth_requires_a_matching_hello_identity() {
        let mut connection = AgentConnectionState::default();
        assert!(validate_auth_identity(&connection, "agent-a").is_err());

        connection.hello_agent_id = Some("agent-a".into());
        assert!(validate_auth_identity(&connection, "agent-a").is_ok());
        assert!(validate_auth_identity(&connection, "agent-b").is_err());
    }

    #[test]
    fn legacy_hello_without_capability_options_deserializes() {
        let message: IncomingClientMessage = serde_json::from_str(
            r#"{"type":"hello","agent_id":"agent-a","capabilities":["terminal"]}"#,
        )
        .expect("deserialize legacy hello");

        let IncomingClientMessage::Hello {
            capability_options, ..
        } = message
        else {
            panic!("expected hello");
        };
        assert_eq!(capability_options, AgentCapabilityOptions::default());
    }
}
