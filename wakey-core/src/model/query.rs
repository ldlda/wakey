use macaddr::MacAddr;
use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, OneOrMany, serde_as};
use std::net::IpAddr;

use crate::model::NeighborState;

#[derive(Debug, Default, Clone, Hash, Deserialize, Serialize)]
pub struct DeviceQuery {
    pub name: Option<String>,
    #[serde(flatten)]
    pub filter: DeviceFilters,
}

#[serde_as]
#[derive(Debug, Default, Clone, Hash, Serialize, Deserialize)]
pub struct DeviceFilters {
    #[serde_as(as = "OneOrMany<_>")]
    #[serde(default)]
    pub ips: Vec<IpAddr>,
    #[serde_as(as = "OneOrMany<_>")]
    #[serde(default)]
    pub devs: Vec<String>,
    #[serde_as(as = "OneOrMany<_>")]
    #[serde(default)]
    pub nuds: Vec<NeighborState>,
    #[serde_as(as = "OneOrMany<DisplayFromStr>")]
    #[serde(default)]
    pub macs: Vec<MacAddr>,
}

#[derive(Debug, Default, Clone, Hash, Deserialize)]
pub struct NamePath {
    pub name: String,
}

#[derive(Debug)]
pub enum QueryInput {
    Ip(IpAddr),
    Mac(MacAddr),
    Dev(String),
    Nud(NeighborState),
    Name(String),
}

#[derive(Debug, Clone)]
pub enum Query {
    Text(String),
    Ip(IpAddr),
    Mac(MacAddr),
    Interface(String),
    NeighborState(NeighborState),
}
