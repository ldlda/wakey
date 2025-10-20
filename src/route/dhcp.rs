use crate::{
    dhcpparse::read_dhcp_leases_with_names,
    route::error::ApiError,
    utils::{parse::boolish_str, query::enrich_leases_with_nud_state},
};
use axum::{Json, extract::Query, http::StatusCode, response::IntoResponse};

// DHCP lease endpoints
#[derive(Debug, Default, Clone, serde::Deserialize)]
pub struct DhcpLeasesQueryRaw {
    include_state: Option<String>,
}

pub async fn get_dhcp_leases(Query(raw): Query<DhcpLeasesQueryRaw>) -> impl IntoResponse {
    let include_state = raw
        .include_state
        .as_deref()
        .map(boolish_str)
        .unwrap_or(false);

    match read_dhcp_leases_with_names().await {
        Ok(leases_with_names) => {
            if !include_state {
                return (StatusCode::OK, Json(leases_with_names)).into_response();
            }
            let out = enrich_leases_with_nud_state(leases_with_names).await;
            (StatusCode::OK, Json(out)).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(ApiError {
                error: e.to_string(),
            }),
        )
            .into_response(),
    }
}
