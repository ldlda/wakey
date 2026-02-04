use lda_ipjs::subcommands::neighbor;
use macaddr::MacAddr;

use crate::arpparse::{IpNeighLine, NUDState};
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::net::IpAddr;

pub async fn get_ips(machine_name: &str) -> Result<impl Iterator<Item = IpAddr>> {
    Ok(tokio::net::lookup_host((machine_name, 0))
        .await
        .with_context(|| format!("DNS resolve failed for {machine_name}"))?
        .map(|c| c.ip()))
}

/// Query neighbor table with multi-filters. Empty slice = no filter.
pub async fn get_macs(
    machine_names: &[impl AsRef<str>],
    ips: &[IpAddr],
    devs: &[impl AsRef<str>],
    state: &[NUDState],
    macs: &[MacAddr],
) -> Result<Vec<IpNeighLine>> {
    // Resolve machine names to IPs
    let resolved_ips: HashSet<IpAddr> = if !machine_names.is_empty() {
        futures::future::try_join_all(machine_names.iter().map(|n| get_ips(n.as_ref())))
            .await?
            .into_iter()
            .flatten()
            .collect()
    } else {
        HashSet::new()
    };

    // Merge provided IPs with resolved IPs
    let ip_filter: Vec<IpAddr> = if ips.is_empty() && resolved_ips.is_empty() {
        vec![]
    } else if ips.is_empty() {
        resolved_ips.into_iter().collect()
    } else if resolved_ips.is_empty() {
        ips.iter().map(|ip| ip.to_canonical()).collect()
    } else {
        // Intersection: only IPs that appear in both
        ips.iter()
            .map(|ip| ip.to_canonical())
            .filter(|ip| resolved_ips.contains(ip))
            .collect()
    };

    // Convert state filter
    let nud_filter: Vec<neighbor::NUDState> = state.iter().copied().map(Into::into).collect();

    // Convert devs to &str for nl::get
    let dev_strs: Vec<&str> = devs.iter().map(AsRef::as_ref).collect();

    // Single rtnetlink call with all filters
    let results: Vec<IpNeighLine> = neighbor::nl::get(&ip_filter, &dev_strs, &nud_filter, macs)
        .await
        .context("rtnetlink failed")?
        .into_iter()
        .map(Into::into)
        .collect();

    Ok(results)
}

/// Legacy single-filter wrapper. Use get_macs for multi-filter.
#[allow(dead_code)]
pub async fn get_mac(
    ip: Option<IpAddr>,
    dev: Option<&str>,
    state: &[NUDState],
) -> Result<Vec<IpNeighLine>> {
    let ips: Vec<IpAddr> = ip.into_iter().collect();
    let devs: Vec<&str> = dev.into_iter().collect();
    let nud: Vec<neighbor::NUDState> = state.iter().copied().map(Into::into).collect();

    Ok(neighbor::nl::get(&ips, &devs, &nud, &[])
        .await
        .context("rtnetlink failed")?
        .into_iter()
        .map(Into::into)
        .collect())
}
