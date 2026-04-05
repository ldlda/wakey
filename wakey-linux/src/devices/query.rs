use std::net::IpAddr;

use wakey_core::{NeighborState, QueryInput, parse};

use crate::devices::interfaces::has_dev;

pub async fn classify_query(q: String) -> QueryInput {
    let s = parse::extract_host(&q);
    if let Some(ip) = parse::parse_numeric_ipv4(s).or_else(|| s.parse::<IpAddr>().ok()) {
        return QueryInput::Ip(ip);
    }
    if let Ok(mac) = s.parse::<macaddr::MacAddr>() {
        return QueryInput::Mac(mac);
    }
    if let Ok(state) = s.parse::<NeighborState>() {
        return QueryInput::Nud(state);
    }
    if has_dev(s).await {
        return QueryInput::Dev(s.to_string());
    }
    QueryInput::Name(s.to_string())
}
