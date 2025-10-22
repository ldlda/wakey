//! ts
//! 
//! deals with both ip a (addroutput) and ip l (commonoutput)
//! 
//! lowk why its free but its indirection and its ass

use macaddr::{MacAddr, MacAddr6};
use serde::{Deserialize, Serialize};

/// i dont include what i dont know about (almost all ts)
#[derive(Debug, Serialize, Deserialize)]
struct CommmonOutput {
    ifindex: u32,
    ifname: String,
    /// i imagine UP or DOWN, unknown
    operstate: String,
    // 6 has a serde and the enum doesnt? why.
    address: Option<MacAddr6>,
}

#[derive(Serialize, Debug, Deserialize)]
struct AddrOutput {
    #[serde(flatten)]
    c: CommmonOutput,
    addr_info: Vec<AddrInfo>,
}

// i be copying
// Raw JSON shape from ip -j -4 address show
#[derive(Debug, Deserialize, Serialize)]
struct AddrInfo {
    family: Option<String>,

    local: Option<String>,
    prefixlen: Option<u8>,

    broadcast: Option<String>,

    scope: Option<String>,
    label: Option<String>,
    // many more exist; we only take what we need
}
