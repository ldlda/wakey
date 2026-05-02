use macaddr::MacAddr;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::net::IpAddr;

use crate::model::NeighborState;
use crate::parse::mac;

/// One parsed DHCP lease row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhcpLease {
    pub expires_epoch: u64,
    pub ip: IpAddr,
    #[serde(with = "mac")]
    pub mac: MacAddr,
    pub name: Option<String>,
}

/// DHCP lease row plus optional current neighbor-state enrichment.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhcpLeaseWithState {
    #[serde(flatten)]
    pub lease_line: DhcpLease,
    pub nud_state: Option<NeighborState>,
}

/// Legacy options for lease retrieval from the service layer.
///
/// Prefer inventory for device state; lease rows are now best treated as a raw
/// dnsmasq snapshot.
#[deprecated(note = "prefer get_leases() or inventory; neighbor state belongs in inventory")]
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct LeaseQuery {
    #[deprecated(note = "prefer inventory for device state")]
    pub include_state: bool,
}
