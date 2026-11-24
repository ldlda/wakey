use anyhow::Result;
use tracing::{debug, info, instrument};

use crate::config::AgentConfig;
use crate::protocol::{
    AgentCommand, CommandResult, DevsRequest, InventoryRequest, LeasesRequest, WakeRequest,
};

#[instrument(skip_all)]
pub async fn dispatch_command(
    command: AgentCommand,
    config: &AgentConfig,
) -> Result<CommandResult> {
    let kind = command.kind();
    info!(command = %kind, "dispatching command into local wakey services");
    match command {
        AgentCommand::Leases(req) => dispatch_leases(req, config).await,
        AgentCommand::Devs(req) => dispatch_devs(req).await,
        AgentCommand::Inventory(req) => dispatch_inventory(req, config).await,
        AgentCommand::Wake(req) => dispatch_wake(req, config).await,
        AgentCommand::Ping(req) => crate::ping::probe(req).await,
    }
}

async fn dispatch_leases(req: LeasesRequest, config: &AgentConfig) -> Result<CommandResult> {
    let leases = config.inventory_sources().read_leases().await?;
    let leases = if req.include_state {
        // only runs when requested explicitly to the endpoint
        #[allow(deprecated)]
        wakey::wakey_linux::dhcp::enrich_leases_with_nud_state(leases).await
    } else {
        wakey::leases_without_state(leases)
    };
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

async fn dispatch_inventory(req: InventoryRequest, config: &AgentConfig) -> Result<CommandResult> {
    let inventory = config
        .inventory_sources()
        .collect(req.into_inventory_query())
        .await?;
    Ok(CommandResult::Inventory(inventory))
}

async fn dispatch_wake(req: WakeRequest, config: &AgentConfig) -> Result<CommandResult> {
    let result = wakey::WakeInput::from_parts(req.query, req.mac, req.ip)?
        .send_with_sources(&config.inventory_sources())
        .await?;
    debug!("dispatched wake command");
    Ok(CommandResult::Wake(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn query_wake_uses_the_configured_dhcp_path() {
        let dir = std::env::temp_dir().join(format!("wakey-wake-path-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = AgentConfig {
            // Reading a directory as a lease file fails before any wake packet is sent.
            dhcp_leases_path: dir.clone(),
            ..(*crate::config::DEFAULT_CONFIG).clone()
        };
        let command = AgentCommand::Wake(WakeRequest {
            query: Some("192.0.2.184".into()),
            mac: None,
            ip: None,
        });
        let result = dispatch_command(command, &config).await;
        std::fs::remove_dir(dir).unwrap();
        let Err(error) = result else {
            panic!("expected reading the configured DHCP path to fail");
        };
        assert!(
            error.to_string().contains("failed to read DHCP leases"),
            "{error:#}"
        );
    }
}
