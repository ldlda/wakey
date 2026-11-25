//! Shared domain types and parsing for DHCP, neighbors, interfaces, devices, and wake flows.

pub mod auth;
pub mod config_duration;
pub mod model;
pub mod parse;
pub mod process_status;
pub mod protocol;

#[cfg(feature = "telemetry")]
pub mod telemetry;

pub use model::*;
