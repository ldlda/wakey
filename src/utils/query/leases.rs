use crate::arpparse::NUDState;
use crate::dhcpparse::DhcpLeaseLine;
use crate::utils::parse::serialize_mac;
use serde_with::skip_serializing_none;
use std::net::IpAddr;

#[skip_serializing_none]
#[derive(Debug, Clone, serde::Serialize)]
pub struct DhcpLeaseOut {
    pub expires_epoch: u64,
    pub ip: IpAddr,
    #[serde(serialize_with = "serialize_mac")]
    pub mac: macaddr::MacAddr,
    pub name: Option<String>,
    pub nud_state: Option<NUDState>,
    pub rank: Option<u8>,
}

/// Enrich DHCP leases with NUD state and rank using get_macs
pub async fn enrich_leases_with_nud_state(leases: Vec<DhcpLeaseLine>) -> Vec<DhcpLeaseOut> {
    use crate::utils::query::macs::get_macs;
    let ips: Vec<IpAddr> = leases.iter().map(|l| l.ip).collect();
    let mut map: std::collections::HashMap<IpAddr, (NUDState, u8)> =
        std::collections::HashMap::new();
    if let Ok(rows) = get_macs(None, Some(&ips), None, None).await {
        for row in rows {
            let state = row.state;
            let r = state.rank();
            map.entry(row.ip)
                .and_modify(|e| {
                    if r > e.1 {
                        *e = (state, r)
                    }
                })
                .or_insert((state, r));
        }
    }
    leases
        .into_iter()
        .map(|l| DhcpLeaseOut {
            expires_epoch: l.expires_epoch,
            ip: l.ip,
            mac: l.mac,
            name: l.name,
            nud_state: map.get(&l.ip).map(|(s, _)| *s),
            rank: map.get(&l.ip).map(|(_, r)| *r),
        })
        .collect()
}
