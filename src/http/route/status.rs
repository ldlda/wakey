use crate::http::route::error::ApiError;
use axum::{Json, http::StatusCode, response::IntoResponse};
use axum_extra::extract::Query;
pub use wakey_core::{DeviceQuery, NamePath};

pub async fn get_status_json(Query(query): Query<DeviceQuery>) -> impl IntoResponse {
    match crate::service::inventory(query.clone()).await {
        Ok(inventory) => (
            StatusCode::OK,
            Json(crate::http::compat::legacy_status_from_inventory(
                inventory,
                query.name,
                query.filter,
            )),
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
