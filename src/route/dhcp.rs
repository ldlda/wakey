use crate::{dhcpparse, route::api::StatusError, utils::parse::boolish_str};
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

    match dhcpparse::read_dhcp_leases_with_names().await {
        Ok(leases_with_names) => {
            if !include_state {
                return (StatusCode::OK, Json(leases_with_names)).into_response();
            }
            let out = crate::utils::query::enrich_leases_with_nud_state(leases_with_names).await;
            (StatusCode::OK, Json(out)).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(StatusError {
                error: e.to_string(),
                ..Default::default()
            }),
        )
            .into_response(),
    }
}
