use std::{net::IpAddr, time::Duration};

use anyhow::{Context, Result, anyhow};
use surge_ping::{Client, Config, ICMP, PingIdentifier, PingSequence, SurgeError};

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
    let client = Client::new(&Config::builder().kind(kind).build())
        .map_err(|err| anyhow!("could not open ICMP socket: {err}"))?;
    let mut pinger = client.pinger(ip, PingIdentifier(rand::random())).await;
    if let Some(scope) = scope {
        pinger.scope_id(scope);
    }
    pinger.timeout(Duration::from_secs(2));
    match pinger.ping(PingSequence(0), &[0; 32]).await {
        Ok((_, rtt)) => Ok(Some(rtt)),
        Err(SurgeError::Timeout { .. }) => Ok(None),
        Err(err) => Err(anyhow!("ICMP probe failed: {err}")),
    }
}

#[cfg(test)]
#[tokio::test]
#[ignore = "requires IPv4 and IPv6 ICMP socket access"]
async fn loopback_returns_icmp_replies() {
    for ip in ["127.0.0.1", "::1"] {
        assert!(probe(ip.parse().unwrap(), None).await.unwrap().is_some());
    }
}
