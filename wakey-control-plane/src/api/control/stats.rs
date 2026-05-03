use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::api::ApiError;
use crate::runtime::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct StateStatsResponse {
    pub db_path: String,
    pub schema_version: u32,
    pub agent_count: usize,
    pub enroll_token_count: usize,
    pub expired_enroll_token_count: usize,
}

pub async fn state_stats(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
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
            Err(ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "state_stats_failed",
                err.to_string(),
            ))
        }
    }
}
