use anyhow::{Context, Result};
use wakey_core::{DhcpLease, DhcpLeaseWithState, LeaseQuery};

/// Read DHCP leases and optionally enrich them with current neighbor-state data.
pub async fn get_leases(query: LeaseQuery) -> Result<Vec<DhcpLeaseWithState>> {
    let leases = wakey_linux::dhcp::read_dhcp_leases_with_names()
        .await
        .context("failed to read DHCP leases")?;
    if query.include_state {
        Ok(wakey_linux::dhcp::enrich_leases_with_nud_state(leases).await)
    } else {
        Ok(leases_without_state(leases))
    }
}

/// Wrap raw DHCP leases in the current service output shape without neighbor state.
pub fn leases_without_state(leases: Vec<DhcpLease>) -> Vec<DhcpLeaseWithState> {
    leases
        .into_iter()
        .map(|lease_line| DhcpLeaseWithState {
            lease_line,
            nud_state: None,
        })
        .collect()
}
