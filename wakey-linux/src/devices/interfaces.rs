use anyhow::{Context, Result};
use lda_ipjs::subcommands::address;
use std::collections::HashSet;
use wakey_core::{InterfaceAddr, InterfaceSummary};

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
                    cidr: info
                        .cidr
                        .local
                        .zip(info.cidr.prefixlen)
                        .map(|(addr, prefixlen)| format!("{addr}/{prefixlen}")),
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

pub async fn has_dev(name: &str) -> bool {
    list_devs().await.contains(name)
}
