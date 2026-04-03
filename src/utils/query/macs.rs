use anyhow::Result;
use std::net::IpAddr;

use crate::arpparse::{IpNeighLine, NUDState};

pub async fn get_ips(machine_name: &str) -> Result<impl Iterator<Item = IpAddr>> {
    wakey_linux::devices::get_ips(machine_name).await
}

pub async fn get_macs(
    machine_names: &[impl AsRef<str>],
    ips: &[IpAddr],
    devs: &[impl AsRef<str>],
    state: &[NUDState],
    macs: &[macaddr::MacAddr],
) -> Result<Vec<IpNeighLine>> {
    wakey_linux::devices::get_neighbors(machine_names, ips, devs, state, macs).await
}

pub async fn get_mac(
    ip: Option<IpAddr>,
    dev: Option<&str>,
    state: &[NUDState],
) -> Result<Vec<IpNeighLine>> {
    let ips: Vec<IpAddr> = ip.into_iter().collect();
    let devs: Vec<&str> = dev.into_iter().collect();
    get_macs(&[] as &[&str], &ips, &devs, state, &[]).await
}
