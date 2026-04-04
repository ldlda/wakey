//! rtnetlink-based neighbor table query. One syscall, filter in userspace.
#![cfg(unix)]
use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
};

use futures::TryStreamExt;
use macaddr::MacAddr;
use rtnetlink::packet_route::{
    AddressFamily,
    link::LinkAttribute,
    neighbour::{NeighbourAddress, NeighbourAttribute, NeighbourState},
};

use super::{NUDState, NeighborItem};

/// Fetch neighbors via rtnetlink. Empty slice = no filter (match all).
/// Non-empty slice = match ANY in the set.
// https://github.com/rust-netlink/rtnetlink/blob/main/examples/get_neighbours.rs
pub async fn get(
    ips: &[IpAddr],
    devs: &[impl AsRef<str>],
    nuds: &[NUDState],
    macs: &[MacAddr],
) -> anyhow::Result<Vec<NeighborItem>> {
    let (conn, handle, _) = rtnetlink::new_connection()?;
    tokio::spawn(conn);

    let mut neighbor_data = handle.neighbours().get().execute();

    // Build filter sets (empty = match all)
    let ip_set: HashSet<IpAddr> = ips.iter().copied().collect();
    let dev_set: HashSet<&str> = devs.iter().map(AsRef::as_ref).collect();
    let nud_set: HashSet<&NUDState> = nuds.iter().collect();
    let mac_set: HashSet<MacAddr> = macs.iter().copied().collect();

    // Cache ifindex -> name
    let mut ifname_cache: HashMap<u32, String> = HashMap::new();
    let mut result = vec![];

    'row: while let Some(msg) = neighbor_data.try_next().await? {
        // Only IPv4/IPv6, skip NOARP
        if !matches!(
            msg.header.family,
            AddressFamily::Inet | AddressFamily::Inet6
        ) || matches!(msg.header.state, NeighbourState::Noarp)
        {
            continue 'row;
        }

        let state: NUDState = msg.header.state.try_into().unwrap_or_default();
        let mut ip = None;
        let mut mac = None;

        for attr in msg.attributes {
            match attr {
                NeighbourAttribute::Destination(addr) => match addr {
                    NeighbourAddress::Inet(v4) => ip = Some(IpAddr::from(v4)),
                    NeighbourAddress::Inet6(v6) => ip = Some(IpAddr::from(v6)),
                    _ => continue 'row,
                },
                NeighbourAttribute::LinkLocalAddress(bytes) => {
                    mac = match bytes.len() {
                        6 => bytes.first_chunk::<6>().map(|&b| MacAddr::from(b)),
                        8 => bytes.first_chunk::<8>().map(|&b| MacAddr::from(b)),
                        _ => continue 'row,
                    }
                }
                _ => {}
            }
        }

        // Resolve ifindex -> name (cached)
        let dev = match ifname_cache.get(&msg.header.ifindex) {
            Some(name) => Some(name.clone()),
            None => {
                let name = handle
                    .link()
                    .get()
                    .match_index(msg.header.ifindex)
                    .execute()
                    .try_next()
                    .await?
                    .and_then(|link| {
                        link.attributes.into_iter().find_map(|a| match a {
                            LinkAttribute::IfName(n) => Some(n),
                            _ => None,
                        })
                    });
                if let Some(ref n) = name {
                    ifname_cache.insert(msg.header.ifindex, n.clone());
                }
                name
            }
        };

        let (Some(ip), Some(dev)) = (ip, dev) else {
            continue 'row;
        };

        // Apply filters (empty set = match all)
        if !ip_set.is_empty() && !ip_set.contains(&ip) {
            continue 'row;
        }
        if !dev_set.is_empty() && !dev_set.contains(dev.as_str()) {
            continue 'row;
        }
        if !nud_set.is_empty() && !nud_set.contains(&state) {
            continue 'row;
        }
        if !mac_set.is_empty() && !mac.is_some_and(|m| mac_set.contains(&m)) {
            continue 'row;
        }

        result.push(NeighborItem {
            ip,
            dev: Some(dev),
            mac,
            state: vec![state],
        });
    }

    Ok(result)
}

impl TryFrom<NeighbourState> for NUDState {
    fn try_from(value: NeighbourState) -> Result<Self, Self::Error> {
        match value {
            NeighbourState::Incomplete => Ok(Self::Incomplete),
            NeighbourState::Reachable => Ok(Self::Reachable),
            NeighbourState::Stale => Ok(Self::Stale),
            NeighbourState::Delay => Ok(Self::Delay),
            NeighbourState::Probe => Ok(Self::Probe),
            NeighbourState::Failed => Ok(Self::Failed),
            NeighbourState::Noarp => Ok(Self::Noarp),
            NeighbourState::Permanent => Ok(Self::Permanent),
            NeighbourState::None => Ok(Self::None),
            NeighbourState::Other(e) => Ok(Self::Other(e)),
            _ => Err(u16::MAX),
        }
    }

    type Error = u16;
}
