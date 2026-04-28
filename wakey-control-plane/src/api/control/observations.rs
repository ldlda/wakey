use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use tracing::warn;
use wakey_agent::protocol::ServerMessage;

use crate::api::json_error;
use crate::runtime::{AppState, SessionEvent};
use crate::state::{
    AgentDeviceObservationEvent, AgentDeviceObservationInput, AgentDeviceObservationView,
};

#[derive(Debug, Deserialize)]
pub struct UploadAgentObservationsRequest {
    pub agent_id: String,
    pub agent_token: String,
    #[serde(default)]
    pub observations: Vec<AgentObservationRequest>,
}

#[derive(Debug, Deserialize)]
pub struct AgentObservationRequest {
    pub kind: String,
    pub action: String,
    pub mac: Option<String>,
    pub ip: Option<String>,
    pub hostname: Option<String>,
    pub first_seen_unix: u64,
    pub last_seen_unix: u64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct UploadAgentObservationsResponse {
    pub accepted: usize,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct RequestAgentObservationSyncResponse {
    pub agent_id: String,
    pub requested: bool,
}

#[derive(Debug, Deserialize)]
pub struct ListObservationsQuery {
    pub agent_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct ListObservationHistoryQuery {
    pub agent_id: Option<String>,
    pub kind: Option<String>,
    pub mac: Option<String>,
    pub ip: Option<String>,
    pub observation_key: Option<String>,
    pub limit: Option<usize>,
}

pub async fn upload_agent_observations(
    State(state): State<AppState>,
    Json(req): Json<UploadAgentObservationsRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if !state
        .store
        .verify_agent_token(&req.agent_id, &req.agent_token)
        .await
    {
        return Err(json_error(
            StatusCode::UNAUTHORIZED,
            "agent_auth_rejected",
            "agent credentials rejected",
        ));
    }

    let observations = req
        .observations
        .into_iter()
        .map(|observation| AgentDeviceObservationInput {
            kind: observation.kind,
            action: observation.action,
            mac: observation.mac,
            ip: observation.ip,
            hostname: observation.hostname,
            first_seen_unix: observation.first_seen_unix,
            last_seen_unix: observation.last_seen_unix,
        })
        .collect();

    match state
        .store
        .upsert_agent_observations(&req.agent_id, observations)
        .await
    {
        Ok(accepted) => Ok((
            StatusCode::OK,
            Json(UploadAgentObservationsResponse { accepted }),
        )),
        Err(err) => {
            warn!(error = %err, agent_id = %req.agent_id, "failed to upload agent observations");
            Err(json_error(
                StatusCode::BAD_REQUEST,
                "upload_observations_failed",
                &err.to_string(),
            ))
        }
    }
}

pub async fn list_agent_observations(
    State(state): State<AppState>,
    Query(query): Query<ListObservationsQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match state
        .store
        .list_agent_observation_views(query.agent_id.as_deref(), query.limit.unwrap_or(500))
        .await
    {
        Ok(observations) => Ok((
            StatusCode::OK,
            Json(
                observations
                    .into_iter()
                    .map(agent_observation_response)
                    .collect::<Vec<_>>(),
            ),
        )),
        Err(err) => {
            warn!(error = %err, "failed to list agent observations");
            Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "list_observations_failed",
                &err.to_string(),
            ))
        }
    }
}

pub async fn request_agent_observation_sync(
    State(state): State<AppState>,
    AxumPath(agent_id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let tx = {
        let sessions = state.sessions.read().await;
        sessions.get(&agent_id).map(|session| session.tx.clone())
    };
    let Some(tx) = tx else {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(RequestAgentObservationSyncResponse {
                agent_id,
                requested: false,
            }),
        ));
    };

    match tx.send(SessionEvent::Message(ServerMessage::SyncObservations)) {
        Ok(()) => Ok((
            StatusCode::OK,
            Json(RequestAgentObservationSyncResponse {
                agent_id,
                requested: true,
            }),
        )),
        Err(err) => {
            warn!(error = %err, "failed to request agent observation sync");
            Err(json_error(
                StatusCode::BAD_GATEWAY,
                "agent_observation_sync_request_failed",
                &err.to_string(),
            ))
        }
    }
}

pub async fn list_agent_observation_history(
    State(state): State<AppState>,
    Query(query): Query<ListObservationHistoryQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let kind = normalized_query_value(query.kind).map(|value| value.to_ascii_lowercase());
    let mac = normalized_query_value(query.mac).map(|value| value.to_ascii_lowercase());
    let ip = normalized_query_value(query.ip);
    let observation_key = normalized_query_value(query.observation_key);
    match state
        .store
        .list_agent_observation_events(
            query.agent_id.as_deref(),
            kind.as_deref(),
            mac.as_deref(),
            ip.as_deref(),
            observation_key.as_deref(),
            query.limit.unwrap_or(500),
        )
        .await
    {
        Ok(events) => Ok((
            StatusCode::OK,
            Json(
                events
                    .into_iter()
                    .map(agent_observation_event_response)
                    .collect::<Vec<_>>(),
            ),
        )),
        Err(err) => {
            warn!(error = %err, "failed to list agent observation history");
            Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "list_observation_history_failed",
                &err.to_string(),
            ))
        }
    }
}

fn agent_observation_response(
    observation: AgentDeviceObservationView,
) -> AgentDeviceObservationView {
    observation
}

fn agent_observation_event_response(
    event: AgentDeviceObservationEvent,
) -> AgentDeviceObservationEvent {
    event
}

fn normalized_query_value(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
