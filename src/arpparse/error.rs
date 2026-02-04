use std::net::AddrParseError;

use strum::Display;
use thiserror::Error;

#[derive(Debug, Display, Error)]
pub enum IPNeighParseError {
    IpWhere, // i never seen a ip neigh where the first thing aint an ip
    IpParseError(#[from] AddrParseError),
    // DevWhere,
    MacParseError(#[from] macaddr::ParseError),
    StateWhere, // i never seen a ip neigh without the big FAILED at the end
    StateParseError(#[from] strum::ParseError),
}
