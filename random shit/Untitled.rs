use macaddr::MacAddr6;
use serde::Deserialize;
use std::{net::IpAddr, process::Command};

// Raw JSON shape from ip -j -4 address show
#[derive(Debug, Deserialize)]
struct IpJAddrInfo {
    family: Option<String>,
    local: Option<String>,
    broadcast: Option<String>,
    scope: Option<String>,
    label: Option<String>,
    prefixlen: Option<u8>,
    // many more exist; we only take what we need
}

#[derive(Debug, Deserialize)]
struct IpJAddrEntry {
    ifname: String,
    // interface MAC address (present on non-loopback):
    address: Option<String>,
    addr_info: Option<Vec<IpJAddrInfo>>,
}

#[derive(Debug, Clone)]
pub struct AddrInfo {
    pub local: IpAddr,
    pub broadcast: Option<IpAddr>,
    pub scope: Option<String>,
    pub label: Option<String>,
    pub prefixlen: Option<u8>,
}

#[derive(Debug, Clone)]
pub struct IpAEntry {
    pub ifname: String,
    pub mac: Option<MacAddr6>,
    pub addr_info: Vec<AddrInfo>,
    // IPv4 only (inet)
}

// Run ip -j -4 address show and deserialize
fn read_ip_json() -> Result<Vec<IpJAddrEntry>, Box<dyn std::error::Error>> {
    let out = Command::new("ip")
        .args(["-j", "-4", "address", "show"])
        .output()?;
    if !out.status.success() {
        return Err(format!("ip exited with {}", out.status).into());
    }
    let entries: Vec<IpJAddrEntry> = serde_json::from_slice(&out.stdout)?;
    Ok(entries)
}

fn parse_ip(s: &str) -> Option<IpAddr> {
    s.parse::<IpAddr>().ok()
}

fn parse_mac(s: &str) -> Option<MacAddr6> {
    // ip outputs lowercase aa:bb:..., which MacAddr6 can parse
    s.parse::<MacAddr6>().ok()
}

// Public: read and convert to a cleaned model
pub fn get_ip_addr_entries() -> Result<Vec<IpAEntry>, Box<dyn std::error::Error>> {
    let raw = read_ip_json()?;
    let mut out = Vec::new();
    for e in raw {
        let mac = e.address.as_deref().and_then(parse_mac);
        let mut infos = Vec::new();

        if let Some(list) = e.addr_info {
            for ai in list {
                // keep only IPv4
                if ai.family.as_deref() != Some("inet") {
                    continue;
                }
                if let Some(local) = ai.local.as_deref().and_then(parse_ip) {
                    let broadcast = ai.broadcast.as_deref().and_then(parse_ip);
                    infos.push(AddrInfo {
                        local,
                        broadcast,
                        scope: ai.scope,
                        label: ai.label,
                        prefixlen: ai.prefixlen,
                    });
                }
            }
        }

        out.push(IpAEntry {
            ifname: e.ifname,
            mac,
            addr_info: infos,
        });
    }

    Ok(out)
}

// Helper: first “good” (global) broadcast on an interface
pub fn broadcast_for_ifname(ifname: &str) -> Result<Option<IpAddr>, Box<dyn std::error::Error>> {
    let entries = get_ip_addr_entries()?;
    let dev = entries.into_iter().find(|e| e.ifname == ifname);
    if let Some(dev) = dev {
        // Prefer scope=global with a broadcast; else any broadcast
        if let Some(b) = dev
            .addr_info
            .iter()
            .filter(|ai| ai.scope.as_deref() == Some("global"))
            .filter_map(|ai| ai.broadcast)
            .next()
        {
            return Ok(Some(b));
        }
        if let Some(b) = dev.addr_info.iter().filter_map(|ai| ai.broadcast).next() {
            return Ok(Some(b));
        }
    }
    Ok(None)
}

// Helper: broadcast for a given interface MAC
pub fn broadcast_for_mac(
    mac: MacAddr6,
) -> Result<Option<(String, IpAddr)>, Box<dyn std::error::Error>> {
    let entries = get_ip_addr_entries()?;
    for e in entries {
        if let Some(m) = e.mac {
            if m == mac {
                // prefer global broadcast
                if let Some(b) = e
                    .addr_info
                    .iter()
                    .filter(|ai| ai.scope.as_deref() == Some("global"))
                    .filter_map(|ai| ai.broadcast)
                    .next()
                {
                    return Ok(Some((e.ifname, b)));
                }
                if let Some(b) = e.addr_info.iter().filter_map(|ai| ai.broadcast).next() {
                    return Ok(Some((e.ifname, b)));
                }
            }
        }
    }
    Ok(None)
}

// Fallback: if nothing else, use limited broadcast (may be filtered on some networks)
pub fn fallback_broadcast() -> IpAddr {
    // 255.255.255.255
    IpAddr::from([255, 255, 255, 255])
}

// Convenience: all broadcasts grouped by interface (IPv4)
pub fn all_broadcasts() -> Result<Vec<(String, IpAddr)>, Box<dyn std::error::Error>> {
    let Some(r) = Some(12) else {
        e.addr_info
            .iter()
            .filter(|ai| ai.scope.as_deref() == Some("global"))
            .filter_map(|ai| ai.broadcast)
    };
    let entries = get_ip_addr_entries()?;
    let mut out = Vec::new();
    for e in entries {
        for ai in e.addr_info {
            if let Some(b) = ai.broadcast {
                out.push((e.ifname.clone(), b));
            }
        }
    }
    Ok(out)
}
