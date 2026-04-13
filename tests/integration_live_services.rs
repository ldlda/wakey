use std::net::IpAddr;

use wakey::{broadcast_wake_targets, get_interface_summaries, inventory, resolve_query};
use wakey_core::{DeviceFilters, DeviceQuery};

#[tokio::test]
#[ignore = "runs against live router data; use on-device or via scripts/test_remote.ps1"]
async fn interfaces_real_router_prints_interface_summaries() -> anyhow::Result<()> {
    let interfaces = get_interface_summaries().await?;
    assert!(
        !interfaces.is_empty(),
        "expected at least one non-loopback interface summary"
    );
    println!("{}", serde_json::to_string_pretty(&interfaces)?);
    Ok(())
}

#[tokio::test]
#[ignore = "runs against live router data; use on-device or via scripts/test_remote.ps1"]
async fn inventory_real_router_default_query_returns_rows_or_empty_cleanly() -> anyhow::Result<()> {
    let inv = inventory(DeviceQuery::default()).await?;
    println!("{}", serde_json::to_string_pretty(&inv)?);
    Ok(())
}

#[tokio::test]
#[ignore = "runs against live router data; use on-device or via scripts/test_remote.ps1"]
async fn inventory_real_router_for_interface_filter_succeeds() -> anyhow::Result<()> {
    let interfaces = get_interface_summaries().await?;
    let first = interfaces
        .first()
        .map(|iface| iface.ifname.clone())
        .expect("expected at least one interface");

    let inv = inventory(DeviceQuery {
        name: None,
        filter: DeviceFilters {
            devs: vec![first.clone()],
            ..Default::default()
        },
    })
    .await?;

    println!("filtered dev: {first}");
    println!("{}", serde_json::to_string_pretty(&inv)?);
    Ok(())
}

#[tokio::test]
#[ignore = "runs against live router data; use on-device or via scripts/test_remote.ps1"]
async fn inventory_real_router_string_input_for_interface_succeeds() -> anyhow::Result<()> {
    let interfaces = get_interface_summaries().await?;
    let first = interfaces
        .first()
        .map(|iface| iface.ifname.clone())
        .expect("expected at least one interface");

    let inv = inventory(resolve_query(first.clone()).await?).await?;
    println!("selector: {first}");
    println!("{}", serde_json::to_string_pretty(&inv)?);
    Ok(())
}

#[tokio::test]
#[ignore = "runs against live router data; use on-device or via scripts/test_remote.ps1"]
async fn broadcast_wake_targets_real_router_resolve_from_interfaces() -> anyhow::Result<()> {
    let mac: macaddr::MacAddr = "aa:bb:cc:dd:ee:ff".parse()?;
    let targets = broadcast_wake_targets(mac).await?;

    assert!(
        targets.iter().all(|target| target.mac == Some(mac)),
        "all broadcast targets should preserve the requested MAC"
    );
    assert!(
        targets
            .iter()
            .all(|target| matches!(target.ip, Some(IpAddr::V4(_)))),
        "broadcast targets should be IPv4 broadcast destinations"
    );

    println!("{}", serde_json::to_string_pretty(&targets)?);
    Ok(())
}
