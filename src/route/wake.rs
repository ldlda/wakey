use crate::route::error::ApiError;
use axum::{extract::Json, http::StatusCode, response::IntoResponse};
pub use wakey_core::{WakeResult, WakeTarget};

pub async fn wake_multi(Json(req): Json<Vec<WakeTarget>>) -> impl IntoResponse {
    match crate::wake_targets(req).await {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(error) => {
            let error = format!("Error: {}", error);
            ApiError::ise(error).into_response()
        }
    }
}
