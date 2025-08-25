// this ENTIRE file is redundant... or?

use std::time::Duration;

use tokio::{
    net::{TcpStream, ToSocketAddrs},
    time::timeout,
};

pub async fn _ping_ip<T: ToSocketAddrs>(addr: T) -> bool {
    timeout(Duration::from_secs(1), TcpStream::connect(addr))
        .await
        .is_ok()
}
pub async fn _ping_ip_2<T: ToSocketAddrs>(_addr: T) -> bool {
    todo!("use icmp")
}
