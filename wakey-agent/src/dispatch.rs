use anyhow::Result;
use tracing::{debug, info, instrument};

use crate::config::AgentConfig;
use crate::inventory;
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
        AgentCommand::Wake(req) => dispatch_wake(req).await,
        AgentCommand::Ping(req) => crate::ping::probe(req).await,
    }
}

async fn dispatch_leases(req: LeasesRequest, config: &AgentConfig) -> Result<CommandResult> {
    let leases = inventory::read_leases(config).await?;
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
    let inventory = inventory::collect(req.into_inventory_query(), config).await?;
    Ok(CommandResult::Inventory(inventory))
}

async fn dispatch_wake(req: WakeRequest) -> Result<CommandResult> {
    let result = wakey::WakeInput::from_parts(req.query, req.mac, req.ip)?
        .send()
        .await?;
    debug!("dispatched wake command");
    Ok(CommandResult::Wake(result))
}
