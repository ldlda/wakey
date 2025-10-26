// hallo
use std::net::IpAddr;

use futures::TryStreamExt;
use macaddr::MacAddr;
use rtnetlink::packet_route::{
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
    let (conn, handle, _) = rtnetlink::new_connection()?;
    tokio::spawn(conn); // every time?
    let mut neighbor_data = handle.neighbours().get().execute(); // can i change header with message_mut? what even is header.
    let mut result = vec![];
    'big: while let Some(neighbour_message_item) = neighbor_data.try_next().await? {
        let state = vec![
            neighbour_message_item
                .header
                .state
                .try_into()
                .unwrap_or_default(),
        ];
        let mut dst = None;
        let mut lladdr = None;

        // a hassle and a half to get the name
        let dev = handle
            .link()
            .get()
            .match_index(neighbour_message_item.header.ifindex)
            .execute()
            .try_next()
            .await?
            .and_then(|a| {
                for link_attr in a.attributes {
                    match link_attr {
                        LinkAttribute::IfName(name) => return Some(name),
                        _ => continue,
                    }
                }
                None
            });
        for neigh_attr in neighbour_message_item.attributes {
            match neigh_attr {
                NeighbourAttribute::Destination(neighbour_address) => match neighbour_address {
                    NeighbourAddress::Inet(ipv4_addr) => dst = Some(ipv4_addr.into()),
                    NeighbourAddress::Inet6(ipv6_addr) => dst = Some(ipv6_addr.into()),
                    _ => continue 'big,
                },
                NeighbourAttribute::LinkLocalAddress(items) => {
                    lladdr = match items.len() {
                        6 => items.first_chunk::<6>().map(|&e| MacAddr::from(e)),
                        8 => items.first_chunk::<8>().map(|&e| MacAddr::from(e)),
                        _ => continue 'big,
                    }
                }
                _ => continue,
            }
        }
        let Some((dst, dev)) = dst.zip(dev) else {
            continue;
        };
        result.push(NeighborItem {
            dst,
            dev,
            lladdr,
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
            NeighbourState::Other(e) => Err(e),
            _ => Err(u16::MAX), // idk
        }
    }

    type Error = u16;
}
