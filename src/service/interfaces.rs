use anyhow::Result;
use tracing::{debug, instrument};
use wakey_core::InterfaceSummary;

/// Return condensed interface summaries useful for CLI and wake routing.
#[instrument(skip_all)]
pub async fn get_interface_summaries() -> Result<Vec<InterfaceSummary>> {
    let summaries = wakey_linux::devices::list_interface_summaries().await?;
    debug!(count = summaries.len(), "loaded interface summaries");
    Ok(summaries)
}

/// Return one named interface summary when present.
#[instrument(skip_all, fields(ifname = name))]
pub async fn get_interface_summary(name: &str) -> Result<Option<InterfaceSummary>> {
    let summary = get_interface_summaries()
        .await?
        .into_iter()
        .find(|iface| iface.ifname == name);
    debug!(found = summary.is_some(), "resolved interface summary");
    Ok(summary)
}

/// Resolve a hostname through the local resolver and collect all returned IPs.
pub async fn get_ips(name: impl AsRef<str>) -> Result<Vec<std::net::IpAddr>> {
    Ok(wakey_linux::devices::get_ips(name.as_ref())
        .await?
        .collect())
}
