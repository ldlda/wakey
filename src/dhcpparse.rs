use macaddr::MacAddr;
use serde::Serializer;
use std::io::{self, ErrorKind};
use std::net::IpAddr;

/// A single line from /tmp/dhcp.leases
#[derive(Debug, Clone, serde::Serialize)]
pub struct DhcpLeaseLine {
    /// Epoch seconds when the lease expires
    pub expires_epoch: u64,
    pub ip: IpAddr,
    #[serde(serialize_with = "ser_mac")]
    pub mac: MacAddr,
    pub name: Option<String>,
}

fn ser_mac<S: Serializer>(m: &MacAddr, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&m.to_string())
}

/// Parse one line of /tmp/dhcp.leases
pub fn parse_dhcp_lease_line(line: &str) -> Option<DhcpLeaseLine> {
    let mut c = line.split_whitespace();
    let expires_epoch: u64 = c.next()?.parse().ok()?;
    let mac = c.next()?.parse().ok()?;
    let ip = c.next()?.parse().ok()?;
    let name = c.next().filter(|c| *c != "*").map(str::to_string);
    // ignore any remaining columns (e.g., client-id)
    Some(DhcpLeaseLine {
        expires_epoch,
        ip,
        mac,
        name,
    })
}

/// Read all leases from /tmp/dhcp.leases (simple and fast)
pub async fn read_dhcp_leases() -> io::Result<Vec<DhcpLeaseLine>> {
    match tokio::fs::read_to_string("/tmp/dhcp.leases").await {
        Ok(file) => Ok(file.lines().filter_map(parse_dhcp_lease_line).collect()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}
