//! idk what to put here

use std::net::IpAddr;

use anyhow::bail;

use super::{NUDState, NeighborItem};

// loose translation of [wakey::utils::query::macs::get_mac]
// i think ill write tokio::process every time tho (for this if let thing) because iterate through all ts youll have to as str and all the hooplas.
// it all turns to live osstr tho so ts just for my own sanity
// thiserror? anyhow
pub async fn get(
    ip: Option<IpAddr>,
    dev: Option<&str>,
    nud: &[NUDState],
) -> anyhow::Result<Vec<NeighborItem>> {
    let mut cmd = tokio::process::Command::new("ip");
    cmd.args(["-j", "neigh", "show"]);

    if let Some(ip) = ip {
        cmd.arg(ip.to_canonical().to_string());
    };

    if let Some(dev) = dev {
        cmd.arg("dev");
        cmd.arg(dev);
    }
    for nud in nud {
        cmd.arg("nud");
        cmd.arg(nud.to_string());
    }

    let output = cmd.output().await?;

    if !output.status.success() {
        bail!(String::from_utf8_lossy(&output.stderr).into_owned())
    } else {
        Ok(serde_json::from_slice(&output.stdout)?)
    }
}
