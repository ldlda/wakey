use anyhow::{Context, Result};
use wakey_core::{DhcpLease, DhcpLeaseWithState};

/// Read DHCP leases from dnsmasq plus remembered hook names.
pub async fn get_leases() -> Result<Vec<DhcpLeaseWithState>> {
    let leases = wakey_linux::dhcp::read_dhcp_leases_with_names()
        .await
        .context("failed to read DHCP leases")?;
    Ok(leases_without_state(leases))
}

/// Read DHCP leases and enrich them with current neighbor state.
///
/// Prefer inventory for device status; this exists for the legacy leases view.
pub async fn get_leases_with_neighbor_state() -> Result<Vec<DhcpLeaseWithState>> {
    let leases = wakey_linux::dhcp::read_dhcp_leases_with_names()
        .await
        .context("failed to read DHCP leases")?;
    // this is only used in the wakey CLI
    #[allow(deprecated)]
    Ok(wakey_linux::dhcp::enrich_leases_with_nud_state(leases).await)
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
