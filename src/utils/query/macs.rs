use crate::arpparse::{self, IpNeighLine, NUDState};
use crate::utils::{
    cmd::exec_command,
    error::{self, Error, Result},
};
use macaddr::MacAddr;
use std::collections::HashSet;
use std::net::IpAddr;

pub async fn get_ips(machine_name: &str) -> error::Result<impl Iterator<Item = IpAddr>> {
    Ok(tokio::net::lookup_host((machine_name, 0))
        .await
        .map_err(|e| error::Error::DnsResolve {
            name: machine_name.to_string(),
            source: e,
        })?
        .map(|c| c.ip()))
}

pub async fn _get_macs_2_1(machine_name: &str) -> Result<HashSet<(IpAddr, MacAddr, NUDState)>> {
    Ok(get_macs_1(machine_name)
        .await?
        .into_iter()
        .filter_map(
            |IpNeighLine {
                 ip,
                 dev: _,
                 mac,
                 state,
             }| mac.map(|mac| (ip, mac, state)),
        )
        .collect())
}

pub async fn get_macs_2_mac(machine_name: &str) -> Result<HashSet<MacAddr>> {
    Ok(get_macs_1(machine_name)
        .await?
        .into_iter()
        .filter_map(
            |IpNeighLine {
                 ip: _,
                 dev: _,
                 mac,
                 state: _,
             }| mac,
        )
        .collect())
}

pub async fn get_macs_1(machine_name: &str) -> Result<Vec<arpparse::IpNeighLine>> {
    let dev = "br-lan";
    let ips = get_ips(machine_name).await?;
    let futures = ips.map(|ip| {
        let ip = ip.to_canonical();
        async move {
            let cmd = "ip";
            let args = ["neigh", "show", "to", &ip.to_string(), "dev", dev];
            let o = exec_command(cmd, args).await?;
            if !o.status.success() {
                return Err(Error::CommandFailed {
                    cmd,
                    args: args.iter().map(ToString::to_string).collect(),
                    status: o.status.code(),
                    stderr: String::from_utf8_lossy(&o.stderr).into(),
                });
            };
            Ok(String::from_utf8_lossy(&o.stdout)
                .lines()
                .flat_map(arpparse::parse_ip_neigh_line)
                .collect::<Vec<_>>())
        }
    });
    let res = futures::future::try_join_all(futures).await?;
    Ok(res.into_iter().flatten().collect())
}

pub async fn get_macs(
    machine_name: Option<&str>,
    ips: Option<&[IpAddr]>,
    dev: Option<&str>,
    state: Option<NUDState>,
) -> Result<Vec<IpNeighLine>> {
    let ip_list: Option<Vec<IpAddr>> =
        ips.map(|slice| slice.iter().map(|ip| ip.to_canonical()).collect());
    let ip_list = match (ip_list, machine_name) {
        (Some(list), _) => list,
        (None, Some(name)) => get_ips(name).await?.collect(),
        (None, None) => Vec::new(),
    };
    let run_one = |to_ip: Option<IpAddr>| get_mac(to_ip, dev, state);
    if !ip_list.is_empty() {
        let futures = ip_list.into_iter().map(|ip| run_one(Some(ip)));
        let res = futures::future::try_join_all(futures).await?;
        Ok(res.into_iter().flatten().collect())
    } else {
        run_one(None).await
    }
}

/// the atomic get_macs. handle ONE thing only.
pub async fn get_mac(
    ip: Option<IpAddr>,
    dev: Option<&str>,
    state: Option<NUDState>,
) -> error::Result<Vec<IpNeighLine>> {
    let mut args: Vec<String> = vec!["neigh".into(), "show".into()];
    if let Some(ip) = ip {
        args.push("to".into());
        args.push(ip.to_string());
    }
    if let Some(d) = dev {
        args.push("dev".into());
        args.push(d.to_string());
    }
    if let Some(nud) = state {
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
