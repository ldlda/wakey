use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::api::json_error;
use crate::runtime::AppState;
use crate::state::AuditEventFilter;

#[derive(Debug, Deserialize)]
pub struct ListAuditEventsQuery {
    pub agent_id: Option<String>,
    pub request_id: Option<String>,
    pub event_type: Option<String>,
    pub outcome: Option<String>,
    pub since_unix: Option<u64>,
    pub until_unix: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct AuditEventResponse {
    pub event_id: String,
    pub ts_unix: u64,
    pub actor_type: String,
    pub actor_id: Option<String>,
    pub agent_id: Option<String>,
    pub request_id: Option<String>,
    pub event_type: String,
    pub outcome: String,
    pub latency_ms: Option<u64>,
    pub message: String,
    pub metadata: serde_json::Value,
}

pub async fn list_audit_events(
    State(state): State<AppState>,
    Query(query): Query<ListAuditEventsQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let filter = AuditEventFilter {
        agent_id: query.agent_id,
        request_id: query.request_id,
        event_type: query.event_type,
        outcome: query.outcome,
        since_unix: query.since_unix,
        until_unix: query.until_unix,
        limit: query.limit.unwrap_or(100),
    };

    match state.store.list_audit_events(filter).await {
        Ok(events) => {
            let body = events
                .into_iter()
                .map(|event| AuditEventResponse {
                    event_id: event.event_id,
                    ts_unix: event.ts_unix,
                    actor_type: event.actor_type,
                    actor_id: event.actor_id,
                    agent_id: event.agent_id,
                    request_id: event.request_id,
                    event_type: event.event_type,
                    outcome: event.outcome,
                    latency_ms: event.latency_ms,
                    message: event.message,
                    metadata: event.metadata,
                })
                .collect::<Vec<_>>();
            Ok((StatusCode::OK, Json(body)))
        }
        Err(err) => {
            warn!(error = %err, "failed to list audit events");
            Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "list_audit_events_failed",
                &err.to_string(),
            ))
        }
    }
}
