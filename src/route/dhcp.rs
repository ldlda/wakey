use crate::{route::error::ApiError, utils::parse::boolish_str};
use axum::{Json, extract::Query, http::StatusCode, response::IntoResponse};

// DHCP lease endpoints
#[derive(Debug, Default, Clone, serde::Deserialize)]
pub struct DhcpLeasesQueryRaw {
    include_state: Option<String>,
}

pub async fn get_dhcp_leases(
    Query(DhcpLeasesQueryRaw { include_state }): Query<DhcpLeasesQueryRaw>,
) -> impl IntoResponse {
    let include_state = include_state.as_deref().map(boolish_str).unwrap_or(false);

    match crate::get_leases(wakey_core::LeaseQuery { include_state }).await {
        Ok(leases) => (
            StatusCode::OK,
            Json(crate::compat::legacy_leases_from_domain(leases)),
        )
            .into_response(),
        Err(e) => ApiError {
            error: e.to_string(),
            code: StatusCode::BAD_GATEWAY,
        }
        .into_response(),
    }
}
