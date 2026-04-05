use macaddr::MacAddr;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::net::IpAddr;

use crate::model::NeighborState;
use crate::parse::mac;

#[derive(Debug, Clone, Serialize)]
pub struct DhcpLease {
    pub expires_epoch: u64,
    pub ip: IpAddr,
    #[serde(with = "mac")]
    pub mac: MacAddr,
    pub name: Option<String>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize)]
pub struct DhcpLeaseWithState {
    #[serde(flatten)]
    pub lease_line: DhcpLease,
    pub nud_state: Option<NeighborState>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct LeaseQuery {
    pub include_state: bool,
}
