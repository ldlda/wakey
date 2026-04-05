use anyhow::{Context, Result};
use macaddr::MacAddr;
use std::net::IpAddr;
use wakey_core::{WakeResult, WakeTarget};

use crate::service::interfaces::get_interface_summaries;
use crate::service::inventory::resolve_devices;

pub async fn wake_targets(targets: Vec<WakeTarget>) -> Result<WakeResult> {
    let result = wakey_linux::wake::wake_many(targets)
        .await
        .context("failed to send wake packets")?;
    Ok(WakeResult { result })
}

pub async fn wake_from_query(input: impl Into<String>) -> Result<WakeResult> {
    let targets = resolve_wake_targets(input).await?;
    wake_targets(targets).await
}

pub async fn broadcast_wake_targets(mac: MacAddr) -> Result<Vec<WakeTarget>> {
    Ok(get_interface_summaries()
        .await?
        .into_iter()
        .flat_map(|iface| iface.addrs.into_iter())
        .filter_map(|addr| addr.broadcast)
        .map(|ip| WakeTarget {
            ip: Some(IpAddr::V4(ip)),
            mac: Some(mac),
        })
        .collect())
}

pub async fn wake_explicit(mac: MacAddr, ip: Option<IpAddr>) -> Result<WakeResult> {
    let targets = match ip {
        Some(ip) => vec![WakeTarget {
            ip: Some(ip),
            mac: Some(mac),
        }],
        None => broadcast_wake_targets(mac).await?,
    };
    wake_targets(targets).await
}

pub async fn resolve_wake_targets(input: impl Into<String>) -> Result<Vec<WakeTarget>> {
    let devices = resolve_devices(input).await?;
    Ok(devices
        .into_iter()
        .flat_map(|device| {
            let mac = device.macs.first().copied();
            device
                .ips
                .into_iter()
                .map(move |ip| WakeTarget { ip: Some(ip), mac })
        })
        .collect())
}
