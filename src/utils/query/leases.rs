use crate::arpparse::NUDState;
use crate::dhcpparse::DhcpLeaseLine;
use crate::utils::query::get_macs;
use serde_with::skip_serializing_none;
use std::net::IpAddr;

#[skip_serializing_none]
#[derive(Debug, Clone, serde::Serialize)]
pub struct DhcpLeaseOut {
    #[serde(flatten)]
    pub lease_line: DhcpLeaseLine,
    pub nud_state: Option<NUDState>,
    pub rank: Option<u8>,
}

/// Enrich DHCP leases with NUD state and rank using get_macs
pub async fn enrich_leases_with_nud_state(leases: Vec<DhcpLeaseLine>) -> Vec<DhcpLeaseOut> {
    let ips: Vec<IpAddr> = leases.iter().map(|l| l.ip).collect();
    let mut map: std::collections::HashMap<IpAddr, (NUDState, u8)> =
        std::collections::HashMap::new();
    if let Ok(rows) = get_macs(&[] as &[&str], &ips, &[] as &[&str], &[], &[]).await {
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
        .map(|lease_line| {
            let (nud_state, rank) = map.get(&lease_line.ip).copied().unzip();
            DhcpLeaseOut {
                lease_line,
                nud_state,
                rank,
            }
        })
        .collect()
}
