use anyhow::{Context, Result};
use futures::future::try_join_all;
use std::collections::HashSet;
use std::net::IpAddr;
use wakey_core::{DeviceQuery, NeighborEntry, NeighborState, QueryInput, parse};

use lda_ipjs::subcommands::neighbor;

pub async fn get_ips(machine_name: &str) -> Result<impl Iterator<Item = IpAddr>> {
    Ok(tokio::net::lookup_host((machine_name, 0))
        .await
        .with_context(|| format!("DNS resolve failed for {machine_name}"))?
        .map(|c| c.ip()))
}

pub async fn get_neighbors(
    machine_names: &[impl AsRef<str>],
    ips: &[IpAddr],
    devs: &[impl AsRef<str>],
    state: &[NeighborState],
    macs: &[macaddr::MacAddr],
) -> Result<Vec<NeighborEntry>> {
    let resolved_ips: HashSet<IpAddr> = if !machine_names.is_empty() {
        try_join_all(machine_names.iter().map(|n| get_ips(n.as_ref())))
            .await?
            .into_iter()
            .flatten()
            .collect()
    } else {
        HashSet::new()
    };

    let ip_filter: Vec<IpAddr> = if ips.is_empty() && resolved_ips.is_empty() {
        vec![]
    } else if ips.is_empty() {
        resolved_ips.into_iter().collect()
    } else if resolved_ips.is_empty() {
        ips.iter().map(|ip| ip.to_canonical()).collect()
    } else {
        ips.iter()
            .map(|ip| ip.to_canonical())
            .filter(|ip| resolved_ips.contains(ip))
            .collect()
    };

    let nud_filter: Vec<neighbor::NUDState> = state.iter().copied().map(to_ipjs_state).collect();
    let dev_strs: Vec<&str> = devs.iter().map(AsRef::as_ref).collect();

    let results = neighbor::nl::get(&ip_filter, &dev_strs, &nud_filter, macs)
        .await
        .context("rtnetlink failed")?
        .into_iter()
        .map(map_neighbor_item)
        .collect();

    Ok(results)
}

pub async fn query_status(query: &DeviceQuery) -> Result<Vec<NeighborEntry>> {
    get_neighbors(
        query.name.as_slice(),
        &query.filter.ips,
        &query.filter.devs,
        &query.filter.nuds,
        &query.filter.macs,
    )
    .await
}

pub async fn list_devs() -> HashSet<String> {
    fn get_dev() -> HashSet<String> {
        let mut devs: HashSet<String> = HashSet::new();
        if let Ok(rd) = std::fs::read_dir("/sys/class/net") {
            for e in rd.flatten() {
                if e.file_type()
                    .map(|ft| {
                        if ft.is_symlink() {
                            std::fs::metadata(e.path())
                                .map(|m| m.is_dir())
                                .unwrap_or(false)
                        } else {
                            ft.is_dir()
                        }
                    })
                    .unwrap_or(false)
                    && let Ok(name) = e.file_name().into_string()
                    && name != "lo"
                    && !name.is_empty()
                {
                    devs.insert(name);
                }
            }
        } else if let Ok(txt) = std::fs::read_to_string("/proc/net/dev") {
            for line in txt.lines().skip(2) {
                if let Some((name, _rest)) = line.split_once(':') {
                    let n = name.trim().to_string();
                    if n != "lo" && !n.is_empty() {
                        devs.insert(n);
                    }
                }
            }
        }
        devs
    }

    tokio::task::spawn_blocking(get_dev)
        .await
        .unwrap_or_default()
}

pub async fn devs_sorted() -> Vec<String> {
    let mut v: Vec<String> = list_devs().await.into_iter().collect();
    v.sort();
    v
}

pub async fn has_dev(name: &str) -> bool {
    list_devs().await.contains(name)
}

pub async fn classify_query(q: String) -> QueryInput {
    let s = parse::extract_host(&q);
    if let Some(ip) = parse::parse_numeric_ipv4(s).or_else(|| s.parse::<IpAddr>().ok()) {
        return QueryInput::Ip(ip);
    }
    if let Ok(mac) = s.parse::<macaddr::MacAddr>() {
        return QueryInput::Mac(mac);
    }
    if let Ok(state) = s.parse::<NeighborState>() {
        return QueryInput::Nud(state);
    }
    if has_dev(s).await {
        return QueryInput::Dev(s.to_string());
    }
    QueryInput::Name(s.to_string())
}

fn to_ipjs_state(value: NeighborState) -> lda_ipjs::subcommands::neighbor::NUDState {
    match value {
        NeighborState::Permanent => lda_ipjs::subcommands::neighbor::NUDState::Permanent,
        NeighborState::Noarp => lda_ipjs::subcommands::neighbor::NUDState::Noarp,
        NeighborState::Reachable => lda_ipjs::subcommands::neighbor::NUDState::Reachable,
        NeighborState::Stale => lda_ipjs::subcommands::neighbor::NUDState::Stale,
        NeighborState::None => lda_ipjs::subcommands::neighbor::NUDState::None,
        NeighborState::Incomplete => lda_ipjs::subcommands::neighbor::NUDState::Incomplete,
        NeighborState::Delay => lda_ipjs::subcommands::neighbor::NUDState::Delay,
        NeighborState::Probe => lda_ipjs::subcommands::neighbor::NUDState::Probe,
        NeighborState::Failed => lda_ipjs::subcommands::neighbor::NUDState::Failed,
    }
}

fn from_ipjs_state(value: lda_ipjs::subcommands::neighbor::NUDState) -> NeighborState {
    match value {
        lda_ipjs::subcommands::neighbor::NUDState::Permanent => NeighborState::Permanent,
        lda_ipjs::subcommands::neighbor::NUDState::Noarp => NeighborState::Noarp,
        lda_ipjs::subcommands::neighbor::NUDState::Reachable => NeighborState::Reachable,
        lda_ipjs::subcommands::neighbor::NUDState::Stale => NeighborState::Stale,
        lda_ipjs::subcommands::neighbor::NUDState::None => NeighborState::None,
        lda_ipjs::subcommands::neighbor::NUDState::Incomplete => NeighborState::Incomplete,
        lda_ipjs::subcommands::neighbor::NUDState::Delay => NeighborState::Delay,
        lda_ipjs::subcommands::neighbor::NUDState::Probe => NeighborState::Probe,
        lda_ipjs::subcommands::neighbor::NUDState::Failed => NeighborState::Failed,
        lda_ipjs::subcommands::neighbor::NUDState::Other(_) => NeighborState::None,
    }
}

fn map_neighbor_item(
    lda_ipjs::subcommands::neighbor::NeighborItem {
        ip,
        dev,
        mac,
        state,
    }: lda_ipjs::subcommands::neighbor::NeighborItem,
) -> NeighborEntry {
    NeighborEntry {
        ip,
        dev,
        mac,
        state: state
            .into_iter()
            .map(from_ipjs_state)
            .max()
            .unwrap_or(NeighborState::None),
    }
}
