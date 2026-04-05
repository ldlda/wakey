use anyhow::{Context, Result};
use lda_ipjs::subcommands::address;
use std::collections::HashSet;
use wakey_core::{InterfaceAddr, InterfaceSummary};

/// Discover interface names from Linux without requiring `ip`.
///
/// This is the lowest-common-denominator interface listing path and is used for
/// quick existence checks and legacy string-only callers.
pub async fn list_devs() -> HashSet<String> {
    fn get_dev() -> HashSet<String> {
        let mut devs: HashSet<String> = HashSet::new();
        if let Ok(rd) = std::fs::read_dir("/sys/class/net") {
            for e in rd.flatten() {
                if e.file_type()
                    .map(|ft| {
                        if ft.is_symlink() {
                            std::fs::metadata(e.path())
                                .map(|m| m.is_dir())
                                .unwrap_or(false)
                        } else {
                            ft.is_dir()
                        }
                    })
                    .unwrap_or(false)
                    && let Ok(name) = e.file_name().into_string()
                    && name != "lo"
                    && !name.is_empty()
                {
                    devs.insert(name);
                }
            }
        } else if let Ok(txt) = std::fs::read_to_string("/proc/net/dev") {
            for line in txt.lines().skip(2) {
                if let Some((name, _rest)) = line.split_once(':') {
                    let n = name.trim().to_string();
                    if n != "lo" && !n.is_empty() {
                        devs.insert(n);
                    }
                }
            }
        }
        devs
    }

    tokio::task::spawn_blocking(get_dev)
        .await
        .unwrap_or_default()
}

pub async fn devs_sorted() -> Vec<String> {
    let mut v: Vec<String> = list_devs().await.into_iter().collect();
    v.sort();
    v
}

/// Build condensed interface summaries from Linux address data.
///
/// On Unix this prefers the `ipjs` netlink-backed address path; elsewhere it
/// falls back to the JSON command path. The result is not a full `ip address show`
/// dump. It is a smaller projection containing the parts `wakey` currently uses:
/// interface name/index, operstate, MAC, bound addresses, and IPv4 broadcast
/// addresses for Wake-on-LAN delivery.
///
/// Loopback is intentionally excluded.
pub async fn list_interface_summaries() -> Result<Vec<InterfaceSummary>> {
    #[cfg(unix)]
    let rows = address::nl::get(None)
        .await
        .context("rtnetlink address query failed")?;

    #[cfg(not(unix))]
    let rows = address::get_with_backend(address::Backend::Json, None)
        .await
        .context("ip -j address show failed")?;

    let mut out: Vec<InterfaceSummary> = rows
        .into_iter()
        .filter(|row| row.ifname != "lo")
        .map(|row| InterfaceSummary {
            ifindex: row.ifindex,
            ifname: row.ifname,
            operstate: row.operstate.as_str().to_ascii_lowercase(),
            mac: row.address,
            addrs: row
                .addr_info
                .into_iter()
                .map(|info| InterfaceAddr {
                    family: info.family.map(|family| family.as_str().to_string()),
                    cidr: info.cidr.to_cidr_string(),
                    broadcast: info.broadcast,
                    scope: info.scope,
                    label: info.label,
                })
                .collect(),
        })
        .collect();

    out.sort_by(|a, b| a.ifname.cmp(&b.ifname));
    Ok(out)
}

/// Return whether a named interface exists according to [`list_devs`].
pub async fn has_dev(name: &str) -> bool {
    list_devs().await.contains(name)
}
