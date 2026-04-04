use crate::route::error::ApiError;
use axum::{Json, http::StatusCode, response::IntoResponse};
use axum_extra::extract::Query;
pub use wakey_core::{DeviceQuery, NamePath};

pub async fn get_status_json(Query(query): Query<DeviceQuery>) -> impl IntoResponse {
    match crate::get_status(query).await {
        Ok(status) => (
            StatusCode::OK,
            Json(crate::compat::legacy_status_from_domain(status)),
        )
            .into_response(),
        Err(error) => ApiError {
            code: StatusCode::BAD_GATEWAY,
            error: error
                .chain()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(": "),
        }
        .into_response(),
    }
}
// Status endpoints
