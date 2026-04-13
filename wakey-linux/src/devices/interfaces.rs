use anyhow::{Context, Result};
use lda_ipjs::subcommands::address;
use wakey_core::{InterfaceAddr, InterfaceSummary};

/// Build condensed interface summaries from Linux address data.
///
/// On Unix this prefers the `ipjs` netlink-backed address path; elsewhere it
/// falls back to the JSON command path. The result is not a full `ip address show`
/// dump. It is a smaller projection containing the parts `wakey` currently uses:
/// interface name/index, operstate, MAC, bound addresses, and IPv4 broadcast
/// addresses for Wake-on-LAN delivery.
///
/// Loopback is intentionally excluded.
pub async fn list_interface_summaries() -> Result<Vec<InterfaceSummary>> {
    #[cfg(unix)]
    let rows = address::nl::get(None)
        .await
        .context("rtnetlink address query failed")?;

    #[cfg(not(unix))]
    let rows = address::get_with_backend(address::Backend::Json, None)
        .await
        .context("ip -j address show failed")?;

    let mut out: Vec<InterfaceSummary> = rows
        .into_iter()
        .filter(|row| row.ifname != "lo")
        .map(|row| InterfaceSummary {
            ifindex: row.ifindex,
            ifname: row.ifname,
            operstate: row.operstate.as_str().to_ascii_lowercase(),
            mac: row.address,
            addrs: row
                .addr_info
                .into_iter()
                .map(|info| InterfaceAddr {
                    family: info.family.map(|family| family.as_str().to_string()),
                    cidr: info.cidr.to_cidr_string(),
                    broadcast: info.broadcast,
                    scope: info.scope,
                    label: info.label,
                })
                .collect(),
        })
        .collect();

    out.sort_by(|a, b| a.ifname.cmp(&b.ifname));
    Ok(out)
}

/// Return whether a named non-loopback interface exists.
pub async fn has_dev(name: &str) -> bool {
    match list_interface_summaries().await {
        Ok(interfaces) => interfaces.iter().any(|iface| iface.ifname == name),
        Err(_) => false,
    }
}
