use std::net::{IpAddr, SocketAddr};

use chrono::{DateTime, Local, Utc};
use clap::{Args, Parser, Subcommand};
use comfy_table::{Cell, ContentArrangement, Table, presets};
use wakey_core::{DeviceFilters, DeviceQuery, DhcpLeaseWithState, InterfaceSummary, WakeResult};

#[derive(Parser)]
#[command(name = "wakey")]
#[command(version, about = "Wakey service CLI and HTTP adapter")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Http(HttpArgs),
    Status(StatusArgs),
    Leases(LeasesArgs),
    Wake(WakeArgs),
    Devs(DevsArgs),
}

#[derive(Args)]
struct HttpArgs {
    #[arg(long, default_value = "::")]
    host: IpAddr,
    #[arg(long, default_value_t = 12012)]
    port: u16,
}

#[derive(Args)]
struct LeasesArgs {
    #[arg(long)]
    include_state: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct WakeArgs {
    query: String,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct StatusArgs {
    query: Option<String>,
    #[arg(long)]
    name: Option<String>,
    #[arg(long = "ip")]
    ips: Vec<std::net::IpAddr>,
    #[arg(long = "dev")]
    devs: Vec<String>,
    #[arg(long = "nud")]
    nuds: Vec<wakey_core::NeighborState>,
    #[arg(long = "mac")]
    macs: Vec<macaddr::MacAddr>,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct DevsArgs {
    #[arg(long)]
    json: bool,
}

fn status_args_to_query(args: StatusArgs) -> wakey_core::DeviceQuery {
    if let Some(query) = args.query.as_ref()
        && args.name.is_none()
        && args.ips.is_empty()
        && args.devs.is_empty()
        && args.nuds.is_empty()
        && args.macs.is_empty()
    {
        return DeviceQuery {
            name: Some(query.clone()),
            ..Default::default()
        };
    }

    DeviceQuery {
        name: args.name.or(args.query),
        filter: DeviceFilters {
            ips: args.ips,
            devs: args.devs,
            nuds: args.nuds,
            macs: args.macs,
        },
    }
}

fn base_table() -> Table {
    let mut table = Table::new();
    table
        .load_preset(presets::UTF8_FULL_CONDENSED)
        .set_content_arrangement(ContentArrangement::Dynamic);
    table
}

fn render_status_table(status: &wakey::StatusResponse) -> Table {
    let mut table = base_table();
    table.set_header(["IP", "MAC", "State", "IF"]);
    for row in &status.table {
        table.add_row([
            Cell::new(row.ip),
            Cell::new(row.mac.map(|m| m.to_string()).unwrap_or_default()),
            Cell::new(format!("{:?}", row.state).to_lowercase()),
            Cell::new(row.dev.clone().unwrap_or_default()),
        ]);
    }
    table
}

fn render_leases_table(leases: &[DhcpLeaseWithState]) -> Table {
    let mut table = base_table();
    table.set_header(["IP", "MAC", "Name", "Expires", "NUD"]);
    for lease in leases {
        let expires = format_epoch_local(lease.lease_line.expires_epoch);
        table.add_row([
            Cell::new(lease.lease_line.ip),
            Cell::new(lease.lease_line.mac),
            Cell::new(lease.lease_line.name.clone().unwrap_or_default()),
            Cell::new(expires),
            Cell::new(
                lease
                    .nud_state
                    .map(|s| format!("{:?}", s).to_lowercase())
                    .unwrap_or_default(),
            ),
        ]);
    }
    table
}

fn render_wake_table(result: &WakeResult) -> Table {
    let mut table = base_table();
    table.set_header(["IP", "MAC", "Status"]);
    for row in &result.result {
        table.add_row([
            Cell::new(row.target.ip.map(|ip| ip.to_string()).unwrap_or_default()),
            Cell::new(row.target.mac.map(|m| m.to_string()).unwrap_or_default()),
            Cell::new(format!("{:?}", row.status).to_lowercase()),
        ]);
    }
    table
}

fn render_devs_table(devs: &[InterfaceSummary]) -> Table {
    let mut table = base_table();
    table.set_header([
        "Interface",
        "State",
        "MAC",
        "Addresses",
        "Broadcasts",
        "Scope/Label",
    ]);
    for dev in devs {
        let addresses = dev
            .addrs
            .iter()
            .filter_map(|addr| addr.cidr.as_deref())
            .collect::<Vec<_>>()
            .join("\n");
        let broadcasts = dev
            .addrs
            .iter()
            .filter_map(|addr| addr.broadcast)
            .map(|addr| addr.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let scope_label = dev
            .addrs
            .iter()
            .map(|addr| match (&addr.scope, &addr.label) {
                (Some(scope), Some(label)) => format!("{scope} ({label})"),
                (Some(scope), None) => scope.clone(),
                (None, Some(label)) => label.clone(),
                (None, None) => String::new(),
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");

        table.add_row([
            Cell::new(&dev.ifname),
            Cell::new(&dev.operstate),
            Cell::new(dev.mac.map(|m| m.to_string()).unwrap_or_default()),
            Cell::new(addresses),
            Cell::new(broadcasts),
            Cell::new(scope_label),
        ]);
    }
    table
}

fn format_epoch_local(epoch: u64) -> String {
    match DateTime::<Utc>::from_timestamp(epoch as i64, 0) {
        Some(dt) => dt
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
        None => epoch.to_string(),
    }
}

#[cfg(not(target_os = "linux"))]
fn main() -> anyhow::Result<()> {
    Err(anyhow::anyhow!(
        "OS not supported! run this on your ahh router!"
    ))
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Http(args) => {
            let addr = SocketAddr::new(args.host, args.port);
            wakey::serve_http_from_current_exe(addr).await?;
        }
        Command::Status(args) => {
            let as_json = args.json;
            let query = status_args_to_query(args);
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
            if as_json {
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else {
                if let Some(name) = &status.name {
                    println!("name: {name}");
                }
                println!("{}", render_status_table(&status));
            }
        }
        Command::Leases(args) => {
            let leases = wakey::get_leases(wakey_core::LeaseQuery {
                include_state: args.include_state,
            })
            .await?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&leases)?);
            } else {
                println!("{}", render_leases_table(&leases));
            }
        }
        Command::Wake(args) => {
            let result = wakey::wake_from_query(args.query).await?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("{}", render_wake_table(&result));
            }
        }
        Command::Devs(args) => {
            let devs = wakey::get_interface_summaries().await?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&devs)?);
            } else {
                println!("{}", render_devs_table(&devs));
            }
        }
    }
    Ok(())
}
