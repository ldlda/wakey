pub mod dev {
    pub use wakey_linux::devices::devs_sorted;
}
pub mod leases {
    pub use wakey_linux::dhcp::enrich_leases_with_nud_state;
}

pub mod parser {
    pub use wakey_core::QueryInput as QueryType;

    pub async fn parse_query(q: String) -> QueryType {
        wakey_linux::devices::classify_query(q).await
    }
}

pub use leases::*;
pub use macs::*;

pub mod macs {
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
}
