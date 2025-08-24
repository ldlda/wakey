use std::{collections::HashSet, net::IpAddr};

use macaddr::MacAddr;
use tokio::io;

use crate::{
    arpparse::{self, IpNeighLine, NUDState},
    utils::{cmd::exec_command, get_ips},
};

pub async fn get_macs_2_1(machine_name: &str) -> io::Result<HashSet<(IpAddr, MacAddr, NUDState)>> {
    Ok(get_macs_1(machine_name)
        .await?
        .into_iter()
        .filter_map(
            |IpNeighLine {
                 ip,
                 dev: _,
                 mac,
                 state,
             }| mac.map(|mac| (ip, mac, state)),
        )
        .collect())
}
pub async fn get_macs_2_mac(machine_name: &str) -> io::Result<HashSet<MacAddr>> {
    Ok(get_macs_1(machine_name)
        .await?
        .into_iter()
        .filter_map(
            |IpNeighLine {
                 ip: _,
                 dev: _,
                 mac,
                 state: _,
             }| mac,
        )
        .collect())
}

pub async fn get_macs_1(machine_name: &str) -> io::Result<Vec<arpparse::IpNeighLine>> {
    let dev = "br-lan";
    let ips = get_ips(machine_name).await?;
    let futures = ips.iter().map(|ip| {
        let ip = ip.to_canonical();
        async move {
            let o =
                exec_command("ip", ["neigh", "show", "to", &ip.to_string(), "dev", dev]).await?;
            if !o.status.success() {
                return Err(io::Error::other(format!(
                    "`ip neigh` failed for {ip} (status: {st}): {err}",
                    st = o.status,
                    err = String::from_utf8_lossy(&o.stderr),
                )));
            };
            Ok(String::from_utf8_lossy(&o.stdout)
                .lines()
                .flat_map(arpparse::parse_ip_neigh_line)
                // .map(IpNeighLine::with_dev(dev)) // this could be after flatmap up there
                .collect::<Vec<_>>())
        }
    });
    let res = futures::future::try_join_all(futures).await?; //  async move block errs.
    Ok(res
        .into_iter()
        
        .flatten() /* resolve double vec */
        // .flatten() /* drop parse errors (flat_map cleared) */
        .collect())
}

pub async fn get_macs(machine_name: Option<&str>, ips: Option<impl IntoIterator<Item = IpAddr>>, dev: Option<&str>, state: Option<NUDState>) -> io::Result<Vec<IpNeighLine>> {
    todo!()
}