//! this is purely experimental. im not doing ts no mo

// hallo
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

// dont you love https://github.com/rust-netlink/rtnetlink/blob/main/examples/get_neighbours.rs
pub async fn get(
    ip: Option<IpAddr>,
    dev: Option<&str>,
    nud: &[NUDState],
) -> anyhow::Result<Vec<NeighborItem>> {
    let (gip, gdev, gnud) = (ip, dev, nud);
    let (conn, handle, _) = rtnetlink::new_connection()?;
    tokio::spawn(conn); // every time?
    let mut neighbor_data = handle.neighbours().get().execute();
    let nudset: HashSet<&NUDState> = HashSet::from_iter(gnud);
    let mut ball: HashMap<u32, String> = HashMap::new();
    let mut result = vec![];
    'big: while let Some(neighbour_message_item) = neighbor_data.try_next().await? {
        // Filter by address family
        if !matches!(
            neighbour_message_item.header.family,
            AddressFamily::Inet | AddressFamily::Inet6
        ) || matches!(neighbour_message_item.header.state, NeighbourState::Noarp)
        // copilot says this to match ip -j n s
        {
            continue 'big;
        }

        let state = vec![
            neighbour_message_item
                .header
                .state
                .try_into()
                .unwrap_or_default(),
        ]; // ONE ITEM. why tf ts design json.
        let mut ip = None;
        let mut mac = None;

        for neigh_attr in neighbour_message_item.attributes {
            match neigh_attr {
                NeighbourAttribute::Destination(neighbour_address) => match neighbour_address {
                    NeighbourAddress::Inet(ipv4_addr) => ip = Some(ipv4_addr.into()),
                    NeighbourAddress::Inet6(ipv6_addr) => ip = Some(ipv6_addr.into()),
                    _ => continue 'big,
                },
                NeighbourAttribute::LinkLocalAddress(items) => {
                    mac = match items.len() {
                        6 => items.first_chunk::<6>().map(|&e| MacAddr::from(e)),
                        8 => items.first_chunk::<8>().map(|&e| MacAddr::from(e)),
                        _ => continue 'big,
                    }
                }
                _ => continue,
            }
        }

        // exquisite
        let dev = if let Some(cached) = ball.get(&neighbour_message_item.header.ifindex) {
            Some(cached.clone())
        } else {
            // Query and cache
            let name = handle
                .link()
                .get()
                .match_index(neighbour_message_item.header.ifindex)
                .execute()
                .try_next()
                .await?
                .and_then(|a| {
                    a.attributes.into_iter().find_map(|attr| match attr {
                        LinkAttribute::IfName(name) => Some(name),
                        _ => None,
                    })
                });

            if let Some(ref n) = name {
                ball.insert(neighbour_message_item.header.ifindex, n.clone());
            }
            name
        };

        let (Some(ip), Some(dev)) = (ip, dev) else {
            continue 'big;
        };

        {
            // low block
            if let Some(fip) = gip
                && fip != ip
            {
                continue 'big;
            }
            if let Some(fdev) = gdev
                && dev != fdev
            {
                continue 'big;
            }
            if !nudset.is_empty() && !nudset.contains(&state[0]) {
                continue 'big;
            };
        }

        result.push(NeighborItem {
            ip,
            dev: Some(dev),
            mac,
            state,
        });
    }
    Ok(result) // now i need another pass to filter out the uh.
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
            _ => Err(u16::MAX), // idk
        }
    }

    type Error = u16;
}
