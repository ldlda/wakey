use anyhow::{Context, Result};
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{Instrument, debug, info, info_span, warn};
use uuid::Uuid;
use wakey_core::auth::BearerToken;
use wakey_core::protocol::{
    AGENT_ID_HEADER, AgentCapability, AgentCapabilityOptions, ClientFrame,
    DEFAULT_TERMINAL_MAX_SESSIONS, DEFAULT_TERMINAL_SESSION_TTL_SECONDS, ServerMessage,
    TerminalCapabilityOptions, TerminalControl, checked_terminal_session_ttl,
};

use crate::runtime::{AgentReply, AgentSession, AppState, SessionEvent};
use crate::state::AuditEventInput;

// The control plane can relay result kinds it does not know about.
type IncomingClientMessage = ClientFrame<serde_json::Value>;

#[derive(Default)]
struct AgentConnectionState {
    upgrade_credentials: Option<UpgradeCredentials>,
    authed_agent_id: Option<String>,
    hello_agent_id: Option<String>,
    hello_at: Option<Instant>,
    capabilities: Vec<AgentCapability>,
    capability_options: AgentCapabilityOptions,
}

struct UpgradeCredentials {
    agent_id: String,
    token: String,
}

fn upgrade_credentials(headers: &HeaderMap) -> Result<Option<UpgradeCredentials>, StatusCode> {
    let mut auth = headers.get_all(header::AUTHORIZATION).iter();
    let mut ids = headers.get_all(AGENT_ID_HEADER).iter();
    let invalid = StatusCode::UNAUTHORIZED;
    let (authorization, id) = match (auth.next(), ids.next()) {
        (None, None) => return Ok(None),
        (Some(authorization), Some(id)) if auth.next().is_none() && ids.next().is_none() => {
            (authorization, id)
        }
        _ => return Err(invalid),
    };
    let token = BearerToken::parse_authorization(authorization.to_str().map_err(|_| invalid)?)
        .ok_or(invalid)?;
    let agent_id = id.to_str().map_err(|_| invalid)?;
    if agent_id.is_empty() || agent_id.bytes().any(|b| !b.is_ascii_graphic()) {
        return Err(invalid);
    }
    Ok(Some(UpgradeCredentials {
        agent_id: agent_id.into(),
        token: token.as_str().into(),
    }))
}

pub async fn agent_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let upgrade_credentials = match upgrade_credentials(&headers) {
        Ok(credentials) => credentials,
        Err(status) => return status.into_response(),
    };
    if let Some(credentials) = &upgrade_credentials {
        let started = Instant::now();
        if !state
            .store
            .verify_agent_token(&credentials.agent_id, &credentials.token)
            .instrument(info_span!("agent_upgrade_auth_verify", agent_id = %credentials.agent_id))
            .await
        {
            audit_auth(
                &state,
                &credentials.agent_id,
                false,
                now_duration_ms(started.elapsed()),
                None,
            )
            .await;
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }
    ws.on_upgrade(move |socket| handle_agent_socket(state, socket, upgrade_credentials))
}

async fn handle_agent_socket(
    state: AppState,
    socket: WebSocket,
    upgrade_credentials: Option<UpgradeCredentials>,
) {
    let connection_id = Uuid::new_v4().to_string();
    // Lifecycle events are available immediately; do not hold an entered span
    // across socket awaits or wait for disconnect to export the connection.
    info!(%connection_id, "agent websocket upgraded");
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

    let mut connection = AgentConnectionState {
        upgrade_credentials,
        ..AgentConnectionState::default()
    };
    let hello_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);

    loop {
        let frame = if connection.upgrade_credentials.is_some() {
            match tokio::time::timeout_at(hello_deadline, read.next()).await {
                Ok(frame) => frame,
                Err(_) => {
                    debug!(%connection_id, "authenticated agent did not send hello");
                    break;
                }
            }
        } else {
            read.next().await
        };
        let msg = match frame {
            Some(Ok(msg)) => msg,
            Some(Err(err)) => {
                warn!(%connection_id, error = %err, "agent websocket receive error");
                break;
            }
            None => {
                info!(%connection_id, "agent websocket stream ended by peer");
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
                    warn!(%connection_id, error = %err, "closing agent websocket due to protocol/auth error");
                    break;
                }
            }
            Message::Ping(_) => {}
            Message::Pong(_) => {}
            Message::Close(frame) => {
                info!(%connection_id, close = ?frame, "agent websocket close frame received");
                break;
            }
            Message::Binary(_) => {
                debug!("ignoring unexpected binary websocket frame");
            }
        }
    }

    if let Some(agent_id) = connection.authed_agent_id {
        info!(%connection_id, agent_id = %agent_id, connected_ms = now_duration_ms(connected_at.elapsed()), "agent disconnected");
        let mut sessions = state.sessions.write().await;
        let should_remove = sessions
            .get(&agent_id)
            .map(|session| session.connection_id == connection_id)
            .unwrap_or(false);
        if should_remove {
            sessions.remove(&agent_id);
        }
        drop(sessions);
        state
            .store
            .audit(AuditEventInput {
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
            .await;
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
            if let Some(credentials) = &connection.upgrade_credentials
                && credentials.agent_id != agent_id
            {
                anyhow::bail!("hello agent does not match authenticated upgrade");
            }
            connection.hello_agent_id = Some(agent_id);
            let terminal = if capabilities.contains(&AgentCapability::Terminal) {
                let max_sessions = capability_options
                    .terminal
                    .as_ref()
                    .map(|terminal| terminal.max_sessions)
                    .unwrap_or(DEFAULT_TERMINAL_MAX_SESSIONS)
                    .max(1);
                let session_ttl_seconds = capability_options
                    .terminal
                    .as_ref()
                    .map(|terminal| terminal.session_ttl_seconds)
                    .unwrap_or(DEFAULT_TERMINAL_SESSION_TTL_SECONDS);
                checked_terminal_session_ttl(session_ttl_seconds).map_err(anyhow::Error::msg)?;
                Some(TerminalCapabilityOptions {
                    max_sessions,
                    session_ttl_seconds,
                })
            } else {
                None
            };
            connection.capability_options = AgentCapabilityOptions { terminal };
            connection.capabilities = capabilities
                .into_iter()
                .filter(|capability| *capability != AgentCapability::Unknown)
                .collect();
            if let Some(credentials) = connection.upgrade_credentials.take() {
                register_agent(
                    state,
                    tx,
                    connection_id,
                    connection,
                    connected_at,
                    credentials,
                )
                .await?;
            }
        }
        IncomingClientMessage::Auth {
            agent_id,
            agent_token,
        } => {
            if connection.upgrade_credentials.is_some() || connection.authed_agent_id.is_some() {
                anyhow::bail!("unexpected agent auth frame");
            }
            validate_auth_identity(connection, &agent_id)?;
            register_agent(
                state,
                tx,
                connection_id,
                connection,
                connected_at,
                UpgradeCredentials {
                    agent_id,
                    token: agent_token,
                },
            )
            .await?;
        }
        IncomingClientMessage::Heartbeat {
            agent_id,
            process_status,
        } => {
            if connection.authed_agent_id.as_deref() != Some(agent_id.as_str()) {
                anyhow::bail!("heartbeat for unauthenticated or mismatched agent");
            }
            {
                // Keep the connection check and status write atomic with reconnects.
                let sessions = state.sessions.read().await;
                validate_current_session(&sessions, &agent_id, connection_id)?;
                if let Some(sample) = process_status {
                    state
                        .process_status
                        .record_agent(&agent_id, connection_id, sample)
                        .await;
                }
            }
            debug!(agent_id = %agent_id, "heartbeat received");
        }
        IncomingClientMessage::DeviceSnapshot {
            agent_id,
            devices,
            trace_context,
        } => {
            if connection.authed_agent_id.as_deref() != Some(agent_id.as_str()) {
                anyhow::bail!("device_snapshot for unauthenticated or mismatched agent");
            }
            ensure_current_session(state, &agent_id, connection_id).await?;
            let mut operation = wakey_core::telemetry::Operation::new("snapshot_store");
            let span = tracing::info_span!("store_device_snapshot", agent_id = %agent_id, devices = devices.len());
            // Continue collection's trace rather than parenting persistence to
            // the long-lived agent connection. Empty/invalid context is ignored.
            wakey_core::telemetry::set_remote_parent(&span, &trace_context);
            match tracing::Instrument::instrument(
                state
                    .store
                    .replace_agent_device_snapshot(&agent_id, &devices),
                span,
            )
            .await
            {
                Ok(accepted) => {
                    operation.outcome("ok");
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
            if let Err(error) = state
                .terminals
                .reject(terminal_id.as_str(), agent_id, error_json)
                .await
            {
                debug!(terminal_id = %terminal_id, agent_id, code = error.code(), "dropping rejection for inactive terminal");
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

async fn register_agent(
    state: &AppState,
    tx: &mpsc::UnboundedSender<SessionEvent>,
    connection_id: &str,
    connection: &mut AgentConnectionState,
    connected_at: Instant,
    credentials: UpgradeCredentials,
) -> Result<()> {
    let UpgradeCredentials { agent_id, token } = credentials;
    let connect_to_auth_ms = now_duration_ms(connected_at.elapsed());
    let hello_to_auth_ms = connection
        .hello_at
        .map(|time| now_duration_ms(time.elapsed()));

    // A token can be revoked while the upgraded socket is waiting for Hello.
    // Check again under the same session lock that revoke uses to remove it.
    let mut sessions = state.sessions.write().await;
    let accepted = state
        .store
        .verify_agent_token(&agent_id, &token)
        .instrument(info_span!("agent_auth_verify", agent_id = %agent_id, %connection_id))
        .await;
    if accepted {
        sessions.insert(
            agent_id.clone(),
            AgentSession {
                connection_id: connection_id.into(),
                tx: tx.clone(),
                capabilities: connection.capabilities.clone(),
                capability_options: connection.capability_options.clone(),
            },
        );
    }
    drop(sessions);

    if !accepted {
        warn!(%agent_id, "agent auth rejected");
        audit_auth(
            state,
            &agent_id,
            false,
            connect_to_auth_ms,
            hello_to_auth_ms,
        )
        .await;
        anyhow::bail!("agent auth rejected");
    }
    connection.authed_agent_id = Some(agent_id.clone());
    info!(%agent_id, connect_to_auth_ms, hello_to_auth_ms = hello_to_auth_ms.unwrap_or(0), "agent authenticated");
    audit_auth(state, &agent_id, true, connect_to_auth_ms, hello_to_auth_ms).await;
    let _ = tx.send(SessionEvent::Message(ServerMessage::SyncDeviceSnapshot));
    Ok(())
}

async fn audit_auth(
    state: &AppState,
    agent_id: &str,
    accepted: bool,
    connect_to_auth_ms: u64,
    hello_to_auth_ms: Option<u64>,
) {
    let (outcome, message) = if accepted {
        ("ok", "agent websocket authenticated")
    } else {
        ("rejected", "agent auth rejected")
    };
    state
        .store
        .audit(AuditEventInput {
            actor_type: "agent".into(),
            actor_id: Some(agent_id.into()),
            agent_id: Some(agent_id.into()),
            request_id: None,
            event_type: "agent_ws_auth".into(),
            outcome: outcome.into(),
            latency_ms: Some(connect_to_auth_ms),
            message: message.into(),
            metadata: serde_json::json!({
                "connect_to_auth_ms": connect_to_auth_ms,
                "hello_to_auth_ms": hello_to_auth_ms,
            }),
        })
        .await;
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
    validate_current_session(&sessions, agent_id, connection_id)
}

fn validate_current_session(
    sessions: &std::collections::HashMap<String, AgentSession>,
    agent_id: &str,
    connection_id: &str,
) -> Result<()> {
    if is_current_session(sessions, agent_id, connection_id) {
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

    use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
    use tokio::sync::mpsc;
    use wakey_core::protocol::AgentCapabilityOptions;

    use crate::runtime::AgentSession;

    use super::{
        AGENT_ID_HEADER, AgentConnectionState, IncomingClientMessage, is_current_session,
        upgrade_credentials, validate_auth_identity,
    };

    #[test]
    fn upgrade_credentials_allow_legacy_only_when_both_headers_are_absent() {
        let mut headers = HeaderMap::new();
        assert!(upgrade_credentials(&headers).unwrap().is_none());
        headers.insert(AGENT_ID_HEADER, HeaderValue::from_static("agent-a"));
        assert_eq!(
            upgrade_credentials(&headers).err(),
            Some(StatusCode::UNAUTHORIZED)
        );
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Basic test-token"),
        );
        assert_eq!(
            upgrade_credentials(&headers).err(),
            Some(StatusCode::UNAUTHORIZED)
        );
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("bearer test-token"),
        );
        let credentials = upgrade_credentials(&headers).unwrap().unwrap();
        assert_eq!(credentials.agent_id, "agent-a");
        assert_eq!(credentials.token, "test-token");
        headers.append(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer another-token"),
        );
        assert_eq!(
            upgrade_credentials(&headers).err(),
            Some(StatusCode::UNAUTHORIZED)
        );
    }

    #[test]
    fn heartbeat_status_is_optional_and_preserves_zero_values() {
        for (json, expected) in [
            (r#"{"type":"heartbeat","agent_id":"agent-a"}"#, None),
            (
                r#"{"type":"heartbeat","agent_id":"agent-a","process_status":{"cpu_time_seconds":0.0}}"#,
                Some(0.0),
            ),
        ] {
            let IncomingClientMessage::Heartbeat { process_status, .. } =
                serde_json::from_str(json).unwrap()
            else {
                panic!("expected heartbeat");
            };
            assert_eq!(
                process_status.and_then(|sample| sample.cpu_time_seconds),
                expected
            );
        }
    }

    #[test]
    fn device_snapshot_trace_context_is_optional_and_cross_crate_compatible() {
        let legacy: IncomingClientMessage =
            serde_json::from_str(r#"{"type":"device_snapshot","agent_id":"agent-a","devices":[]}"#)
                .unwrap();
        let IncomingClientMessage::DeviceSnapshot { trace_context, .. } = legacy else {
            panic!("expected snapshot");
        };
        assert!(trace_context.is_empty());

        let expected = [(
            "traceparent".to_string(),
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_string(),
        )]
        .into_iter()
        .collect();
        let message = wakey_core::protocol::ClientMessage::DeviceSnapshot {
            agent_id: "agent-a".into(),
            devices: vec![],
            trace_context: expected,
        };
        let IncomingClientMessage::DeviceSnapshot {
            agent_id,
            trace_context,
            ..
        } = serde_json::from_str(&serde_json::to_string(&message).unwrap()).unwrap()
        else {
            panic!("expected snapshot");
        };
        assert_eq!(agent_id, "agent-a");
        assert_eq!(
            trace_context["traceparent"],
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
        );
    }

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
