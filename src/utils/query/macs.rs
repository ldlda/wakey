use crate::arpparse::{self, IpNeighLine, NUDState};
use crate::utils::{
    cmd::exec_command,
    error::{self, Error, Result},
};
use macaddr::MacAddr;
use std::collections::HashSet;
use std::net::IpAddr;

pub async fn get_ips(machine_name: &str) -> error::Result<Vec<IpAddr>> {
    let it = tokio::net::lookup_host((machine_name, 0))
        .await
        .map_err(|e| error::Error::DnsResolve {
            name: machine_name.to_string(),
            source: e,
        })?;
    Ok(it.map(|c| c.ip()).collect())
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
    let futures = ips.iter().map(|ip| {
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
        ips.map(|slice| slice.iter().copied().map(|ip| ip.to_canonical()).collect());
    let ip_list = match (ip_list, machine_name) {
        (Some(list), _) => list,
        (None, Some(name)) => get_ips(name).await?.into_iter().collect(),
        (None, None) => Vec::new(),
    };
    let nud_arg = state.map(NUDState::as_ip_neigh_arg);
    let run_one = |to_ip: Option<IpAddr>| async move {
        let mut args: Vec<String> = vec!["neigh".into(), "show".into()];
        if let Some(ip) = to_ip {
            args.push("to".into());
            args.push(ip.to_string());
        }
        if let Some(d) = dev {
            args.push("dev".into());
            args.push(d.to_string());
        }
        if let Some(nud) = nud_arg {
            args.push("nud".into());
            args.push(nud.to_string());
        }
        let o = exec_command("ip", args.iter().map(String::as_str).collect::<Vec<_>>()).await?;
        if !o.status.success() {
            return Err(Error::CommandFailed {
                cmd: "ip",
                args,
                status: o.status.code(),
                stderr: String::from_utf8_lossy(&o.stderr).into(),
            });
        }
        let lines = String::from_utf8_lossy(&o.stdout);
        let parsed = lines.lines().flat_map(arpparse::parse_ip_neigh_line);
        let rows: Vec<IpNeighLine> = if let Some(d) = dev {
            parsed.map(IpNeighLine::with_dev(d)).collect()
        } else {
            parsed.collect()
        };
        Ok::<Vec<IpNeighLine>, error::Error>(rows)
    };
    if !ip_list.is_empty() {
        let futures = ip_list.into_iter().map(|ip| run_one(Some(ip)));
        let res = futures::future::try_join_all(futures).await?;
        Ok(res.into_iter().flatten().collect())
    } else {
        run_one(None).await
    }
}
