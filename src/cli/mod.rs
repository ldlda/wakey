//! CLI argument parsing, dispatch, rendering, and tracing defaults.

pub mod table;

use std::net::IpAddr;

use anyhow::Result;
use clap::{ArgAction, Args, Parser, Subcommand};
use tracing::debug;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};
use wakey_core::{InterfaceSummary, InventoryQuery, InventoryQueryBuilder, Query, WakeResult};

#[derive(Parser)]
#[command(name = "wakey")]
#[command(version, about = "Operator CLI for Wakey service actions")]
pub struct Cli {
    /// Increase log verbosity. Use `-v` for debug and `-vv` for trace.
    #[arg(short = 'v', long = "verbose", action = ArgAction::Count, global = true)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Show merged device inventory rows.
    #[command(visible_alias = "status")]
    Inventory(InventoryArgs),
    /// Show DHCP leases, optionally enriched with current neighbor state.
    Leases(LeasesArgs),
    /// Send Wake-on-LAN packets from a query or explicit MAC/IP pair.
    Wake(WakeArgs),
    /// Show condensed network interface summaries.
    Devs(DevsArgs),
}

#[derive(Args)]
pub struct LeasesArgs {
    /// Include best-known current neighbor state for each lease IP.
    #[arg(long)]
    pub include_state: bool,
    /// Print machine-readable JSON instead of a table.
    #[arg(long)]
    pub json: bool,
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
pub struct WakeArgs {
    /// Free-form device query, for example a hostname, IP, MAC, interface, or NUD state.
    pub query: Option<String>,
    /// Explicit MAC address for manual wake mode.
    #[arg(long)]
    pub mac: Option<macaddr::MacAddr>,
    /// Explicit destination IP or broadcast address for manual wake mode.
    #[arg(long)]
    pub ip: Option<IpAddr>,
    /// Print machine-readable JSON instead of a table.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
#[command(after_long_help = "Examples:
    wakey inventory bedroom-pc
    wakey inventory --mac aa:bb:cc:dd:ee:ff
    wakey inventory --dev br-lan --nud reachable

If only the positional query is provided, it is treated as free-form input and resolved through the smart selector path.")]
pub struct InventoryArgs {
    /// Free-form device query.
    pub query: Option<String>,
    /// Explicit name/text filter.
    #[arg(long)]
    pub name: Option<String>,
    /// Explicit IP filters.
    #[arg(long = "ip")]
    pub ips: Vec<std::net::IpAddr>,
    /// Explicit interface-name filters.
    #[arg(long = "dev")]
    pub devs: Vec<String>,
    /// Explicit neighbor-state filters.
    #[arg(long = "nud")]
    pub nuds: Vec<wakey_core::NeighborState>,
    /// Explicit MAC-address filters.
    #[arg(long = "mac")]
    pub macs: Vec<macaddr::MacAddr>,
    /// Print machine-readable JSON instead of a table.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
#[command(after_long_help = "Examples:
  wakey devs
  wakey devs br-lan
  wakey devs --up
  wakey devs --json")]
pub struct DevsArgs {
    /// Optional interface name to show.
    pub dev: Option<String>,
    /// Show only interfaces whose operstate is `up`.
    #[arg(long)]
    pub up: bool,
    /// Print machine-readable JSON instead of a table.
    #[arg(long)]
    pub json: bool,
}

pub fn init_tracing(verbose: u8) {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(default_filter_for_verbosity(verbose)))
        .expect("static tracing filter should parse");

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer())
        .init();
}

pub fn default_filter_for_verbosity(verbose: u8) -> &'static str {
    match verbose {
        0 => "wakey=info",
        1 => "wakey=debug",
        _ => "wakey=trace",
    }
}

pub async fn run(cli: Cli) -> Result<()> {
    init_tracing(cli.verbose);

    match cli.command {
        Command::Inventory(args) => {
            let as_json = args.json;
            let query = inventory_args_to_query(args);
            let selected_name = query.iter().find_map(|term| match term {
                Query::Text(text) => Some(text.clone()),
                _ => None,
            });
            debug!(?query, json = as_json, "dispatching inventory command");
            let status = wakey::inventory(query).await?;
            if as_json {
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else {
                if let Some(name) = &selected_name {
                    println!("name: {name}");
                }
                println!("{}", table::render_status_table(&status));
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
                println!("{}", table::render_leases_table(&leases));
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
                println!("{}", table::render_wake_table(&result));
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
                println!("{}", table::render_devs_table(&devs));
            }
        }
    }

    Ok(())
}

fn inventory_args_to_query(args: InventoryArgs) -> InventoryQuery {
    InventoryQueryBuilder::new()
        .maybe_text(args.name.or(args.query))
        .ips(args.ips)
        .interfaces(args.devs)
        .neighbor_states(args.nuds)
        .macs(args.macs)
        .build()
}

fn validate_wake_args(args: &WakeArgs) -> Result<()> {
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

async fn run_wake(args: WakeArgs) -> Result<WakeResult> {
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
        assert_eq!(default_filter_for_verbosity(0), "wakey=info");
        assert_eq!(default_filter_for_verbosity(1), "wakey=debug");
        assert_eq!(default_filter_for_verbosity(2), "wakey=trace");
        assert_eq!(default_filter_for_verbosity(9), "wakey=trace");
    }
}
