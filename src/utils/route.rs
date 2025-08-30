use std::net::IpAddr;

use axum::{http::header, response::IntoResponse};
use macaddr::MacAddr;

use crate::{arpparse::NUDState, route::DeviceQuery, utils::query::dev::has_dev};

pub async fn serve_js(content: &'static str) -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "application/javascript"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        content,
    )
}

pub async fn status_smart_redirect(q: String) -> DeviceQuery {
    let s = if cfg!(feature = "very-smart-parsing") {
        crate::utils::parse::extract_host(&q)
    } else {
        q.trim()
    };
    // 1) IP
    let ip = if cfg!(feature = "very-smart-parsing") {
        crate::utils::parse::parse_numeric_ipv4(s).or_else(|| s.parse::<IpAddr>().ok())
    } else {
        s.parse::<IpAddr>().ok()
    };
    if let Some(ip) = ip {
        let ip = vec![ip];
        return DeviceQuery {
            ip,
            ..Default::default()
        };
    }
    // 2) MAC
    if let Ok(mac) = s.parse::<MacAddr>() {
        let mac = vec![mac];
        return DeviceQuery {
            mac,
            ..Default::default()
        };
    }
    // 3) NUD state (reachable, stale, ...)
    if let Ok(state) = s.parse::<NUDState>() {
        let nud = vec![state];
        return DeviceQuery {
            nud,
            ..Default::default()
        };
    }
    // 4) Known device? prefer dev first
    if has_dev(s) {
        return DeviceQuery {
            dev: vec![s.to_string()],
            ..Default::default()
        };
    }
    // 5) Try DNS: if it resolves, treat as name
    if tokio::net::lookup_host((s, 0)).await.is_ok() {
        return DeviceQuery {
            name: Some(s.to_string()),
            ..Default::default()
        };
    }
    // Default: name last // it will fail also
    DeviceQuery {
        name: Some(s.to_string()),
        ..Default::default()
    }
}
