pub const LDA_MACS: [[u8; 6]; 2] = [
    // ether
    [0x04, 0x7c, 0x16, 0x79, 0x6d, 0xee],
    // wifi
    [0xbc, 0x09, 0x1b, 0xec, 0x65, 0xd0],
];

/// is it time to lookup host lda.lan for this...
pub static LDA_MACS_2: LazyLock<[MacAddr; 2]> = LazyLock::new(|| LDA_MACS.map(MacAddr::from));
pub async fn wake(machine_name: &str) -> io::Result<u32> {
    let suh = UdpSocket::bind("0.0.0.0:0").await?;
    suh.set_broadcast(true)?;
    let mut macs = get_macs_2_1(machine_name).await.unwrap_or_default();
    macs.extend(LDA_MACS_2.map(|m| ([192, 168, 100, 255].into(), m)));
    let len = macs.len() as u32;
    // count - count fail
    let mut count_fail = 0;
    for (ip, mac) in macs.into_iter() {
        // let Some(mac) = mac else {
        //     continue;
        // };
        let mb = mac.as_bytes();
        let start = [0xff; 6];
        let pac: Vec<u8> = iter::once(start.as_slice())
            .chain(iter::repeat_n(mb, 16))
            // what happens here?
            .flatten()
            .copied()
            .collect();
        suh.send_to(&pac, (ip, 9))
            .await
            .inspect_err(|e| {
                eprintln!("ping error: {e}");
                count_fail += 1;
            })
            .ok()
            .inspect(|f| {
                if *f < (6 + 16 * 6) {
                    // rare ass code path
                    eprintln!("not complete transmission");
                    count_fail += 1;
                }
            }); // type shit
        // suh.send_to(&pac, "192.168.100.255:9").await?; // type good
        // suh.send_to(&pac, "255.255.255.255:9").await?; // type ass; this doesnt work somehow.
    }
    Ok(len - count_fail)
}
use std::{collections::HashSet, iter, net::IpAddr, str::FromStr, sync::LazyLock, time::Duration};

use macaddr::MacAddr;
use tokio::{
    io,
    net::{TcpStream, ToSocketAddrs, UdpSocket},
    time::timeout,
};

use crate::arpparse::{self, IpNeighLine};

/// generic so you can do "123.45.67.89:22" or "lda.lan:22" as an input
// this is so bad
pub async fn ping_ip<T: ToSocketAddrs>(addr: T) -> bool {
    timeout(Duration::from_secs(1), TcpStream::connect(addr))
        .await
        .is_ok()
}
pub async fn _ping_ip_2<T: ToSocketAddrs>(_addr: T) -> bool {
    todo!("use icmp")
}

/// this is because i like [`IpAddr`] more than [`SocketAddr`](std::net::SocketAddr)
pub async fn get_ips(machine_name: &str) -> io::Result<Vec<IpAddr>> {
    Ok(tokio::net::lookup_host((machine_name, 0))
        .await?
        .map(|c| c.ip())
        .collect())
}
// not used lets go
pub async fn get_macs(machine_name: &str) -> io::Result<Vec<(IpAddr, MacAddr)>> {
    let ips = get_ips(machine_name).await?;
    let futures = ips
        .iter()
        // .filter(|f| f.is_ipv4())
        .map(|ip| {
            let ip = ip.to_canonical();
            async move {
                let o = exec_command("ip", ["neigh", "show", "to", &ip.to_string()])
                    .await
                    .ok()?;
                o.status.success().then(|| {
                    let stdout = String::from_utf8_lossy(&o.stdout);
                    stdout
                        .lines()
                        .filter_map(|line| {
                            let mut parts = line.split_whitespace();
                            parts.find(|&x| x == "lladdr")?;
                            parts
                                .next()
                                .and_then(|mac| MacAddr::from_str(mac).ok()) // 100% correct because its ip neigh brother they know how to code.
                                .map(|mac| (ip, mac))
                        })
                        .collect::<Vec<_>>()
                })
            }
        });
    let results = futures::future::join_all(futures).await;
    Ok(results.into_iter().flatten().flatten().collect())
}

pub async fn get_macs_1(machine_name: &str) -> io::Result<Vec<arpparse::IpNeighLine>> {
    let ips = get_ips(machine_name).await?;
    let futures = ips.iter().map(|ip| {
        let ip = ip.to_canonical();
        async move {
            let o = exec_command("ip", ["neigh", "show", "to", &ip.to_string()]).await?;
            if !o.status.success() {
                return Err(io::Error::other(format!(
                    "`ip neigh` failed for {ip} (status: {st}): {err}",
                    st = o.status,
                    err = String::from_utf8_lossy(&o.stderr),
                )));
            };
            Ok(String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(arpparse::parse_ip_neigh_line)
                .collect::<Vec<_>>())
        }
    });
    let res = futures::future::try_join_all(futures).await?; //  async move block errs.
    Ok(res
        .into_iter()
        .flatten() /* resolve double vec */
        .flatten() /* drop parse errors */
        .collect())
}

async fn exec_command<S: AsRef<std::ffi::OsStr>>(
    cmd: S,
    args: impl IntoIterator<Item = S>,
) -> io::Result<std::process::Output> {
    let mut u = tokio::process::Command::new(cmd);
    u.args(args);
    u.output().await
}

pub async fn get_macs_2(machine_name: &str) -> io::Result<HashSet<(IpAddr, MacAddr)>> {
    Ok(get_macs(machine_name).await?.into_iter().collect())
}

pub async fn get_macs_2_1(machine_name: &str) -> io::Result<HashSet<(IpAddr, MacAddr)>> {
    Ok(get_macs_1(machine_name).await?.into_iter().filter_map(|IpNeighLine { ip, dev: _, mac, state: _ }| mac.map(|mac| (ip, mac))).collect())
}

pub fn to_arr(macstr: &str) -> Option<[u8; 6]> {
    let mut this = [0u8; 6];
    let c: Vec<_> = macstr.split(':').collect();
    (c.len() == 6).then(|| {
        for (n, h) in this.iter_mut().zip(c) {
            *n = u8::from_str_radix(h, 16).ok()?;
        }
        Some(this)
    })?
}

pub fn back_to_str(thing: &[u8; 6]) -> String {
    thing
        .iter()
        .map(|n| format!("{n:02x}"))
        .collect::<Vec<String>>()
        .join(":")
}
