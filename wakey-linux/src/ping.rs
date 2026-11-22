use std::{net::IpAddr, time::Duration};

use anyhow::{Context, Result};
use surge_ping::{AsyncSocket, Client, Config, ICMP, PingIdentifier, PingSequence, SurgeError};

fn icmp_config(kind: ICMP) -> Config {
    Config::builder().kind(kind).build()
}

/// Check socket permissions without sending traffic. One usable address family is enough.
pub fn is_available() -> bool {
    let v4 = AsyncSocket::new(&icmp_config(ICMP::V4));
    let v6 = AsyncSocket::new(&icmp_config(ICMP::V6));
    match (v4, v6) {
        (Err(v4), Err(v6)) => {
            tracing::debug!(ipv4_error = %v4, ipv6_error = %v6, "no usable ICMP sockets");
            false
        }
        _ => true,
    }
}

/// Send one ICMP echo and return the round-trip time, or `None` after two seconds.
/// Link-local IPv6 requires the interface that owns the address's scope.
pub async fn probe(ip: IpAddr, interface: Option<&str>) -> Result<Option<Duration>> {
    let (kind, scope) = match ip {
        IpAddr::V4(_) => (ICMP::V4, None),
        IpAddr::V6(ip) if ip.is_unicast_link_local() => {
            let interface = interface.context("link-local IPv6 requires an interface")?;
            let scope = nix::net::if_::if_nametoindex(interface)
                .context("could not resolve ping interface")?;
            (ICMP::V6, Some(scope))
        }
        IpAddr::V6(_) => (ICMP::V6, None),
    };
    let client = Client::new(&icmp_config(kind)).context("could not open ICMP socket")?;
    let mut pinger = client.pinger(ip, PingIdentifier(rand::random())).await;
    if let Some(scope) = scope {
        pinger.scope_id(scope);
    }
    pinger.timeout(Duration::from_secs(2));
    match pinger.ping(PingSequence(0), &[0; 32]).await {
        Ok((_, rtt)) => Ok(Some(rtt)),
        Err(SurgeError::Timeout { .. }) => Ok(None),
        Err(err) => Err(err).context("ICMP probe failed"),
    }
}

#[cfg(test)]
#[tokio::test]
#[ignore = "requires IPv4 and IPv6 ICMP socket access"]
async fn loopback_returns_icmp_replies() {
    assert!(is_available());
    for ip in ["127.0.0.1", "::1"] {
        assert!(probe(ip.parse().unwrap(), None).await.unwrap().is_some());
    }
}
