use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::api::json_error;
use crate::runtime::{AppState, SessionEvent};
use crate::state::AuditEventInput;

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
