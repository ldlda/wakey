use crate::http::route::error::ApiError;
use axum::{extract::Json, http::StatusCode, response::IntoResponse};
pub use wakey_core::WakeTarget;

pub async fn wake_multi(Json(req): Json<Vec<WakeTarget>>) -> impl IntoResponse {
    match crate::service::wake_targets(req).await {
        Ok(result) => (
            StatusCode::OK,
            Json(crate::http::compat::legacy_wake_from_domain(result)),
        )
            .into_response(),
        Err(error) => {
            let error = format!("Error: {}", error);
            ApiError::ise(error).into_response()
        }
    }
}
