use std::{collections::HashSet, net::IpAddr};

use macaddr::MacAddr;
use tokio::io;

use crate::{
    arpparse::{self, IpNeighLine, NUDState},
    utils::{cmd::exec_command, get_ips},
};

pub async fn get_macs_2_1(machine_name: &str) -> io::Result<HashSet<(IpAddr, MacAddr, NUDState)>> {
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
pub async fn get_macs_2_mac(machine_name: &str) -> io::Result<HashSet<MacAddr>> {
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

pub async fn get_macs_1(machine_name: &str) -> io::Result<Vec<arpparse::IpNeighLine>> {
    let dev = "br-lan";
    let ips = get_ips(machine_name).await?;
    let futures = ips.iter().map(|ip| {
        let ip = ip.to_canonical();
        async move {
            let o =
                exec_command("ip", ["neigh", "show", "to", &ip.to_string(), "dev", dev]).await?;
            if !o.status.success() {
                return Err(io::Error::other(format!(
                    "`ip neigh` failed for {ip} (status: {st}): {err}",
                    st = o.status,
                    err = String::from_utf8_lossy(&o.stderr),
                )));
            };
            Ok(String::from_utf8_lossy(&o.stdout)
                .lines()
                .flat_map(arpparse::parse_ip_neigh_line)
                // .map(IpNeighLine::with_dev(dev)) // this could be after flatmap up there
                .collect::<Vec<_>>())
        }
    });
    let res = futures::future::try_join_all(futures).await?; //  async move block errs.
    Ok(res
        .into_iter()
        .flatten() /* resolve double vec */
        // .flatten() /* drop parse errors (flat_map cleared) */
        .collect())
}

pub async fn get_macs(
    machine_name: Option<&str>,
    ips: Option<&[IpAddr]>,
    dev: Option<&str>,
    state: Option<NUDState>,
) -> io::Result<Vec<IpNeighLine>> {
    // Collect IPs early (before any await) to avoid holding generics across await points
    let ip_list: Option<Vec<IpAddr>> = ips.map(|slice| {
        slice
            .iter()
            .copied()
            .map(|ip| ip.to_canonical())
            .collect()
    });

    // Resolve by machine name if no IPs provided but we have a name
    let ip_list = match (ip_list, machine_name) {
        (Some(list), _) => list,
        (None, Some(name)) => get_ips(name).await?.into_iter().collect(),
        (None, None) => Vec::new(),
    };

    // Helper to convert NUDState to the string expected by `ip neigh`
    let nud_arg = state.map(NUDState::as_ip_neigh_arg);
    // let nud_arg = Rc::new(state.map(|s| s.to_string().to_lowercase()));
    // Build a closure to run one `ip neigh` invocation and parse results
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

        let o = exec_command("ip", args.iter().map(String::as_str).collect::<Vec<_>>()).await?; // hope to rustc that it knows how to unfuck ts
        if !o.status.success() {
            return Err(io::Error::other(format!(
                "`ip neigh` failed{ctx} (status: {st}): {err}",
                ctx = to_ip.map(|ip| format!(" for {ip}")).unwrap_or_default(),
                st = o.status,
                err = String::from_utf8_lossy(&o.stderr),
            )));
        }

        let lines = String::from_utf8_lossy(&o.stdout);
        // Parse lines and, if a specific dev filter was used, stamp that dev onto rows
        let parsed = lines.lines().flat_map(arpparse::parse_ip_neigh_line);
        let rows: Vec<IpNeighLine> = if let Some(d) = dev {
            parsed.map(IpNeighLine::with_dev(d)).collect()
        } else {
            parsed.collect()
        };
        Ok::<Vec<IpNeighLine>, io::Error>(rows)
    };

    // If we have specific IPs, query each; otherwise query the whole table once
    if !ip_list.is_empty() {
        let futures = ip_list.into_iter().map(|ip| run_one(Some(ip)));
        let res = futures::future::try_join_all(futures).await?;
        Ok(res.into_iter().flatten().collect())
    } else {
        run_one(None).await
    }
}
