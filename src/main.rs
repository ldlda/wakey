use std::net::{IpAddr, SocketAddr};

use clap::{Args, Parser, Subcommand};
use wakey_core::{DeviceFilters, DeviceQuery};

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
    Devs,
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
}

#[derive(Args)]
struct WakeArgs {
    query: String,
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
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
        Command::Leases(args) => {
            let leases = wakey::get_leases(wakey_core::LeaseQuery {
                include_state: args.include_state,
            })
            .await?;
            println!("{}", serde_json::to_string_pretty(&leases)?);
        }
        Command::Wake(args) => {
            let result = wakey::wake_from_query(args.query).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::Devs => {
            let devs = wakey::list_interfaces().await?;
            println!("{}", serde_json::to_string_pretty(&devs)?);
        }
    }
    Ok(())
}
