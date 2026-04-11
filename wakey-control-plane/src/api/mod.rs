use axum::Json;
use axum::http::StatusCode;

mod commands;
mod control;
mod audit;

pub use commands::{list_agents, run_command};
pub use audit::list_audit_events;
pub use control::{
    EnrollTokenStatus, IssueEnrollTokenResponse, RevokeEnrollTokenResponse, StateStatsResponse,
    enroll, healthz, issue_enroll_token, list_enroll_tokens, revoke_enroll_token, state_stats,
};

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
