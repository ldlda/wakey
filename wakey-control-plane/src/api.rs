use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use tracing::{info, info_span, warn};
use uuid::Uuid;
use wakey_agent::protocol::{AgentCommand, ErrorPayload, RequestId, ServerMessage};

use crate::runtime::{AgentReply, AppState};

#[derive(Debug, Deserialize)]
pub struct EnrollRequest {
    pub enroll_token: String,
}

#[derive(Debug, Serialize)]
pub struct EnrollResponse {
    pub agent_id: String,
    pub agent_token: String,
    pub server_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IssueEnrollTokenResponse {
    pub enroll_token: String,
    pub expires_at_unix: u64,
}

#[derive(Debug, Deserialize)]
pub struct IssueEnrollTokenQuery {
    pub ttl_seconds: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct AgentStatus {
    pub agent_id: String,
    pub connected: bool,
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

pub async fn healthz() -> &'static str {
    "ok"
}

pub async fn enroll(
    State(state): State<AppState>,
    Json(req): Json<EnrollRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match state.store.enroll(&req.enroll_token).await {
        Ok(issued) => {
            info!(agent_id = %issued.agent_id, "agent enrollment accepted");
            Ok((
                StatusCode::OK,
                Json(EnrollResponse {
                    agent_id: issued.agent_id,
                    agent_token: issued.agent_token,
                    server_url: state.public_url,
                }),
            ))
        }
        Err(err) => {
            warn!(error = %err, "agent enrollment rejected");
            Err(json_error(
                StatusCode::UNAUTHORIZED,
                "enrollment_rejected",
                &err.to_string(),
            ))
        }
    }
}

pub async fn issue_enroll_token(
    State(state): State<AppState>,
    Query(query): Query<IssueEnrollTokenQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let ttl = std::time::Duration::from_secs(
        query
            .ttl_seconds
            .unwrap_or(state.enroll_token_ttl.as_secs())
            .max(1),
    );

    match state.store.issue_enroll_token(ttl).await {
        Ok(issued) => {
            info!(expires_at_unix = issued.expires_at_unix, "issued enroll token");
            Ok((
                StatusCode::OK,
                Json(IssueEnrollTokenResponse {
                    enroll_token: issued.enroll_token,
                    expires_at_unix: issued.expires_at_unix,
                }),
            ))
        }
        Err(err) => {
            warn!(error = %err, "failed to issue enroll token");
            Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "issue_enroll_token_failed",
                &err.to_string(),
            ))
        }
    }
}

pub async fn list_agents(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let enrolled = state.store.list_agents().await;
    let sessions = state.sessions.read().await;

    let agents = enrolled
        .into_iter()
        .map(|agent_id| AgentStatus {
            connected: sessions.contains_key(&agent_id),
            agent_id,
        })
        .collect::<Vec<_>>();

    Ok((StatusCode::OK, Json(agents)))
}

pub async fn run_command(
    State(state): State<AppState>,
    AxumPath(agent_id): AxumPath<String>,
    Json(req): Json<RelayCommandRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let request_id_string = format!("req-{}", Uuid::new_v4());
    let command = command_kind(&req.command);
    let span = info_span!(
        "relay_command",
        agent_id = %agent_id,
        request_id = %request_id_string,
        command = %command,
    );
    let _span_guard = span.enter();

    let request_id = RequestId::try_from(request_id_string.clone()).map_err(|err| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid_request_id",
            &err,
        )
    })?;

    let tx = {
        let sessions = state.sessions.read().await;
        sessions.get(&agent_id).cloned()
    }
    .ok_or_else(|| {
        warn!("command rejected: agent not connected");
        json_error(
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

    info!(
        "dispatching command to agent"
    );

    if let Err(err) = tx.send(ServerMessage::Command {
        request_id,
        command: req.command,
    }) {
        state.pending.lock().await.remove(&request_id_string);
        warn!(error = %err, "failed sending command to agent session");
        return Err(json_error(
            StatusCode::BAD_GATEWAY,
            "agent_send_failed",
            &format!("failed to send command to agent: {err}"),
        ));
    }

    let timeout = std::time::Duration::from_millis(
        req.timeout_ms
            .unwrap_or(state.command_timeout.as_millis() as u64)
            .max(1),
    );
    let outcome = tokio::time::timeout(timeout, pending_rx).await;
    let response = match outcome {
        Ok(Ok(AgentReply::Result(result))) => {
            info!("agent command completed");
            RelayCommandResponse {
                request_id: request_id_string,
                status: "ok".into(),
                result: Some(result),
                error: None,
            }
        }
        Ok(Ok(AgentReply::Error(error))) => {
            warn!(code = %error.code, "agent command returned error");
            RelayCommandResponse {
                request_id: request_id_string,
                status: "error".into(),
                result: None,
                error: Some(error),
            }
        }
        Ok(Err(_)) => {
            warn!("agent response channel dropped");
            return Err(json_error(
                StatusCode::BAD_GATEWAY,
                "agent_response_dropped",
                "agent response channel dropped",
            ));
        }
        Err(_) => {
            state.pending.lock().await.remove(&request_id_string);
            warn!(timeout_ms = timeout.as_millis() as u64, "agent command timed out");
            return Err(json_error(
                StatusCode::GATEWAY_TIMEOUT,
                "agent_timeout",
                "agent did not answer before timeout",
            ));
        }
    };

    Ok((StatusCode::OK, Json(response)))
}

pub fn json_error(
    status: StatusCode,
    code: &str,
    message: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "error": {
                "code": code,
                "message": message,
            }
        })),
    )
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
