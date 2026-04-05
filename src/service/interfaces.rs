use anyhow::Result;
use wakey_core::InterfaceSummary;

pub async fn list_interfaces() -> Result<Vec<String>> {
    Ok(wakey_linux::devices::devs_sorted().await)
}

pub async fn get_interface_summaries() -> Result<Vec<InterfaceSummary>> {
    wakey_linux::devices::list_interface_summaries().await
}

pub async fn get_interface_summary(name: &str) -> Result<Option<InterfaceSummary>> {
    Ok(get_interface_summaries()
        .await?
        .into_iter()
        .find(|iface| iface.ifname == name))
}

pub async fn get_ips(name: impl AsRef<str>) -> Result<Vec<std::net::IpAddr>> {
    Ok(wakey_linux::devices::get_ips(name.as_ref())
        .await?
        .collect())
}
