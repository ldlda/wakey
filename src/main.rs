mod cli_table;

use std::net::{IpAddr, SocketAddr};

use clap::{ArgAction, Args, Parser, Subcommand};
use tracing::{debug, info};
use wakey_core::{DeviceFilters, DeviceQuery, InterfaceSummary, WakeResult};

#[derive(Parser)]
#[command(name = "wakey")]
#[command(version, about = "CLI and temporary HTTP adapter for Wakey")]
#[command(
    long_about = "Wakey can run as a local/operator CLI or serve the legacy HTTP/static interface during the migration to a service-first architecture."
)]
struct Cli {
    /// Increase log verbosity. Use `-v` for debug and `-vv` for trace.
    #[arg(short = 'v', long = "verbose", action = ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the temporary legacy HTTP/static app.
    Http(HttpArgs),
    /// Show device status rows from neighbor/device data.
    Status(StatusArgs),
    /// Show DHCP leases, optionally enriched with current neighbor state.
    Leases(LeasesArgs),
    /// Send Wake-on-LAN packets from a query or explicit MAC/IP pair.
    Wake(WakeArgs),
    /// Show condensed network interface summaries.
    Devs(DevsArgs),
}

#[derive(Args)]
struct HttpArgs {
    /// Host address to bind the HTTP server to.
    #[arg(long, default_value = "::")]
    host: IpAddr,
    /// TCP port to bind the HTTP server to.
    #[arg(long, default_value_t = 12012)]
    port: u16,
}

#[derive(Args)]
struct LeasesArgs {
    /// Include best-known current neighbor state for each lease IP.
    #[arg(long)]
    include_state: bool,
    /// Print machine-readable JSON instead of a table.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
#[command(after_long_help = "Examples:
  wakey wake bedroom-pc
  wakey wake --mac aa:bb:cc:dd:ee:ff
  wakey wake --mac aa:bb:cc:dd:ee:ff --ip 192.168.1.255

Rules:
  - query mode and explicit --mac/--ip mode are mutually exclusive
  - --ip requires --mac
  - --mac without --ip fans out to interface broadcast targets")]
struct WakeArgs {
    /// Free-form device query, for example a hostname, IP, MAC, interface, or NUD state.
    query: Option<String>,
    /// Explicit MAC address for manual wake mode.
    #[arg(long)]
    mac: Option<macaddr::MacAddr>,
    /// Explicit destination IP or broadcast address for manual wake mode.
    #[arg(long)]
    ip: Option<IpAddr>,
    /// Print machine-readable JSON instead of a table.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
#[command(after_long_help = "Examples:
  wakey status bedroom-pc
  wakey status --mac aa:bb:cc:dd:ee:ff
  wakey status --dev br-lan --nud reachable

If only the positional query is provided, it is treated as free-form input and resolved through the smart selector path.")]
struct StatusArgs {
    /// Free-form device query.
    query: Option<String>,
    /// Explicit name/text filter.
    #[arg(long)]
    name: Option<String>,
    /// Explicit IP filters.
    #[arg(long = "ip")]
    ips: Vec<std::net::IpAddr>,
    /// Explicit interface-name filters.
    #[arg(long = "dev")]
    devs: Vec<String>,
    /// Explicit neighbor-state filters.
    #[arg(long = "nud")]
    nuds: Vec<wakey_core::NeighborState>,
    /// Explicit MAC-address filters.
    #[arg(long = "mac")]
    macs: Vec<macaddr::MacAddr>,
    /// Print machine-readable JSON instead of a table.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
#[command(after_long_help = "Examples:
  wakey devs
  wakey devs br-lan
  wakey devs --up
  wakey devs --json")]
struct DevsArgs {
    /// Optional interface name to show.
    dev: Option<String>,
    /// Show only interfaces whose operstate is `up`.
    #[arg(long)]
    up: bool,
    /// Print machine-readable JSON instead of a table.
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
    init_tracing(cli.verbose);
    match cli.command {
        Command::Http(args) => {
            let addr = SocketAddr::new(args.host, args.port);
            info!(%addr, "dispatching http command");
            wakey::serve_http_from_current_exe(addr).await?;
        }
        Command::Status(args) => {
            let as_json = args.json;
            let query = status_args_to_query(args);
            debug!(?query, json = as_json, "dispatching status command");
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
            debug!(
                include_state = args.include_state,
                json = args.json,
                "dispatching leases command"
            );
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
            debug!(
                has_query = args.query.is_some(),
                has_mac = args.mac.is_some(),
                has_ip = args.ip.is_some(),
                json = as_json,
                "dispatching wake command"
            );
            let result = run_wake(args).await?;
            if as_json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("{}", cli_table::render_wake_table(&result));
            }
        }
        Command::Devs(args) => {
            debug!(dev = ?args.dev, up = args.up, json = args.json, "dispatching devs command");
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

#[cfg(target_os = "linux")]
fn init_tracing(verbose: u8) {
    use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(default_filter_for_verbosity(verbose)))
        .expect("static tracing filter should parse");

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer())
        .init();
}

#[cfg(target_os = "linux")]
fn default_filter_for_verbosity(verbose: u8) -> &'static str {
    match verbose {
        0 => "wakey=info,tower_http=info",
        1 => "wakey=debug,tower_http=debug",
        _ => "wakey=trace,tower_http=trace",
    }
}

#[cfg(test)]
mod tests {
    use super::{WakeArgs, default_filter_for_verbosity};

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

    #[test]
    fn verbosity_maps_to_expected_default_filters() {
        assert_eq!(
            default_filter_for_verbosity(0),
            "wakey=info,tower_http=info"
        );
        assert_eq!(
            default_filter_for_verbosity(1),
            "wakey=debug,tower_http=debug"
        );
        assert_eq!(
            default_filter_for_verbosity(2),
            "wakey=trace,tower_http=trace"
        );
        assert_eq!(
            default_filter_for_verbosity(9),
            "wakey=trace,tower_http=trace"
        );
    }
}
