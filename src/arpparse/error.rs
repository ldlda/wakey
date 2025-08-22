use std::net::AddrParseError;

use strum::Display;
use thiserror::Error;

#[derive(Debug, Display, Error)]
pub enum IPNeighParseError {
    IpWhere,
    IpParseError(AddrParseError),
    DevWhere,
    MacParseError(macaddr::ParseError),
    StateWhere,
    StateParseError(strum::ParseError),
}

impl From<AddrParseError> for IPNeighParseError {
    fn from(value: AddrParseError) -> Self {
        Self::IpParseError(value)
    }
}
impl From<macaddr::ParseError> for IPNeighParseError {
    fn from(value: macaddr::ParseError) -> Self {
        Self::MacParseError(value)
    }
}
impl From<strum::ParseError> for IPNeighParseError {
    fn from(value: strum::ParseError) -> Self {
        Self::StateParseError(value)
    }
}