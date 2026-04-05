//! i said i aint doing ts no more why am i still here
#![cfg(unix)]
use std::collections::BTreeMap;

use futures::TryStreamExt;
use rtnetlink::Handle;
use rtnetlink::packet_route::{AddressFamily, address::AddressAttribute};

use crate::subcommands::{
    address::{AddrInfo, AddrOutput, AddressFamily as IpAddressFamily, InterfaceCidr},
    link,
};

pub async fn get(dev: Option<&str>) -> anyhow::Result<Vec<AddrOutput>> {
    let (conn, handle, _) = rtnetlink::new_connection()?;
    tokio::spawn(conn);

    get_with_handle(&handle, dev).await
}

pub async fn get_with_handle(
    handle: &Handle,
    dev: Option<&str>,
) -> anyhow::Result<Vec<AddrOutput>> {
    let links = link::nl::get_with_handle(handle, dev).await?;
    let link_by_index: BTreeMap<u32, crate::subcommands::link::LinkOutput> =
        links.into_iter().map(|link| (link.ifindex, link)).collect();

    let mut address = handle.address().get();
    if let Some(dev) = dev {
        if let Some(index) = link_by_index
            .values()
            .find(|link| link.ifname == dev)
            .map(|link| link.ifindex)
        {
            address = address.set_link_index_filter(index);
        } else {
            return Ok(Vec::new());
        }
    }

    let mut stream = address.execute();
    let mut addr_info_by_index: BTreeMap<u32, Vec<AddrInfo>> = BTreeMap::new();

    while let Some(msg) = stream.try_next().await? {
        if !matches!(
            msg.header.family,
            AddressFamily::Inet | AddressFamily::Inet6
        ) {
            continue;
        }

        let family = match msg.header.family {
            AddressFamily::Inet => Some(IpAddressFamily::Inet),
            AddressFamily::Inet6 => Some(IpAddressFamily::Inet6),
            _ => None,
        };

        let mut local = None;
        let mut prefixlen = Some(msg.header.prefix_len);
        let mut broadcast = None;
        let mut scope = Some(format!("{:?}", msg.header.scope).to_lowercase());
        let mut label = None;

        for attr in msg.attributes {
            match attr {
                AddressAttribute::Address(addr) | AddressAttribute::Local(addr) => {
                    if local.is_none() {
                        local = addr.to_string().parse().ok();
                    }
                }
                AddressAttribute::Broadcast(addr) => {
                    broadcast = addr.to_string().parse().ok();
                }
                AddressAttribute::Label(name) => {
                    label = Some(name);
                }
                _ => {}
            }
        }

        addr_info_by_index
            .entry(msg.header.index)
            .or_default()
            .push(AddrInfo {
                family,
                cidr: InterfaceCidr {
                    local,
                    prefixlen: prefixlen.take(),
                },
                broadcast,
                scope: scope.take(),
                label,
            });
    }

    let out = link_by_index
        .into_values()
        .map(|link| AddrOutput {
            ifindex: link.ifindex,
            ifname: link.ifname,
            operstate: link.operstate.unwrap_or(link::OperState::Unknown),
            address: link.address,
            addr_info: addr_info_by_index.remove(&link.ifindex).unwrap_or_default(),
        })
        .collect();

    Ok(out)
}
