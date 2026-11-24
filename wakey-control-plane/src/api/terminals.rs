use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;
use wakey_core::protocol::{
    AgentCapability, DEFAULT_TERMINAL_MAX_SESSIONS, DEFAULT_TERMINAL_SESSION_TTL_SECONDS,
    ServerMessage, TerminalId,
};

use crate::api::ApiError;
use crate::runtime::terminals::{TerminalRegistryError, TerminalSummary};
use crate::runtime::{AppState, SessionEvent};
use crate::state::AuditEventInput;

mod transport;

pub use transport::{agent_terminal_ws, operator_terminal_ws, terminal_events_ws};

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
    pub expires_at_unix: Option<u64>,
    pub agent_attached: bool,
    pub operator_attached: bool,
    pub websocket_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment_token: Option<String>,
}

#[tracing::instrument(name = "terminal_create", skip_all)]
pub async fn create_terminal(
    State(state): State<AppState>,
    Json(request): Json<CreateTerminalRequest>,
) -> Result<(StatusCode, Json<TerminalSessionResponse>), ApiError> {
    validate_size(request.rows, request.cols)?;
    let (agent_tx, max_sessions, session_ttl_seconds) = {
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
        let max_sessions = session
            .capability_options
            .terminal
            .as_ref()
            .map(|terminal| terminal.max_sessions)
            .unwrap_or(DEFAULT_TERMINAL_MAX_SESSIONS)
            .max(1);
        let session_ttl_seconds = session
            .capability_options
            .terminal
            .as_ref()
            .map(|terminal| terminal.session_ttl_seconds)
            .unwrap_or(DEFAULT_TERMINAL_SESSION_TTL_SECONDS);
        (session.tx.clone(), max_sessions, session_ttl_seconds)
    };

    let created = state
        .terminals
        .create_with_limits(request.agent_id.clone(), max_sessions, session_ttl_seconds)
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

    spawn_session_timeout(
        state.clone(),
        terminal_id.clone(),
        request.agent_id.clone(),
        session_ttl_seconds,
    );
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
            expires_at_unix: created.expires_at_unix,
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
    let summary = state
        .terminals
        .summary(&terminal_id)
        .await
        .ok_or_else(|| terminal_not_found(&terminal_id))?;
    Ok(Json(TerminalSessionResponse {
        websocket_url: operator_ws_path(&terminal_id),
        terminal_id,
        agent_id: summary.agent_id,
        created_at_unix: summary.created_at_unix,
        expires_at_unix: summary.expires_at_unix,
        agent_attached: summary.agent_attached,
        operator_attached: summary.operator_attached,
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

#[tracing::instrument(name = "terminal_attach", skip_all)]
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
    let summary = state
        .terminals
        .summary(&terminal_id)
        .await
        .ok_or_else(|| terminal_not_found(&terminal_id))?;
    Ok(Json(TerminalSessionResponse {
        websocket_url: operator_ws_path(&terminal_id),
        terminal_id,
        agent_id: summary.agent_id,
        created_at_unix: summary.created_at_unix,
        expires_at_unix: summary.expires_at_unix,
        agent_attached: summary.agent_attached,
        operator_attached: summary.operator_attached,
        attachment_token: Some(attachment_token),
    }))
}

#[tracing::instrument(name = "terminal_close", skip_all)]
pub async fn close_terminal(
    State(state): State<AppState>,
    Path(terminal_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    if let Some(agent_id) =
        close_registered_terminal(&state, &terminal_id, TerminalCloseReason::HttpDelete).await
    {
        info!(terminal_id, agent_id, "terminal session closed by operator");
    } else {
        if !state.terminals.was_closed(&terminal_id).await {
            return Err(terminal_not_found(&terminal_id));
        }
        info!(terminal_id, "terminal session was already closed");
    }
    Ok(StatusCode::NO_CONTENT)
}

fn terminal_response(summary: TerminalSummary) -> TerminalSessionResponse {
    TerminalSessionResponse {
        websocket_url: operator_ws_path(&summary.terminal_id),
        terminal_id: summary.terminal_id,
        agent_id: summary.agent_id,
        created_at_unix: summary.created_at_unix,
        expires_at_unix: summary.expires_at_unix,
        agent_attached: summary.agent_attached,
        operator_attached: summary.operator_attached,
        attachment_token: None,
    }
}

#[derive(Clone, Copy)]
enum TerminalCloseReason {
    HttpDelete,
    BrowserClose,
    AbsoluteTimeout,
}

impl TerminalCloseReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::HttpDelete => "http_delete",
            Self::BrowserClose => "browser_close",
            Self::AbsoluteTimeout => "absolute_timeout",
        }
    }
}

/// Removes and audits a terminal as one logical operation. Only the caller
/// that actually removes the session records the close, keeping retries and
/// simultaneous timeout/operator closes from producing duplicate events.
async fn close_registered_terminal(
    state: &AppState,
    terminal_id: &str,
    reason: TerminalCloseReason,
) -> Option<String> {
    let agent_id = state.terminals.remove(terminal_id).await?;
    if let Some(session) = state.sessions.read().await.get(&agent_id) {
        let terminal_id = TerminalId::new(terminal_id.to_string()).ok()?;
        let _ = session
            .tx
            .send(SessionEvent::Message(ServerMessage::CloseTerminal {
                terminal_id,
            }));
    }
    append_terminal_audit(
        state,
        &agent_id,
        terminal_id,
        TerminalAudit {
            actor_type: "admin_api",
            event_type: "terminal_close",
            outcome: "ok",
            message: "terminal session closed",
            metadata: serde_json::json!({ "reason": reason.as_str() }),
        },
    )
    .await;
    Some(agent_id)
}

fn spawn_session_timeout(
    state: AppState,
    terminal_id: TerminalId,
    agent_id: String,
    session_ttl_seconds: u64,
) {
    if session_ttl_seconds == 0 {
        return;
    }
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(session_ttl_seconds)).await;
        if close_registered_terminal(
            &state,
            terminal_id.as_str(),
            TerminalCloseReason::AbsoluteTimeout,
        )
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

async fn append_terminal_audit(
    state: &AppState,
    agent_id: &str,
    terminal_id: &str,
    audit: TerminalAudit<'_>,
) {
    state
        .store
        .audit(AuditEventInput {
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
        .await;
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

fn registry_error(error: TerminalRegistryError) -> ApiError {
    use TerminalRegistryError::*;

    let status = match error {
        NotFound => StatusCode::NOT_FOUND,
        InvalidOperatorId => StatusCode::BAD_REQUEST,
        InvalidRelayToken | InvalidAttachmentToken | OperatorMismatch => StatusCode::UNAUTHORIZED,
        AgentLimitReached
        | InvalidTtl
        | OperatorAlreadyAttached
        | AgentMismatch
        | AgentAlreadyAttached
        | AgentDisconnected
        | Expired => StatusCode::CONFLICT,
    };
    let code = error.code();
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
    fn registry_errors_keep_their_http_status_and_wire_code() {
        use TerminalRegistryError::*;

        for (error, status, code) in [
            (NotFound, StatusCode::NOT_FOUND, "terminal_not_found"),
            (
                InvalidOperatorId,
                StatusCode::BAD_REQUEST,
                "terminal_operator_id_invalid",
            ),
            (
                InvalidAttachmentToken,
                StatusCode::UNAUTHORIZED,
                "terminal_attachment_token_invalid",
            ),
            (
                AgentLimitReached,
                StatusCode::CONFLICT,
                "agent_terminal_limit_reached",
            ),
        ] {
            let response = registry_error(error);
            assert_eq!(response.status, status);
            assert_eq!(response.code, code);
        }
    }
}
