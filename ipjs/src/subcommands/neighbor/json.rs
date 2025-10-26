//! idk what to put here

use std::{iter, net::IpAddr};

use anyhow::bail;

use super::{NUDState, NeighborItem};

pub async fn get(
    ip: Option<IpAddr>,
    dev: Option<&str>,
    nud: &[NUDState],
) -> anyhow::Result<Vec<NeighborItem>> {
    let output = tokio::process::Command::new("ip")
        .args(
            (["-j", "neigh", "show"].iter().map(ToString::to_string))
                .chain(ip.map(|ip| ip.to_canonical().to_string()))
                .chain(dev.map(ToString::to_string))
                .chain(nud.iter().map(|f| f.to_string())),
        )
        .output()
        .await?;

    bail!("no")
}
