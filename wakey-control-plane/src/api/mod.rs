use axum::Json;
use axum::http::StatusCode;

mod alerts;
mod audit;
mod commands;
mod control;

pub use alerts::{active_alerts, alert_history, alerts_stream};
pub use audit::list_audit_events;
pub use commands::{list_agents, run_command};
pub use control::{
    EnrollTokenStatus, IssueEnrollTokenResponse, RevokeAgentResponse, RevokeEnrollTokenResponse,
    StateStatsResponse, attach_device_identifier, create_known_device, detach_device_identifier,
    enroll, forget_known_device, healthz, issue_enroll_token, list_enroll_tokens,
    list_fleet_devices, list_known_devices, merge_known_device, refresh_fleet_devices,
    revoke_agent, revoke_enroll_token, set_agent_nickname, state_stats, wake_fleet_device,
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
