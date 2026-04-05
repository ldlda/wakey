use macaddr::MacAddr;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::net::IpAddr;

use crate::parse::mac;

/// Concrete Wake-on-LAN destination fields.
///
/// A fully usable target needs both an IP address and a MAC address.
#[skip_serializing_none]
#[derive(Debug, Serialize, Deserialize, Clone, Copy, Hash, PartialEq, Eq)]
pub struct WakeTarget {
    #[serde(default)]
    pub ip: Option<IpAddr>,
    #[serde(default, with = "mac::option_mac")]
    pub mac: Option<MacAddr>,
}

impl WakeTarget {
    /// Return whether this target has both fields needed to send WoL.
    pub const fn is_complete(&self) -> bool {
        matches!(
            self,
            Self {
                ip: Some(_),
                mac: Some(_)
            }
        )
    }
}

/// Result of waking one or more targets.
#[derive(Debug, Serialize, Clone)]
pub struct WakeResult {
    pub result: Vec<WakeTargetResult>,
}

/// Per-target wake result row.
#[skip_serializing_none]
#[derive(Debug, Serialize, Clone, Copy)]
pub struct WakeTargetResult {
    #[serde(flatten)]
    pub target: WakeTarget,
    pub status: WakeStatus,
}

/// Outcome of trying to wake one target.
#[derive(Debug, Serialize, Clone, Copy, Hash, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WakeStatus {
    Succeed,
    NonexistentAddress,
    WrongSize,
    Incomplete,
}

impl WakeTargetResult {
    /// Construct an incomplete result for a target missing required fields.
    pub const fn incomplete(target: WakeTarget) -> Self {
        Self {
            target,
            status: WakeStatus::Incomplete,
        }
    }
}
