use axum::Json;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tracing::{info, info_span, warn};
use uuid::Uuid;
use wakey_agent::protocol::{AgentCommand, ErrorPayload, RequestId, ServerMessage};

use crate::api::ApiError;
use crate::runtime::{AgentReply, AppState, SessionEvent};
use crate::state::AuditEventInput;

#[derive(Debug, Serialize)]
pub struct AgentStatus {
    pub agent_id: String,
    pub connected: bool,
    pub nickname: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RelayCommandRequest {
    pub command: AgentCommand,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct RelayCommandResponse {
    pub request_id: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorPayload>,
}

pub async fn list_agents(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let enrolled = state.store.list_agents_with_nicknames().await;
    let sessions = state.sessions.read().await;

    let agents = enrolled
        .into_iter()
        .map(|(agent_id, nickname)| AgentStatus {
            connected: sessions.contains_key(&agent_id),
            agent_id,
            nickname,
        })
        .collect::<Vec<_>>();

    Ok((StatusCode::OK, Json(agents)))
}

pub async fn run_command(
    State(state): State<AppState>,
    AxumPath(agent_id): AxumPath<String>,
    Json(req): Json<RelayCommandRequest>,
) -> Result<impl IntoResponse, ApiError> {
    match relay_agent_command(&state, &agent_id, req.command, req.timeout_ms).await {
        Ok(response) => Ok((StatusCode::OK, Json(response))),
        Err(err) => Err(err),
    }
}

pub async fn relay_agent_command(
    state: &AppState,
    agent_id: &str,
    command: AgentCommand,
    timeout_ms: Option<u64>,
) -> Result<RelayCommandResponse, ApiError> {
    let request_id_string = format!("req-{}", Uuid::new_v4());
    let command_kind = command_kind(&command);
    let started = Instant::now();
    let span = info_span!(
        "relay_command",
        agent_id = %agent_id,
        request_id = %request_id_string,
        command = %command_kind,
    );
    let _span_guard = span.enter();

    let request_id = RequestId::try_from(request_id_string.clone()).map_err(|err| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid_request_id",
            &err,
        )
    })?;

    let tx = {
        let sessions = state.sessions.read().await;
        sessions.get(agent_id).map(|session| session.tx.clone())
    }
    .ok_or_else(|| {
        warn!("command rejected: agent not connected");
        ApiError::new(
            StatusCode::NOT_FOUND,
            "agent_not_connected",
            "agent is not connected",
        )
    })?;

    let (pending_tx, pending_rx) = tokio::sync::oneshot::channel();
    state
        .pending
        .lock()
        .await
        .insert(request_id_string.clone(), pending_tx);

    info!("dispatching command to agent");
    if let Err(err) = state
        .store
        .append_audit_event(AuditEventInput {
            actor_type: "admin_api".into(),
            actor_id: None,
            agent_id: Some(agent_id.to_string()),
            request_id: Some(request_id_string.clone()),
            event_type: "command_dispatch".into(),
            outcome: "sent".into(),
            latency_ms: None,
            message: "dispatched command to connected agent".into(),
            metadata: serde_json::json!({ "command": command_kind }),
        })
        .await
    {
        warn!(error = %err, "failed to append audit event for command dispatch");
    }

    if let Err(err) = tx.send(SessionEvent::Message(ServerMessage::Command {
        request_id,
        command,
    })) {
        state.pending.lock().await.remove(&request_id_string);
        warn!(error = %err, "failed sending command to agent session");
        if let Err(audit_err) = state
            .store
            .append_audit_event(AuditEventInput {
                actor_type: "admin_api".into(),
                actor_id: None,
                agent_id: Some(agent_id.to_string()),
                request_id: Some(request_id_string.clone()),
                event_type: "command_dispatch".into(),
                outcome: "send_failed".into(),
                latency_ms: Some(started.elapsed().as_millis() as u64),
                message: err.to_string(),
                metadata: serde_json::json!({ "command": command_kind }),
            })
            .await
        {
            warn!(error = %audit_err, "failed to append audit event for command send failure");
        }
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "agent_send_failed",
            format!("failed to send command to agent: {err}"),
        ));
    }

    let timeout = std::time::Duration::from_millis(
        timeout_ms
            .unwrap_or(state.command_timeout.as_millis() as u64)
            .max(1),
    );
    let outcome = tokio::time::timeout(timeout, pending_rx).await;
    let response = match outcome {
        Ok(Ok(AgentReply::Result(result))) => {
            info!("agent command completed");
            if let Err(err) = state
                .store
                .append_audit_event(AuditEventInput {
                    actor_type: "admin_api".into(),
                    actor_id: None,
                    agent_id: Some(agent_id.to_string()),
                    request_id: Some(request_id_string.clone()),
                    event_type: "command_result".into(),
                    outcome: "ok".into(),
                    latency_ms: Some(started.elapsed().as_millis() as u64),
                    message: "agent command completed".into(),
                    metadata: serde_json::json!({ "command": command_kind }),
                })
                .await
            {
                warn!(error = %err, "failed to append audit event for command success");
            }
            RelayCommandResponse {
                request_id: request_id_string,
                status: "ok".into(),
                result: Some(result),
                error: None,
            }
        }
        Ok(Ok(AgentReply::Error(error))) => {
            warn!(code = %error.code, "agent command returned error");
            if let Err(err) = state
                .store
                .append_audit_event(AuditEventInput {
                    actor_type: "admin_api".into(),
                    actor_id: None,
                    agent_id: Some(agent_id.to_string()),
                    request_id: Some(request_id_string.clone()),
                    event_type: "command_result".into(),
                    outcome: "error".into(),
                    latency_ms: Some(started.elapsed().as_millis() as u64),
                    message: error.message.clone(),
                    metadata: serde_json::json!({ "command": command_kind, "code": error.code }),
                })
                .await
            {
                warn!(error = %err, "failed to append audit event for command error result");
            }
            RelayCommandResponse {
                request_id: request_id_string,
                status: "error".into(),
                result: None,
                error: Some(error),
            }
        }
        Ok(Err(_)) => {
            warn!("agent response channel dropped");
            if let Err(err) = state
                .store
                .append_audit_event(AuditEventInput {
                    actor_type: "admin_api".into(),
                    actor_id: None,
                    agent_id: Some(agent_id.to_string()),
                    request_id: Some(request_id_string.clone()),
                    event_type: "command_result".into(),
                    outcome: "response_dropped".into(),
                    latency_ms: Some(started.elapsed().as_millis() as u64),
                    message: "agent response channel dropped".into(),
                    metadata: serde_json::json!({ "command": command_kind }),
                })
                .await
            {
                warn!(error = %err, "failed to append audit event for dropped response");
            }
            return Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "agent_response_dropped",
                "agent response channel dropped",
            ));
        }
        Err(_) => {
            state.pending.lock().await.remove(&request_id_string);
            warn!(
                timeout_ms = timeout.as_millis() as u64,
                "agent command timed out"
            );
            if let Err(err) = state
                .store
                .append_audit_event(AuditEventInput {
                    actor_type: "admin_api".into(),
                    actor_id: None,
                    agent_id: Some(agent_id.to_string()),
                    request_id: Some(request_id_string.clone()),
                    event_type: "command_result".into(),
                    outcome: "timeout".into(),
                    latency_ms: Some(started.elapsed().as_millis() as u64),
                    message: "agent command timed out".into(),
                    metadata: serde_json::json!({ "command": command_kind, "timeout_ms": timeout.as_millis() as u64 }),
                })
                .await
            {
                warn!(error = %err, "failed to append audit event for timeout");
            }
            return Err(ApiError::new(
                StatusCode::GATEWAY_TIMEOUT,
                "agent_timeout",
                "agent did not answer before timeout",
            ));
        }
    };

    Ok(response)
}

fn command_kind(command: &AgentCommand) -> &'static str {
    match command {
        AgentCommand::Leases(_) => "leases",
        AgentCommand::Devs(_) => "devs",
        AgentCommand::Inventory(_) => "inventory",
        AgentCommand::Wake(_) => "wake",
    }
}
