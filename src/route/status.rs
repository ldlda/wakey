use crate::route::error::ApiError;
use axum::{Json, http::StatusCode, response::IntoResponse};
use axum_extra::extract::Query;
use crate::utils::query::get_macs;
pub use wakey_core::{DeviceQuery, NamePath, Status};

pub async fn get_status_json(
    Query(DeviceQuery {
        name,
        filter: filters,
        ..
    }): Query<DeviceQuery>,
) -> impl IntoResponse {
    match get_macs(
        name.as_slice(),
        &filters.ips,
        &filters.devs,
        &filters.nuds,
        &filters.macs,
    )
    .await
    {
        Ok(table) => (
            StatusCode::OK,
            Json(Status {
                name,
                table,
                filters,
            }),
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
