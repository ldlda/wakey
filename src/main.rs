use std::{collections::HashSet, fs, iter, net::IpAddr};

use tokio::{io, net::UdpSocket};

#[tokio::main(flavor = "current_thread")]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    wake("lda.lan").await?;
    Ok(())
}

// async fn wake(machine_name: &str) -> io::Result<()> {
//     let sock = UdpSocket::bind("0.0.0.0:0").await?;
//     for (ip, mac) in find_mac(machine_name).await? {
//         // println!("yo drop the {ip}/{mac:?}");
//         let packet: Vec<u8> = iter::once([0xff; 6])
//             .chain(std::iter::repeat_n(mac, 16))
//             .flatten()
//             .collect();
//         // dbg!(&ip, &mac);
//         sock.send_to(&packet, (ip, 9)).await?;
//     }
//     Ok(())
// }

// async fn find_mac(machine_name: &str) -> io::Result<Vec<(IpAddr, [u8; 6])>> {
//     let arp = fs::read_to_string("/proc/net/arp")?;
//     let ips: HashSet<IpAddr> = tokio::net::lookup_host((machine_name, 0))
//         .await?
//         .map(|c| c.ip())
//         .collect();
//     // dbg!(&ips, &arp);

//     Ok(arp
//         .lines()
//         .skip(1)
//         .filter_map(|line| {
//             let mut parts = line.split_ascii_whitespace();
//             let ip: IpAddr = parts.next()?.parse().ok()?;
//             // println!("ip looks kinda good! {ip}");
//             if !ips.contains(&ip) {
//                 return None;
//             }
//             let mac_str = parts.nth(2)?;
//             let mut mac = [0u8; 6];
//             for (byte, hex) in mac.iter_mut().zip(mac_str.split(':')) {
//                 *byte = u8::from_str_radix(hex, 16).ok()?;
//             }
//             Some((ip, mac))
//         })
//         .collect())
// }

async fn wake(_machine_name: &str) -> io::Result<()> {
    let mac1 = [0x04, 0x7c, 0x16, 0x79, 0x6d, 0xee];
    let mac2 = [0xbc, 0x09, 0x1b, 0xec, 0x65, 0xd0];
    let suh = UdpSocket::bind("0.0.0.0:0").await?;
    suh.set_broadcast(true)?;
    for mac in [mac1, mac2] {
        let pac: Vec<u8> = iter::once([0xff; 6])
            .chain(iter::repeat_n(mac, 16))
            .flatten()
            .collect();
        suh.send_to(&pac, "192.168.100.255:9").await?;
    }
    Ok(())
}
