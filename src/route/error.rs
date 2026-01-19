use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ApiError {
    #[serde(skip_serializing, skip_deserializing)]
    pub code: StatusCode,
    pub error: String,
}

impl ApiError {
    /// [StatusCode::INTERNAL_SERVER_ERROR] shortcut
    pub const fn ise(error: String) -> Self {
        Self {
            code: StatusCode::INTERNAL_SERVER_ERROR,
            error,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.code, Json(self)).into_response()
    }
}
