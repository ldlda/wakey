use macaddr::MacAddr;

use crate::arpparse::{IpNeighLine, NUDState};
use anyhow::{Context, Result};
use lda_ipjs::subcommands::neighbor as ipjs_neigh;
use std::collections::HashSet;
use std::net::IpAddr;

pub async fn get_ips(machine_name: &str) -> Result<impl Iterator<Item = IpAddr>> {
    Ok(tokio::net::lookup_host((machine_name, 0))
        .await
        .with_context(|| format!("DNS resolve failed for {machine_name}"))?
        .map(|c| c.ip()))
}

// good now
//
// Current logic: When filtering by exactly 1 dev/mac, exclude entries missing that field.
// This is because missing dev/mac usually means the entry is incomplete/transient.
//
// when there is only one MACs (getmac got some), the result will not have them fields.
// so there are three cases:
//
// 1. dont got nothing: take all of them (macset.is_empty())
// 2. exactly one: pre-filtered by ip, everything matches,
// devset.len() != 1 returns false, but then it works????
// OH THIS fuckass code i added it in the get_mac
// 3. devset.len() > 1. if none then absolutely not match,
// if some then check with the set; thats normal
pub async fn get_macs(
    machine_names: &[impl AsRef<str>],
    ips: &[IpAddr],
    devs: &[impl AsRef<str>],
    state: &[NUDState],
    macs: &[MacAddr],
) -> Result<Vec<IpNeighLine>> {
    let mut ip_set: HashSet<IpAddr> = ips.iter().map(|ip| ip.to_canonical()).collect();
    let ip_m: HashSet<IpAddr> =
        futures::future::try_join_all(machine_names.iter().map(|c| get_ips(c.as_ref())))
            .await?
            .into_iter()
            .flatten()
            .collect();
    let ip_all = if ip_set.is_empty() && ip_m.is_empty() {
        None
    } else if ip_set.is_empty() {
        Some(ip_m)
    } else if ip_m.is_empty() {
        Some(ip_set)
    } else {
        Some({
            ip_set.retain(|c| ip_m.contains(c)); // inline AHHH
            ip_set
        })
    };

    let opt_dev = if devs.len() > 1 {
        None
    } else {
        devs.iter().next().map(AsRef::as_ref)
    };

    let run_one = |to_ip: Option<IpAddr>| get_mac(to_ip, opt_dev, state);

    let mut ip_filtered = if let Some(something) = ip_all {
        if something.len() == 1 {
            run_one(something.into_iter().next()).await?
        } else {
            run_one(None)
                .await?
                .into_iter()
                .filter(|c| something.contains(&c.ip))
                .collect()
        }
    } else {
        run_one(None).await?
    };

    // Apply additional filters if any were provided
    if !devs.is_empty() || !macs.is_empty() || !state.is_empty() {
        let devset: HashSet<_> = devs.iter().map(AsRef::as_ref).collect();
        let macset: HashSet<_> = macs.iter().collect();

        ip_filtered.retain(|entry| {
            // Dev filter: if we're filtering by dev, entry must have a dev AND it must be in the set
            let dev_ok =
                devset.is_empty() || entry.dev.as_deref().is_some_and(|d| devset.contains(d));

            // MAC filter: if we're filtering by MAC, entry must have a MAC AND it must be in the set
            let mac_ok = macset.is_empty() || entry.mac.is_some_and(|m| macset.contains(&m));

            dev_ok && mac_ok
        })
    };
    Ok(ip_filtered)
}

/// the atomic get_macs. handle ONE thing only.
pub async fn get_mac(
    ip: Option<IpAddr>,
    dev: Option<&str>,
    state: &[NUDState],
) -> Result<Vec<IpNeighLine>> {
    let ipjs_states: Vec<ipjs_neigh::NUDState> = state.iter().copied().map(Into::into).collect();

    let items = ipjs_neigh::json::get(ip, dev, &ipjs_states)
        .await
        .context("Calling ip -j neigh failed")?;

    let lines = items
        .into_iter()
        .map(|item| IpNeighLine {
            ip: item.ip,
            dev: Some(item.dev),
            mac: item.mac,
            state: item
                .state
                .first()
                .copied()
                .map(Into::into)
                .unwrap_or(NUDState::None),
        })
        .collect();

    Ok(lines)
}

/*
/// get macs where you just run ip neigh then rust handles the filtering (faster than get mac)
pub async fn get_macs_rust(
    machine_names: Option<&str>,
    ips: Option<&[IpAddr]>,
    devs: Option<&[&str]>,
    states: Option<&[NUDState]>,
) -> Result<Vec<IpNeighLine>> {
    let mut ip_map: HashSet<IpAddr> = HashSet::new();
    let mut machine_map = HashSet::new();
    if let Some(ips) = ips {
        ip_map.extend(ips);
    }
    if let Some(m) = machine_names {
        machine_map.extend(get_ips(m).await.into_iter().flatten());
    }

    let real = match (ip_map.is_empty(), machine_map.is_empty()) {
        (true, true) => HashSet::new(),
        (true, false) => machine_map,
        (false, true) => ip_map,
        (false, false) => ip_map.intersection(&machine_map).copied().collect(),
    };

    Ok(vec![])
}

pub async fn _get_machines(m: &[&str]) -> Vec<IpAddr> {
    let futs = m.iter().map(|c| get_ips(c));
    futures::future::join_all(futs)
        .await
        .into_iter()
        .flat_map(|f| f.into_iter().flatten())
        .collect()
} */
