use std::net::IpAddr;

use macaddr::MacAddr;

use crate::{arpparse::NUDState, utils::query::dev::has_dev};

pub enum QueryType {
    Ip(IpAddr),
    Mac(MacAddr),
    Dev(String),
    Nud(NUDState),
    Unknown(String),
}

pub fn parse_query(q: String) -> QueryType {
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
        return QueryType::Ip(ip);
    }
    // 2) MAC
    if let Ok(mac) = s.parse::<MacAddr>() {
        return QueryType::Mac(mac);
    }
    // 3) NUD state (reachable, stale, ...)
    if let Ok(state) = s.parse::<NUDState>() {
        return QueryType::Nud(state);
    }
    // 4) Known device? prefer dev first
    if has_dev(s) {
        return QueryType::Dev(s.to_string());
    }
    // Default: name last // it will fail also
    QueryType::Unknown(s.to_string())
}
