//! idk what to put here

use std::{io, net::IpAddr, process::Output};

use anyhow::{Context, bail};

use super::{NUDState, NeighborItem};

// loose translation of [wakey::utils::query::macs::get_mac]
// i think ill write tokio::process every time tho (for this if let thing) because iterate through all ts youll have to as str and all the hooplas.
// it all turns to live osstr tho so ts just for my own sanity
// thiserror? anyhow

// what is vro sayin
// ahh. instead of using [wakey::utils::cmd::exec_command] which is ass we jus write everything out. so i dont have to .as_str() so often.

pub async fn get(
    ip: Option<IpAddr>,
    dev: Option<&str>,
    nud: &[NUDState],
) -> anyhow::Result<Vec<NeighborItem>> {
    let output = _get(ip, dev, nud).await.context("Can not run command")?;

    if !output.status.success() {
        bail!(String::from_utf8_lossy(&output.stderr).into_owned())
    } else {
        serde_json::from_slice(&output.stdout).context("Deserialize failed")
    }
}

pub async fn _get(ip: Option<IpAddr>, dev: Option<&str>, nud: &[NUDState]) -> io::Result<Output> {
    let mut cmd = tokio::process::Command::new("ip");
    cmd.args(["-j", "neigh", "show"]);

    if let Some(ip) = ip {
        cmd.arg(ip.to_canonical().to_string());
    };

    if let Some(dev) = dev {
        cmd.args(["dev", dev]);
    }
    for nud in nud {
        cmd.arg("nud");
        cmd.arg(nud.to_string());
    }

    cmd.output().await
}
