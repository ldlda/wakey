// this ENTIRE file is redundant... or?

use std::{net::IpAddr, time::Duration};

use tokio::{
    net::{TcpStream, ToSocketAddrs},
    time::timeout,
};

use crate::{legacy::arpparse::NUDState, utils::query::get_mac};

pub async fn _ping_ip<T: ToSocketAddrs>(addr: T) -> bool {
    timeout(Duration::from_secs(1), TcpStream::connect(addr))
        .await
        .is_ok()
}
pub async fn _ping_ip_2<T: ToSocketAddrs>(_addr: T) -> bool {
    todo!("use icmp")
}

pub async fn _ping_ip_3<T: Into<IpAddr>>(addr: T) -> u8 {
    match get_mac(Some(addr.into()), None, &[] as &[NUDState]).await {
        Err(_) => 0,
        Ok(l) => l
            .into_iter()
            .map(|e| e.state)
            .max()
            .map(NUDState::rank)
            .unwrap_or_default(),
    }
}
