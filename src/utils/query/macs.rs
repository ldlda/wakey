use macaddr::MacAddr;

use crate::arpparse::{self, IpNeighLine, NUDState};
use crate::utils::{
    cmd::exec_command,
    error::{self, Result},
};
use std::collections::HashSet;
use std::net::IpAddr;

pub async fn get_ips(machine_name: &str) -> Result<impl Iterator<Item = IpAddr>> {
    Ok(tokio::net::lookup_host((machine_name, 0))
        .await
        .map_err(|e| error::Error::DnsResolve {
            name: machine_name.to_string(),
            source: e,
        })?
        .map(|c| c.ip()))
}

// #[deprecated(
//     since = "0.1.5",
//     note = "just call once everything with get mac
//     and then filter it bro WHY DO YOU EVEN DO TS"
// )]
// good now
//
// Current logic: When filtering by exactly 1 dev/mac, exclude entries missing that field.
// This is because missing dev/mac usually means the entry is incomplete/transient.
//
// when there is only one MACs (getmac got some), the result will not have them fields.
// so there are three cases:
//
// 1. dont got nothing: take all of them (macset.is_empty())
// 2. exactly one: pre-filtered by ip, everything matches,
// devset.len() != 1 returns false, but then it works????
// OH THIS fuckass code i added it in the get_mac
// 3. devset.len() > 1. if none then absolutely not match,
// if some then check with the set; thats normal
pub async fn get_macs(
    machine_names: &[impl AsRef<str>],
    ips: &[IpAddr],
    devs: &[impl AsRef<str>],
    state: &[NUDState],
    macs: &[MacAddr],
) -> Result<Vec<IpNeighLine>> {
    let mut ip_set: HashSet<IpAddr> = ips.iter().map(|ip| ip.to_canonical()).collect();
    let ip_m: HashSet<IpAddr> = futures::future::try_join_all(
        machine_names
            .iter()
            .map(|c| async { get_ips(c.as_ref()).await }),
    )
    .await?
    .into_iter()
    .flatten()
    .collect();
    let ip_all = if ip_set.is_empty() && ip_m.is_empty() {
        None
    } else if ip_set.is_empty() {
        Some(ip_m)
    } else if ip_m.is_empty() {
        Some(ip_set)
    } else {
        Some({
            ip_set.retain(|c| ip_m.contains(c)); // inline AHHH
            ip_set
        })
    };

    let opt_dev = if devs.len() > 1 {
        None
    } else {
        devs.iter().next().map(AsRef::as_ref)
    };

    let run_one = |to_ip: Option<IpAddr>| get_mac(to_ip, opt_dev, state);

    let mut ip_filtered = if let Some(something) = ip_all {
        if something.len() == 1 {
            run_one(something.into_iter().next()).await?
        } else {
            run_one(None)
                .await?
                .into_iter()
                .filter(|c| something.contains(&c.ip))
                .collect()
        }
    } else {
        run_one(None).await?
    };

    // Apply additional filters if any were provided
    if !devs.is_empty() || !macs.is_empty() || !state.is_empty() {
        let devset: HashSet<_> = devs.iter().map(AsRef::as_ref).collect();
        let macset: HashSet<_> = macs.iter().collect();

        ip_filtered.retain(|entry| {
            // Dev filter: if we're filtering by dev, entry must have a dev AND it must be in the set
            let dev_ok =
                devset.is_empty() || entry.dev.as_deref().is_some_and(|d| devset.contains(d));

            // MAC filter: if we're filtering by MAC, entry must have a MAC AND it must be in the set
            let mac_ok = macset.is_empty() || entry.mac.is_some_and(|m| macset.contains(&m));

            dev_ok && mac_ok
        })
    };
    Ok(ip_filtered)
}

/// the atomic get_macs. handle ONE thing only.
pub async fn get_mac(
    ip: Option<IpAddr>,
    dev: Option<&str>,
    state: &[NUDState],
) -> Result<Vec<IpNeighLine>> {
    let mut args: Vec<String> = vec!["neigh".into(), "show".into()];
    if let Some(ip) = ip {
        args.push("to".into());
        args.push(ip.to_string());
    }
    if let Some(d) = dev {
        args.push("dev".into());
        args.push(d.to_string());
    }
    for nud in state {
        args.push("nud".into());
        args.push(nud.as_ip_neigh_arg().into());
    }
    let cmd = "ip";
    let out = exec_command(cmd, args.iter().map(String::as_str).collect::<Vec<_>>()).await?;
    if !out.status.success() {
        return Err(error::Error::CommandFailed {
            cmd,
            args,
            status: out.status.code(),
            stderr: String::from_utf8_lossy(&out.stderr).into(),
        });
    }
    let lines = String::from_utf8_lossy(&out.stdout);
    let parsed = lines.lines().flat_map(arpparse::parse_ip_neigh_line);
    let rows: Vec<IpNeighLine> = if let Some(d) = dev {
        parsed.map(IpNeighLine::with_dev(d)).collect()
    } else {
        parsed.collect()
    };
    Ok(rows)
}

/*
/// get macs where you just run ip neigh then rust handles the filtering (faster than get mac)
pub async fn get_macs_rust(
    machine_names: Option<&str>,
    ips: Option<&[IpAddr]>,
    devs: Option<&[&str]>,
    states: Option<&[NUDState]>,
) -> Result<Vec<IpNeighLine>> {
    let mut ip_map: HashSet<IpAddr> = HashSet::new();
    let mut machine_map = HashSet::new();
    if let Some(ips) = ips {
        ip_map.extend(ips);
    }
    if let Some(m) = machine_names {
        machine_map.extend(get_ips(m).await.into_iter().flatten());
    }

    let real = match (ip_map.is_empty(), machine_map.is_empty()) {
        (true, true) => HashSet::new(),
        (true, false) => machine_map,
        (false, true) => ip_map,
        (false, false) => ip_map.intersection(&machine_map).copied().collect(),
    };

    Ok(vec![])
}

pub async fn _get_machines(m: &[&str]) -> Vec<IpAddr> {
    let futs = m.iter().map(|c| get_ips(c));
    futures::future::join_all(futs)
        .await
        .into_iter()
        .flat_map(|f| f.into_iter().flatten())
        .collect()
} */
