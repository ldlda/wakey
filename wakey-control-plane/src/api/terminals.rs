use axum::Json;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use wakey_agent::protocol::{
    AgentCapability, ServerMessage, TerminalAgentHandshake, TerminalControl, TerminalId,
    TerminalOperatorHandshake,
};

use crate::api::ApiError;
use crate::runtime::terminals::{
    TERMINAL_ABSOLUTE_TIMEOUT, TERMINAL_ATTACH_TIMEOUT, TERMINAL_MAX_FRAME_BYTES,
    TerminalRelayFrame, TerminalSummary,
};
use crate::runtime::{AppState, SessionEvent};
use crate::state::AuditEventInput;

#[derive(Debug, Deserialize)]
pub struct CreateTerminalRequest {
    pub agent_id: String,
    #[serde(default = "default_rows")]
    pub rows: u16,
    #[serde(default = "default_cols")]
    pub cols: u16,
}

#[derive(Debug, Deserialize)]
pub struct AttachTerminalRequest {
    /// Stable for one browser tab, allowing CC to distinguish a stale socket
    /// owned by this tab from a session open in another browser.
    pub operator_id: String,
}

#[derive(Debug, Serialize)]
pub struct TerminalSessionResponse {
    pub terminal_id: String,
    pub agent_id: String,
    pub created_at_unix: u64,
    pub agent_attached: bool,
    pub operator_attached: bool,
    pub websocket_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment_token: Option<String>,
}

pub async fn create_terminal(
    State(state): State<AppState>,
    Json(request): Json<CreateTerminalRequest>,
) -> Result<(StatusCode, Json<TerminalSessionResponse>), ApiError> {
    validate_size(request.rows, request.cols)?;
    let agent_tx = {
        let sessions = state.sessions.read().await;
        let session = sessions.get(&request.agent_id).ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "agent_not_connected",
                "agent is not connected",
            )
        })?;
        if !session.capabilities.contains(&AgentCapability::Terminal) {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "terminal_not_supported",
                "agent has not advertised terminal capability",
            ));
        }
        session.tx.clone()
    };

    let created = state
        .terminals
        .create(request.agent_id.clone())
        .await
        .map_err(registry_error)?;
    let terminal_id = TerminalId::new(created.terminal_id.clone()).map_err(|message| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "terminal_id_invalid",
            message,
        )
    })?;
    if agent_tx
        .send(SessionEvent::Message(ServerMessage::OpenTerminal {
            terminal_id: terminal_id.clone(),
            relay_token: created.relay_token,
            rows: request.rows,
            cols: request.cols,
        }))
        .is_err()
    {
        state.terminals.remove(terminal_id.as_str()).await;
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "agent_send_failed",
            "failed to send terminal request to agent",
        ));
    }

    spawn_absolute_timeout(state.clone(), terminal_id.clone(), request.agent_id.clone());
    append_terminal_audit(
        &state,
        &request.agent_id,
        terminal_id.as_str(),
        TerminalAudit {
            actor_type: "admin_api",
            event_type: "terminal_request",
            outcome: "sent",
            message: "terminal session requested",
            metadata: serde_json::json!({ "rows": request.rows, "cols": request.cols }),
        },
    )
    .await;
    info!(terminal_id = %terminal_id, agent_id = %request.agent_id, "terminal session requested");
    Ok((
        StatusCode::CREATED,
        Json(TerminalSessionResponse {
            websocket_url: operator_ws_path(terminal_id.as_str()),
            terminal_id: terminal_id.to_string(),
            agent_id: request.agent_id,
            created_at_unix: created.created_at_unix,
            agent_attached: false,
            operator_attached: false,
            attachment_token: Some(created.attachment_token),
        }),
    ))
}

pub async fn get_terminal(
    State(state): State<AppState>,
    Path(terminal_id): Path<String>,
) -> Result<Json<TerminalSessionResponse>, ApiError> {
    let (agent_id, created_at_unix, agent_attached, operator_attached) = state
        .terminals
        .summary(&terminal_id)
        .await
        .ok_or_else(|| terminal_not_found(&terminal_id))?;
    Ok(Json(TerminalSessionResponse {
        websocket_url: operator_ws_path(&terminal_id),
        terminal_id,
        agent_id,
        created_at_unix,
        agent_attached,
        operator_attached,
        attachment_token: None,
    }))
}

pub async fn list_terminals(State(state): State<AppState>) -> Json<Vec<TerminalSessionResponse>> {
    Json(
        state
            .terminals
            .summaries()
            .await
            .into_iter()
            .map(terminal_response)
            .collect(),
    )
}

pub async fn attach_terminal(
    State(state): State<AppState>,
    Path(terminal_id): Path<String>,
    Json(request): Json<AttachTerminalRequest>,
) -> Result<Json<TerminalSessionResponse>, ApiError> {
    let attachment_token = state
        .terminals
        .issue_attachment_token_for_operator(&terminal_id, &request.operator_id)
        .await
        .map_err(registry_error)?;
    let (agent_id, created_at_unix, agent_attached, operator_attached) = state
        .terminals
        .summary(&terminal_id)
        .await
        .ok_or_else(|| terminal_not_found(&terminal_id))?;
    Ok(Json(TerminalSessionResponse {
        websocket_url: operator_ws_path(&terminal_id),
        terminal_id,
        agent_id,
        created_at_unix,
        agent_attached,
        operator_attached,
        attachment_token: Some(attachment_token),
    }))
}

pub async fn close_terminal(
    State(state): State<AppState>,
    Path(terminal_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    if let Some(agent_id) = close_registered_terminal(&state, &terminal_id).await {
        info!(terminal_id, agent_id, "terminal session closed by operator");
        append_terminal_audit(
            &state,
            &agent_id,
            &terminal_id,
            TerminalAudit {
                actor_type: "admin_api",
                event_type: "terminal_close",
                outcome: "ok",
                message: "terminal session closed by operator",
                metadata: serde_json::json!({}),
            },
        )
        .await;
    } else {
        if !state.terminals.was_closed(&terminal_id).await {
            return Err(terminal_not_found(&terminal_id));
        }
        info!(terminal_id, "terminal session was already closed");
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn agent_terminal_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(terminal_id): Path<String>,
) -> Response {
    ws.max_message_size(TERMINAL_MAX_FRAME_BYTES)
        .on_upgrade(move |socket| handle_agent_terminal_socket(state, terminal_id, socket))
}

pub async fn operator_terminal_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(terminal_id): Path<String>,
) -> Response {
    ws.max_message_size(TERMINAL_MAX_FRAME_BYTES)
        .on_upgrade(move |socket| handle_operator_terminal_socket(state, terminal_id, socket))
}

async fn handle_agent_terminal_socket(state: AppState, terminal_id: String, mut socket: WebSocket) {
    let auth = match receive_text_handshake(&mut socket).await.and_then(|text| {
        serde_json::from_str::<TerminalAgentHandshake>(&text).map_err(|_| "invalid handshake")
    }) {
        Ok(TerminalAgentHandshake::Auth {
            agent_id,
            relay_token,
        }) => (agent_id, relay_token),
        Err(code) => {
            close_socket(&mut socket, code).await;
            return;
        }
    };
    let (mut outbound, pending) = match state
        .terminals
        .attach_agent(&terminal_id, &auth.0, &auth.1)
        .await
    {
        Ok(outbound) => outbound,
        Err(code) => {
            close_socket(&mut socket, code).await;
            return;
        }
    };
    info!(terminal_id, agent_id = %auth.0, "agent terminal socket attached");
    append_terminal_audit(
        &state,
        &auth.0,
        &terminal_id,
        TerminalAudit {
            actor_type: "agent",
            event_type: "terminal_agent_attach",
            outcome: "ok",
            message: "agent terminal transport attached",
            metadata: serde_json::json!({}),
        },
    )
    .await;

    let (mut write, mut read) = socket.split();
    for frame in pending {
        let closes = matches!(frame, TerminalRelayFrame::Close);
        if send_relay_frame(&mut write, frame).await.is_err() || closes {
            state.terminals.remove(&terminal_id).await;
            return;
        }
    }
    loop {
        tokio::select! {
            biased;
            outbound_frame = outbound.recv() => {
                let Some(frame) = outbound_frame else { break; };
                if send_relay_frame(&mut write, frame).await.is_err() { break; }
            }
            incoming = read.next() => {
                let Some(Ok(message)) = incoming else { break; };
                match agent_relay_frame(message) {
                    Ok(Some(frame)) => {
                        let closes = matches!(frame, TerminalRelayFrame::Close);
                        audit_agent_control_frame(&state, &auth.0, &terminal_id, &frame).await;
                        if state.terminals.relay_from_agent(&terminal_id, frame).await.is_err() { break; }
                        if closes { break; }
                    }
                    Ok(None) => {}
                    Err(code) => {
                        warn!(terminal_id, code, "invalid agent terminal frame");
                        break;
                    }
                }
            }
        }
    }
    if let Some(agent_id) = state.terminals.detach_agent(&terminal_id).await
        && let Some(session) = state.sessions.read().await.get(&agent_id)
    {
        let _ = session
            .tx
            .send(SessionEvent::Message(ServerMessage::SyncTerminalSessions));
    }
    info!(terminal_id, "agent terminal relay detached");
}

async fn handle_operator_terminal_socket(
    state: AppState,
    terminal_id: String,
    mut socket: WebSocket,
) {
    let auth = match receive_text_handshake(&mut socket).await.and_then(|text| {
        serde_json::from_str::<TerminalOperatorHandshake>(&text).map_err(|_| "invalid handshake")
    }) {
        Ok(TerminalOperatorHandshake::Attach {
            attachment_token,
            operator_id,
        }) => (attachment_token, operator_id),
        Err(code) => {
            close_socket(&mut socket, code).await;
            return;
        }
    };
    let mut outbound = match state
        .terminals
        .attach_operator(&terminal_id, &auth.0, &auth.1)
        .await
    {
        Ok(attached) => attached,
        Err(code) => {
            close_socket(&mut socket, code).await;
            return;
        }
    };
    info!(terminal_id, "operator terminal socket attached");
    let snapshot = serde_json::to_string(&TerminalControl::Snapshot)
        .expect("terminal snapshot control serializes");
    if let Err(code) = state
        .terminals
        .relay_to_agent(&terminal_id, TerminalRelayFrame::Text(snapshot))
        .await
    {
        warn!(terminal_id, code, "failed to request terminal snapshot");
    }
    let summary = state.terminals.summary(&terminal_id).await;
    if let Some((agent_id, _, _, _)) = &summary {
        append_terminal_audit(
            &state,
            agent_id,
            &terminal_id,
            TerminalAudit {
                actor_type: "admin_api",
                event_type: "terminal_operator_attach",
                outcome: "ok",
                message: "operator terminal transport attached",
                metadata: serde_json::json!({}),
            },
        )
        .await;
    }

    let (mut write, mut read) = socket.split();
    if summary.is_some_and(|(_, _, agent_attached, _)| agent_attached) {
        let ready = serde_json::to_string(&TerminalControl::Ready)
            .expect("terminal ready control serializes");
        if send_relay_frame(&mut write, TerminalRelayFrame::Text(ready))
            .await
            .is_err()
        {
            state.terminals.detach_operator(&terminal_id, &auth.0).await;
            return;
        }
    }
    let mut explicit_close = false;
    loop {
        tokio::select! {
            biased;
            incoming = read.next() => {
                let Some(Ok(message)) = incoming else { break; };
                match operator_relay_frame(message) {
                    Ok(Some(frame)) => {
                        explicit_close = matches!(frame, TerminalRelayFrame::Close);
                        if state.terminals.relay_to_agent(&terminal_id, frame).await.is_err() { break; }
                        if explicit_close { break; }
                    }
                    Ok(None) => {}
                    Err(code) => {
                        warn!(terminal_id, code, "invalid operator terminal frame");
                        break;
                    }
                }
            }
            outbound_frame = outbound.recv() => {
                let Some(frame) = outbound_frame else { break; };
                if send_relay_frame(&mut write, frame).await.is_err() { break; }
            }
        }
    }

    if explicit_close {
        close_registered_terminal(&state, &terminal_id).await;
    } else {
        state.terminals.detach_operator(&terminal_id, &auth.0).await;
    }
    info!(
        terminal_id,
        explicit_close, "operator terminal socket detached"
    );
}

fn terminal_response(summary: TerminalSummary) -> TerminalSessionResponse {
    TerminalSessionResponse {
        websocket_url: operator_ws_path(&summary.terminal_id),
        terminal_id: summary.terminal_id,
        agent_id: summary.agent_id,
        created_at_unix: summary.created_at_unix,
        agent_attached: summary.agent_attached,
        operator_attached: summary.operator_attached,
        attachment_token: None,
    }
}

fn agent_relay_frame(message: Message) -> Result<Option<TerminalRelayFrame>, &'static str> {
    match message {
        Message::Binary(bytes) => Ok(Some(TerminalRelayFrame::Binary(bytes.to_vec()))),
        Message::Text(text) => {
            let control: TerminalControl =
                serde_json::from_str(&text).map_err(|_| "terminal_control_invalid")?;
            if !matches!(
                control,
                TerminalControl::Ready
                    | TerminalControl::Exited { .. }
                    | TerminalControl::Error { .. }
                    | TerminalControl::Close
            ) {
                return Err("terminal_control_direction_invalid");
            }
            Ok(Some(TerminalRelayFrame::Text(
                serde_json::to_string(&control).map_err(|_| "terminal_control_invalid")?,
            )))
        }
        Message::Ping(_) | Message::Pong(_) => Ok(None),
        Message::Close(_) => Ok(Some(TerminalRelayFrame::Close)),
    }
}

fn operator_relay_frame(message: Message) -> Result<Option<TerminalRelayFrame>, &'static str> {
    match message {
        Message::Binary(bytes) => Ok(Some(TerminalRelayFrame::Binary(bytes.to_vec()))),
        Message::Text(text) => {
            let control: TerminalControl =
                serde_json::from_str(&text).map_err(|_| "terminal_control_invalid")?;
            if !matches!(
                control,
                TerminalControl::Resize { .. } | TerminalControl::Close
            ) {
                return Err("terminal_control_direction_invalid");
            }
            if let TerminalControl::Resize { rows, cols } = control {
                validate_size(rows, cols).map_err(|_| "terminal_size_invalid")?;
            }
            Ok(Some(TerminalRelayFrame::Text(
                serde_json::to_string(&control).map_err(|_| "terminal_control_invalid")?,
            )))
        }
        Message::Ping(_) | Message::Pong(_) => Ok(None),
        Message::Close(_) => Ok(None),
    }
}

async fn receive_text_handshake(socket: &mut WebSocket) -> Result<String, &'static str> {
    match tokio::time::timeout(TERMINAL_ATTACH_TIMEOUT, socket.recv()).await {
        Ok(Some(Ok(Message::Text(text)))) => Ok(text.to_string()),
        Ok(_) => Err("terminal_handshake_required"),
        Err(_) => Err("terminal_handshake_timeout"),
    }
}

async fn close_socket(socket: &mut WebSocket, code: &str) {
    let control = TerminalControl::Error {
        code: code.into(),
        message: code.replace('_', " "),
    };
    if let Ok(json) = serde_json::to_string(&control) {
        let _ = socket.send(Message::Text(json.into())).await;
    }
    let _ = socket.send(Message::Close(None)).await;
}

async fn send_relay_frame<S>(write: &mut S, frame: TerminalRelayFrame) -> Result<(), axum::Error>
where
    S: futures_util::Sink<Message, Error = axum::Error> + Unpin,
{
    let message = match frame {
        TerminalRelayFrame::Binary(bytes) => Message::Binary(bytes.into()),
        TerminalRelayFrame::Text(text) => Message::Text(text.into()),
        TerminalRelayFrame::Close => Message::Close(None),
    };
    write.send(message).await
}

async fn close_registered_terminal(state: &AppState, terminal_id: &str) -> Option<String> {
    let agent_id = state.terminals.remove(terminal_id).await?;
    if let Some(session) = state.sessions.read().await.get(&agent_id) {
        let terminal_id = TerminalId::new(terminal_id.to_string()).ok()?;
        let _ = session
            .tx
            .send(SessionEvent::Message(ServerMessage::CloseTerminal {
                terminal_id,
            }));
    }
    Some(agent_id)
}

fn spawn_absolute_timeout(state: AppState, terminal_id: TerminalId, agent_id: String) {
    tokio::spawn(async move {
        tokio::time::sleep(TERMINAL_ABSOLUTE_TIMEOUT).await;
        if close_registered_terminal(&state, terminal_id.as_str())
            .await
            .is_some()
        {
            info!(terminal_id = %terminal_id, agent_id, "terminal absolute timeout reached");
        }
    });
}

struct TerminalAudit<'a> {
    actor_type: &'a str,
    event_type: &'a str,
    outcome: &'a str,
    message: &'a str,
    metadata: serde_json::Value,
}

async fn audit_agent_control_frame(
    state: &AppState,
    agent_id: &str,
    terminal_id: &str,
    frame: &TerminalRelayFrame,
) {
    let TerminalRelayFrame::Text(text) = frame else {
        return;
    };
    let Ok(control) = serde_json::from_str::<TerminalControl>(text) else {
        return;
    };
    let audit = match control {
        TerminalControl::Ready => TerminalAudit {
            actor_type: "agent",
            event_type: "terminal_ready",
            outcome: "ok",
            message: "terminal PTY ready",
            metadata: serde_json::json!({}),
        },
        TerminalControl::Exited { exit_code } => TerminalAudit {
            actor_type: "agent",
            event_type: "terminal_exit",
            outcome: "exited",
            message: "terminal process exited",
            metadata: serde_json::json!({ "exit_code": exit_code }),
        },
        TerminalControl::Error { code, .. } => TerminalAudit {
            actor_type: "agent",
            event_type: "terminal_error",
            outcome: "error",
            message: "terminal worker reported an error",
            metadata: serde_json::json!({ "code": code }),
        },
        _ => return,
    };
    append_terminal_audit(state, agent_id, terminal_id, audit).await;
}

async fn append_terminal_audit(
    state: &AppState,
    agent_id: &str,
    terminal_id: &str,
    audit: TerminalAudit<'_>,
) {
    if let Err(err) = state
        .store
        .append_audit_event(AuditEventInput {
            actor_type: audit.actor_type.into(),
            actor_id: None,
            agent_id: Some(agent_id.to_string()),
            request_id: Some(terminal_id.to_string()),
            event_type: audit.event_type.into(),
            outcome: audit.outcome.into(),
            latency_ms: None,
            message: audit.message.into(),
            metadata: audit.metadata,
        })
        .await
    {
        warn!(terminal_id, event_type = audit.event_type, error = %err, "failed to append terminal audit event");
    }
}

fn validate_size(rows: u16, cols: u16) -> Result<(), ApiError> {
    if !(1..=300).contains(&rows) || !(1..=500).contains(&cols) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "terminal_size_invalid",
            "terminal rows must be 1..=300 and columns must be 1..=500",
        ));
    }
    Ok(())
}

fn registry_error(code: &'static str) -> ApiError {
    let status = match code {
        "terminal_not_found" => StatusCode::NOT_FOUND,
        "terminal_operator_id_invalid" => StatusCode::BAD_REQUEST,
        "terminal_relay_token_invalid"
        | "terminal_attachment_token_invalid"
        | "terminal_operator_mismatch" => StatusCode::UNAUTHORIZED,
        _ => StatusCode::CONFLICT,
    };
    ApiError::new(status, code, code.replace('_', " "))
}

fn terminal_not_found(terminal_id: &str) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "terminal_not_found",
        format!("terminal session {terminal_id} was not found"),
    )
}

fn operator_ws_path(terminal_id: &str) -> String {
    format!("/api/v1/control/terminals/{terminal_id}/ws")
}

const fn default_rows() -> u16 {
    24
}

const fn default_cols() -> u16 {
    80
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_control_direction_is_enforced() {
        let resize = serde_json::to_string(&TerminalControl::Resize { rows: 24, cols: 80 })
            .expect("resize json");
        assert!(operator_relay_frame(Message::Text(resize.into())).is_ok());

        let ready = serde_json::to_string(&TerminalControl::Ready).expect("ready json");
        assert!(operator_relay_frame(Message::Text(ready.into())).is_err());

        let snapshot = serde_json::to_string(&TerminalControl::Snapshot).expect("snapshot json");
        assert!(operator_relay_frame(Message::Text(snapshot.into())).is_err());
    }
}
