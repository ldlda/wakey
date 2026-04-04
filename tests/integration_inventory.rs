use wakey::inventory;
use wakey_core::DeviceQuery;

#[tokio::test]
#[ignore = "runs against live router data; use on-device or via scripts/test_remote.ps1"]
async fn inventory_real_router_prints_device_inventory() -> anyhow::Result<()> {
    let inventory = inventory(DeviceQuery::default()).await?;
    println!("{}", serde_json::to_string_pretty(&inventory)?);
    Ok(())
}
