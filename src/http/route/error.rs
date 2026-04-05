use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ApiError<T: Serialize> {
    #[serde(skip_serializing, skip_deserializing)]
    pub code: StatusCode,
    pub error: T,
}

impl<T: Serialize> ApiError<T> {
    /// [StatusCode::INTERNAL_SERVER_ERROR] shortcut
    pub const fn ise(error: T) -> Self {
        Self {
            code: StatusCode::INTERNAL_SERVER_ERROR,
            error,
        }
    }
}

impl<T: Serialize> IntoResponse for ApiError<T> {
    fn into_response(self) -> Response {
        (self.code, Json(self)).into_response()
    }
}
