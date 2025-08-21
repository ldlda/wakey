pub const LDA_MACS: [[u8; 6]; 2] = [
    [0x04, 0x7c, 0x16, 0x79, 0x6d, 0xee],
    [0xbc, 0x09, 0x1b, 0xec, 0x65, 0xd0],
];


pub async fn wake(_machine_name: &str) -> io::Result<()> {
    let suh = UdpSocket::bind("0.0.0.0:0").await?;
    suh.set_broadcast(true)?;
    for mac in LDA_MACS {
        let pac: Vec<u8> = iter::once([0xff; 6])
            .chain(iter::repeat_n(mac, 16))
            .flatten()
            .collect();
        suh.send_to(&pac, "192.168.100.255:9").await?;
    }
    Ok(())
}
use std::{iter, time::Duration};

use tokio::{
    io,
    net::{TcpStream, ToSocketAddrs, UdpSocket},
    time::timeout,
};

/// generic so you can do "123.45.67.89:22" as an input
pub async fn ping_ip<T: ToSocketAddrs>(addr: T) -> bool {
    // thanks cahtgpt
    timeout(Duration::from_secs(1), TcpStream::connect(addr))
        .await
        .is_ok()
}


// pub async fn get_ips(macs: &[&[u8;6]]) -> Vec<IpAddr> {
//     /* 
//         root@AP-AX3000CV2-0A55:~# ip neigh show 192.168.100.94
//         192.168.100.94 dev eth1.6  FAILED
//         192.168.100.94 dev br-lan lladdr 04:7c:16:79:6d:ee REACHABLE
//         192.168.100.94 dev eth1.7  FAILED
//         192.168.100.94 dev eth1.5  FAILED
//      */

// }