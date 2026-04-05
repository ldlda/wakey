use anyhow::Result;
use wakey_core::InterfaceSummary;

/// Return interface names only.
///
/// This is the old, lightweight interface listing surface kept for compatibility.
pub async fn list_interfaces() -> Result<Vec<String>> {
    Ok(wakey_linux::devices::devs_sorted().await)
}

/// Return condensed interface summaries useful for CLI and wake routing.
pub async fn get_interface_summaries() -> Result<Vec<InterfaceSummary>> {
    wakey_linux::devices::list_interface_summaries().await
}

/// Return one named interface summary when present.
pub async fn get_interface_summary(name: &str) -> Result<Option<InterfaceSummary>> {
    Ok(get_interface_summaries()
        .await?
        .into_iter()
        .find(|iface| iface.ifname == name))
}

/// Resolve a hostname through the local resolver and collect all returned IPs.
pub async fn get_ips(name: impl AsRef<str>) -> Result<Vec<std::net::IpAddr>> {
    Ok(wakey_linux::devices::get_ips(name.as_ref())
        .await?
        .collect())
}
