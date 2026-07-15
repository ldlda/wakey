use axum::Json;
use axum::http::StatusCode;

mod alerts;
mod audit;
mod commands;
mod control;
mod terminals;

pub use alerts::{active_alerts, alert_history, alerts_stream};
pub use audit::list_audit_events;
pub use commands::{list_agents, run_command};
pub use control::{
    EnrollTokenStatus, IssueEnrollTokenResponse, RevokeAgentResponse, RevokeEnrollTokenResponse,
    StateStatsResponse, attach_device_identifier, create_known_device, detach_device_identifier,
    enroll, forget_known_device, get_known_device, healthz, issue_enroll_token, list_enroll_tokens,
    list_fleet_devices, list_known_devices, merge_known_device, refresh_fleet_devices,
    revoke_agent, revoke_enroll_token, set_agent_nickname, state_stats, wake_fleet_device,
};
pub use terminals::{
    agent_terminal_ws, attach_terminal, close_terminal, create_terminal, get_terminal,
    operator_terminal_ws,
};

use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiErrorResponse {
    pub error: ApiErrorDetail,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiErrorDetail {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    #[allow(dead_code)]
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(ApiErrorResponse {
            error: ApiErrorDetail {
                code: self.code,
                message: self.message,
                details: self.details,
            },
        });
        (self.status, body).into_response()
    }
}
