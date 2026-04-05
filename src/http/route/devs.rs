use crate::http::route::error::ApiError;
use axum::{Json, response::IntoResponse};

pub async fn devs_router() -> impl IntoResponse {
    match crate::service::list_interfaces().await {
        Ok(devs) => Json(devs).into_response(),
        Err(e) => ApiError::ise(e.to_string()).into_response(),
    }
}
// Device listing endpoints
