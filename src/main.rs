mod cli_table;

use std::net::{IpAddr, SocketAddr};

use clap::{Args, Parser, Subcommand};
use wakey_core::{DeviceFilters, DeviceQuery, InterfaceSummary, WakeResult};

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
    query: Option<String>,
    #[arg(long)]
    mac: Option<macaddr::MacAddr>,
    #[arg(long)]
    ip: Option<IpAddr>,
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
    dev: Option<String>,
    #[arg(long)]
    up: bool,
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

fn validate_wake_args(args: &WakeArgs) -> anyhow::Result<()> {
    let has_query = args.query.is_some();
    let has_mac = args.mac.is_some();
    let has_ip = args.ip.is_some();

    if has_ip && !has_mac {
        anyhow::bail!("`wakey wake --ip` needs `--mac`");
    }

    if has_query && (has_mac || has_ip) {
        anyhow::bail!("query mode and explicit `--mac/--ip` mode are mutually exclusive");
    }

    if !has_query && !has_mac {
        anyhow::bail!("provide either a query or `--mac`");
    }

    Ok(())
}

async fn run_wake(args: WakeArgs) -> anyhow::Result<WakeResult> {
    validate_wake_args(&args)?;

    match (args.query, args.mac, args.ip) {
        (Some(query), None, None) => wakey::wake_from_query(query).await,
        (None, Some(mac), ip) => wakey::wake_explicit(mac, ip).await,
        _ => unreachable!("wake args validated before dispatch"),
    }
}

fn filter_interface_summaries(
    mut devs: Vec<InterfaceSummary>,
    args: &DevsArgs,
) -> Vec<InterfaceSummary> {
    if args.up {
        devs.retain(|dev| dev.operstate == "up");
    }
    if let Some(name) = &args.dev {
        devs.retain(|dev| &dev.ifname == name);
    }
    devs
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
                println!("{}", cli_table::render_status_table(&status));
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
                println!("{}", cli_table::render_leases_table(&leases));
            }
        }
        Command::Wake(args) => {
            let as_json = args.json;
            let result = run_wake(args).await?;
            if as_json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("{}", cli_table::render_wake_table(&result));
            }
        }
        Command::Devs(args) => {
            let devs = if let Some(name) = &args.dev {
                wakey::get_interface_summary(name)
                    .await?
                    .into_iter()
                    .collect()
            } else {
                wakey::get_interface_summaries().await?
            };
            let devs = filter_interface_summaries(devs, &args);
            if args.json {
                println!("{}", serde_json::to_string_pretty(&devs)?);
            } else {
                println!("{}", cli_table::render_devs_table(&devs));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::WakeArgs;

    #[test]
    fn wake_rejects_ip_without_mac() {
        let err = super::validate_wake_args(&WakeArgs {
            query: None,
            mac: None,
            ip: Some("192.168.1.10".parse().expect("ip")),
            json: false,
        })
        .expect_err("ip-only wake should be rejected");

        assert!(err.to_string().contains("--ip"));
    }

    #[test]
    fn wake_rejects_mixed_query_and_explicit_mode() {
        let err = super::validate_wake_args(&WakeArgs {
            query: Some("pc".into()),
            mac: Some("aa:bb:cc:dd:ee:ff".parse().expect("mac")),
            ip: None,
            json: false,
        })
        .expect_err("mixed wake mode should be rejected");

        assert!(err.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn wake_accepts_query_mode() {
        super::validate_wake_args(&WakeArgs {
            query: Some("pc".into()),
            mac: None,
            ip: None,
            json: false,
        })
        .expect("query mode should be accepted");
    }

    #[test]
    fn wake_accepts_manual_mac_mode() {
        super::validate_wake_args(&WakeArgs {
            query: None,
            mac: Some("aa:bb:cc:dd:ee:ff".parse().expect("mac")),
            ip: None,
            json: false,
        })
        .expect("manual mac mode should be accepted");
    }
}
