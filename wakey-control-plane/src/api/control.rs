use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::api::json_error;
use crate::runtime::{AppState, SessionEvent};
use crate::state::{
    AgentDeviceObservationInput, AgentDeviceObservationView, AuditEventInput,
    DeviceIdentifierInput, KnownDeviceInput,
};

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

#[derive(Debug, Serialize, Deserialize)]
pub struct EnrollTokenStatus {
    pub enroll_token: String,
    pub expires_at_unix: u64,
    pub expired: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RevokeEnrollTokenResponse {
    pub token: String,
    pub revoked: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RevokeAgentResponse {
    pub agent_id: String,
    pub revoked: bool,
}

#[derive(Debug, Deserialize)]
pub struct SetAgentNicknameRequest {
    pub nickname: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetAgentNicknameResponse {
    pub agent_id: String,
    pub nickname: Option<String>,
    pub updated: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StateStatsResponse {
    pub db_path: String,
    pub schema_version: u32,
    pub agent_count: usize,
    pub enroll_token_count: usize,
    pub expired_enroll_token_count: usize,
}

#[derive(Debug, Deserialize)]
pub struct CreateKnownDeviceRequest {
    pub display_name: String,
    #[serde(default)]
    pub pinned: bool,
    pub notes: Option<String>,
    #[serde(default)]
    pub identifiers: Vec<DeviceIdentifierRequest>,
}

#[derive(Debug, Deserialize)]
pub struct DeviceIdentifierRequest {
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub struct AttachObservationIdentifierRequest {
    pub observation_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct KnownDeviceResponse {
    pub device_id: String,
    pub display_name: String,
    pub pinned: bool,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
    pub notes: Option<String>,
    pub identifiers: Vec<DeviceIdentifierResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceIdentifierResponse {
    pub identifier_key: String,
    pub device_id: String,
    pub kind: String,
    pub value: String,
    pub created_at_unix: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ForgetKnownDeviceResponse {
    pub device_id: String,
    pub forgotten: bool,
}

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

#[derive(Debug, Serialize, Deserialize)]
pub struct UploadAgentObservationsResponse {
    pub accepted: usize,
}

#[derive(Debug, Deserialize)]
pub struct ListObservationsQuery {
    pub agent_id: Option<String>,
    pub limit: Option<usize>,
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
            if let Err(err) = state
                .store
                .append_audit_event(AuditEventInput {
                    actor_type: "agent".into(),
                    actor_id: Some(issued.agent_id.clone()),
                    agent_id: Some(issued.agent_id.clone()),
                    request_id: None,
                    event_type: "agent_enroll".into(),
                    outcome: "ok".into(),
                    latency_ms: None,
                    message: "agent enrollment accepted".into(),
                    metadata: serde_json::json!({}),
                })
                .await
            {
                warn!(error = %err, "failed to append audit event for enroll success");
            }
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
            if let Err(audit_err) = state
                .store
                .append_audit_event(AuditEventInput {
                    actor_type: "agent".into(),
                    actor_id: None,
                    agent_id: None,
                    request_id: None,
                    event_type: "agent_enroll".into(),
                    outcome: "rejected".into(),
                    latency_ms: None,
                    message: err.to_string(),
                    metadata: serde_json::json!({}),
                })
                .await
            {
                warn!(error = %audit_err, "failed to append audit event for enroll rejection");
            }
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
            info!(
                expires_at_unix = issued.expires_at_unix,
                "issued enroll token"
            );
            if let Err(err) = state
                .store
                .append_audit_event(AuditEventInput {
                    actor_type: "admin_api".into(),
                    actor_id: None,
                    agent_id: None,
                    request_id: None,
                    event_type: "enroll_token_issue".into(),
                    outcome: "ok".into(),
                    latency_ms: None,
                    message: "issued enroll token".into(),
                    metadata: serde_json::json!({
                        "ttl_seconds": ttl.as_secs(),
                        "expires_at_unix": issued.expires_at_unix,
                    }),
                })
                .await
            {
                warn!(error = %err, "failed to append audit event for token issuance");
            }
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

pub async fn list_enroll_tokens(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match state.store.list_enroll_tokens().await {
        Ok(tokens) => {
            if let Err(err) = state
                .store
                .append_audit_event(AuditEventInput {
                    actor_type: "admin_api".into(),
                    actor_id: None,
                    agent_id: None,
                    request_id: None,
                    event_type: "enroll_token_list".into(),
                    outcome: "ok".into(),
                    latency_ms: None,
                    message: "listed enroll tokens".into(),
                    metadata: serde_json::json!({
                        "count": tokens.len(),
                    }),
                })
                .await
            {
                warn!(error = %err, "failed to append audit event for token listing");
            }
            let body = tokens
                .into_iter()
                .map(|t| EnrollTokenStatus {
                    enroll_token: t.enroll_token,
                    expires_at_unix: t.expires_at_unix,
                    expired: t.expired,
                })
                .collect::<Vec<_>>();
            Ok((StatusCode::OK, Json(body)))
        }
        Err(err) => {
            warn!(error = %err, "failed to list enroll tokens");
            Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "list_enroll_tokens_failed",
                &err.to_string(),
            ))
        }
    }
}

pub async fn revoke_enroll_token(
    State(state): State<AppState>,
    AxumPath(token): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match state.store.revoke_enroll_token(&token).await {
        Ok(revoked) => {
            if let Err(err) = state
                .store
                .append_audit_event(AuditEventInput {
                    actor_type: "admin_api".into(),
                    actor_id: None,
                    agent_id: None,
                    request_id: None,
                    event_type: "enroll_token_revoke".into(),
                    outcome: if revoked {
                        "ok".into()
                    } else {
                        "not_found".into()
                    },
                    latency_ms: None,
                    message: if revoked {
                        "revoked enroll token".into()
                    } else {
                        "enroll token not found".into()
                    },
                    metadata: serde_json::json!({ "token": token }),
                })
                .await
            {
                warn!(error = %err, "failed to append audit event for token revoke");
            }
            Ok((
                StatusCode::OK,
                Json(RevokeEnrollTokenResponse { token, revoked }),
            ))
        }
        Err(err) => {
            warn!(error = %err, "failed to revoke enroll token");
            Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "revoke_enroll_token_failed",
                &err.to_string(),
            ))
        }
    }
}

pub async fn revoke_agent(
    State(state): State<AppState>,
    AxumPath(agent_id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match state.store.revoke_agent(&agent_id).await {
        Ok(revoked) => {
            if revoked {
                // Request a graceful websocket close, then remove session from active map.
                if let Some(session) = state.sessions.write().await.remove(&agent_id) {
                    let _ = session.tx.send(SessionEvent::Close);
                }
            }

            if let Err(err) = state
                .store
                .append_audit_event(AuditEventInput {
                    actor_type: "admin_api".into(),
                    actor_id: None,
                    agent_id: Some(agent_id.clone()),
                    request_id: None,
                    event_type: "agent_revoke".into(),
                    outcome: if revoked {
                        "ok".into()
                    } else {
                        "not_found".into()
                    },
                    latency_ms: None,
                    message: if revoked {
                        "revoked agent credentials".into()
                    } else {
                        "agent credentials not found".into()
                    },
                    metadata: serde_json::json!({ "agent_id": agent_id }),
                })
                .await
            {
                warn!(error = %err, "failed to append audit event for agent revoke");
            }

            Ok((
                StatusCode::OK,
                Json(RevokeAgentResponse { agent_id, revoked }),
            ))
        }
        Err(err) => {
            warn!(error = %err, "failed to revoke agent credentials");
            Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "revoke_agent_failed",
                &err.to_string(),
            ))
        }
    }
}

pub async fn set_agent_nickname(
    State(state): State<AppState>,
    AxumPath(agent_id): AxumPath<String>,
    Json(req): Json<SetAgentNicknameRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let normalized = req
        .nickname
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned);

    match state
        .store
        .set_agent_nickname(&agent_id, normalized.as_deref())
        .await
    {
        Ok(updated) => {
            if let Err(err) = state
                .store
                .append_audit_event(AuditEventInput {
                    actor_type: "admin_api".into(),
                    actor_id: None,
                    agent_id: Some(agent_id.clone()),
                    request_id: None,
                    event_type: "agent_nickname_set".into(),
                    outcome: if updated {
                        "ok".into()
                    } else {
                        "not_found".into()
                    },
                    latency_ms: None,
                    message: if updated {
                        "updated agent nickname".into()
                    } else {
                        "agent not found for nickname update".into()
                    },
                    metadata: serde_json::json!({
                        "agent_id": agent_id,
                        "nickname": normalized,
                    }),
                })
                .await
            {
                warn!(error = %err, "failed to append audit event for nickname set");
            }

            Ok((
                StatusCode::OK,
                Json(SetAgentNicknameResponse {
                    agent_id,
                    nickname: normalized,
                    updated,
                }),
            ))
        }
        Err(err) => {
            warn!(error = %err, "failed to update agent nickname");
            Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "set_agent_nickname_failed",
                &err.to_string(),
            ))
        }
    }
}

pub async fn create_known_device(
    State(state): State<AppState>,
    Json(req): Json<CreateKnownDeviceRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let input = KnownDeviceInput {
        display_name: req.display_name,
        pinned: req.pinned,
        notes: req.notes,
        identifiers: req
            .identifiers
            .into_iter()
            .map(|identifier| DeviceIdentifierInput {
                kind: identifier.kind,
                value: identifier.value,
            })
            .collect(),
    };

    match state.store.create_known_device(input).await {
        Ok(device) => Ok((StatusCode::CREATED, Json(known_device_response(device)))),
        Err(err) => {
            warn!(error = %err, "failed to create known device");
            Err(json_error(
                StatusCode::BAD_REQUEST,
                "create_known_device_failed",
                &err.to_string(),
            ))
        }
    }
}

pub async fn list_known_devices(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match state.store.list_known_devices().await {
        Ok(devices) => Ok((
            StatusCode::OK,
            Json(
                devices
                    .into_iter()
                    .map(known_device_response)
                    .collect::<Vec<_>>(),
            ),
        )),
        Err(err) => {
            warn!(error = %err, "failed to list known devices");
            Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "list_known_devices_failed",
                &err.to_string(),
            ))
        }
    }
}

pub async fn forget_known_device(
    State(state): State<AppState>,
    AxumPath(device_id): AxumPath<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match state.store.forget_known_device(&device_id).await {
        Ok(forgotten) => Ok((
            StatusCode::OK,
            Json(ForgetKnownDeviceResponse {
                device_id,
                forgotten,
            }),
        )),
        Err(err) => {
            warn!(error = %err, "failed to forget known device");
            Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "forget_known_device_failed",
                &err.to_string(),
            ))
        }
    }
}

pub async fn attach_device_identifier(
    State(state): State<AppState>,
    AxumPath(device_id): AxumPath<String>,
    Json(req): Json<DeviceIdentifierRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let input = DeviceIdentifierInput {
        kind: req.kind,
        value: req.value,
    };

    match state
        .store
        .attach_device_identifier(&device_id, input)
        .await
    {
        Ok(Some(device)) => Ok((StatusCode::OK, Json(known_device_response(device)))),
        Ok(None) => Err(json_error(
            StatusCode::NOT_FOUND,
            "known_device_not_found",
            "known device not found",
        )),
        Err(err) => {
            warn!(error = %err, "failed to attach device identifier");
            Err(json_error(
                StatusCode::BAD_REQUEST,
                "attach_device_identifier_failed",
                &err.to_string(),
            ))
        }
    }
}

pub async fn attach_observation_identifier(
    State(state): State<AppState>,
    AxumPath(device_id): AxumPath<String>,
    Json(req): Json<AttachObservationIdentifierRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match state
        .store
        .attach_observation_identifier(&device_id, &req.observation_key)
        .await
    {
        Ok(Some(device)) => Ok((StatusCode::OK, Json(known_device_response(device)))),
        Ok(None) => Err(json_error(
            StatusCode::NOT_FOUND,
            "known_device_not_found",
            "known device not found",
        )),
        Err(err) => {
            warn!(error = %err, "failed to attach observation identifier");
            Err(json_error(
                StatusCode::BAD_REQUEST,
                "attach_observation_identifier_failed",
                &err.to_string(),
            ))
        }
    }
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
                    .map(agent_observation_response) // no-op premium
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

pub async fn state_stats(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match state.store.stats().await {
        Ok(stats) => Ok((
            StatusCode::OK,
            Json(StateStatsResponse {
                db_path: stats.db_path.display().to_string(),
                schema_version: stats.schema_version,
                agent_count: stats.agent_count,
                enroll_token_count: stats.enroll_token_count,
                expired_enroll_token_count: stats.expired_enroll_token_count,
            }),
        )),
        Err(err) => {
            warn!(error = %err, "failed to read state stats");
            Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "state_stats_failed",
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

fn known_device_response(device: crate::state::KnownDevice) -> KnownDeviceResponse {
    KnownDeviceResponse {
        device_id: device.device_id,
        display_name: device.display_name,
        pinned: device.pinned,
        created_at_unix: device.created_at_unix,
        updated_at_unix: device.updated_at_unix,
        notes: device.notes,
        identifiers: device
            .identifiers
            .into_iter()
            .map(|identifier| DeviceIdentifierResponse {
                identifier_key: identifier.identifier_key,
                device_id: identifier.device_id,
                kind: identifier.kind,
                value: identifier.value,
                created_at_unix: identifier.created_at_unix,
            })
            .collect(),
    }
}
