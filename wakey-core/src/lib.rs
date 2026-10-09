//! Shared domain types and parsing for DHCP, neighbors, interfaces, devices, and wake flows.

pub mod model;
pub mod parse;

#[cfg(feature = "telemetry")]
pub mod telemetry;

pub use model::*;
