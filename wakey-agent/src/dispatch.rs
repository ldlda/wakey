use anyhow::Result;
use tracing::{debug, info, instrument};

use crate::protocol::{
    AgentCommand, CommandResult, DevsRequest, InventoryRequest, LeasesRequest, StatusRequest,
    WakeRequest,
};

#[instrument(skip_all)]
pub async fn dispatch_command(command: AgentCommand) -> Result<CommandResult> {
    let kind = command_kind(&command);
    info!(command = %kind, "dispatching command into local wakey services");
    match command {
        AgentCommand::Status(req) => dispatch_status(req).await,
        AgentCommand::Leases(req) => dispatch_leases(req).await,
        AgentCommand::Devs(req) => dispatch_devs(req).await,
        AgentCommand::Inventory(req) => dispatch_inventory(req).await,
        AgentCommand::Wake(req) => dispatch_wake(req).await,
    }
}

async fn dispatch_status(req: StatusRequest) -> Result<CommandResult> {
    let query = req.into_device_query();
    let status = if query.name.is_some()
        && query.filter.ips.is_empty()
        && query.filter.devs.is_empty()
        && query.filter.nuds.is_empty()
        && query.filter.macs.is_empty()
    {
        wakey::get_status_for_input(query.name.clone().unwrap_or_default()).await?
    } else {
        wakey::get_status(query).await?
    };
    debug!(devices = status.devices.len(), "dispatched status command");
    Ok(CommandResult::Status(status))
}

async fn dispatch_leases(req: LeasesRequest) -> Result<CommandResult> {
    let leases = wakey::get_leases(wakey_core::LeaseQuery {
        include_state: req.include_state,
    })
    .await?;
    debug!(
        rows = leases.len(),
        include_state = req.include_state,
        "dispatched leases command"
    );
    Ok(CommandResult::Leases { rows: leases })
}

async fn dispatch_devs(req: DevsRequest) -> Result<CommandResult> {
    let mut devs = if let Some(name) = &req.dev {
        wakey::get_interface_summary(name)
            .await?
            .into_iter()
            .collect()
    } else {
        wakey::get_interface_summaries().await?
    };
    if req.up_only {
        devs.retain(|dev| dev.operstate == "up");
    }
    debug!(
        rows = devs.len(),
        up_only = req.up_only,
        "dispatched devs command"
    );
    Ok(CommandResult::Devs { rows: devs })
}

async fn dispatch_inventory(req: InventoryRequest) -> Result<CommandResult> {
    let inventory = wakey::inventory(req.into_device_query()).await?;
    debug!(
        rows = inventory.devices.len(),
        "dispatched inventory command"
    );
    Ok(CommandResult::Inventory(inventory))
}

async fn dispatch_wake(req: WakeRequest) -> Result<CommandResult> {
    validate_wake_request(&req)?;
    let result = match (req.query, req.mac, req.ip) {
        (Some(query), None, None) => wakey::wake_from_query(query).await?,
        (None, Some(mac), ip) => wakey::wake_explicit(mac, ip).await?,
        _ => unreachable!("wake request validated before dispatch"),
    };
    debug!("dispatched wake command");
    Ok(CommandResult::Wake(result))
}

fn command_kind(command: &AgentCommand) -> &'static str {
    match command {
        AgentCommand::Status(_) => "status",
        AgentCommand::Leases(_) => "leases",
        AgentCommand::Devs(_) => "devs",
        AgentCommand::Inventory(_) => "inventory",
        AgentCommand::Wake(_) => "wake",
    }
}

pub fn validate_wake_request(req: &WakeRequest) -> Result<()> {
    let has_query = req.query.is_some();
    let has_mac = req.mac.is_some();
    let has_ip = req.ip.is_some();

    if has_ip && !has_mac {
        anyhow::bail!("wake request `ip` requires `mac`");
    }
    if has_query && (has_mac || has_ip) {
        anyhow::bail!("wake query mode and explicit mode are mutually exclusive");
    }
    if !has_query && !has_mac {
        anyhow::bail!("wake request requires either `query` or `mac`");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::WakeRequest;

    #[test]
    fn wake_rejects_ip_without_mac() {
        let err = validate_wake_request(&WakeRequest {
            query: None,
            mac: None,
            ip: Some("192.168.1.1".parse().expect("ip")),
        })
        .expect_err("should reject");
        assert!(err.to_string().contains("requires `mac`"));
    }

    #[test]
    fn wake_rejects_mixed_mode() {
        let err = validate_wake_request(&WakeRequest {
            query: Some("pc".into()),
            mac: Some("aa:bb:cc:dd:ee:ff".parse().expect("mac")),
            ip: None,
        })
        .expect_err("should reject");
        assert!(err.to_string().contains("mutually exclusive"));
    }
}
