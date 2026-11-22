use anyhow::Result;

use crate::protocol::{CommandResult, PingRequest};

#[cfg(target_os = "linux")]
pub async fn probe(req: PingRequest) -> Result<CommandResult> {
    use anyhow::Context;
    use std::{net::IpAddr, time::Duration};
    use surge_ping::{Client, Config, ICMP, PingIdentifier, PingSequence, SurgeError};

    let (kind, scope) = match req.ip {
        IpAddr::V4(_) => (ICMP::V4, None),
        IpAddr::V6(ip) if ip.is_unicast_link_local() => {
            // The observed interface disambiguates link-local addresses.
            let interface = req
                .interface
                .as_deref()
                .context("link-local IPv6 requires an interface")?;
            let scope = nix::net::if_::if_nametoindex(interface)
                .context("could not resolve ping interface")?;
            (ICMP::V6, Some(scope))
        }
        IpAddr::V6(_) => (ICMP::V6, None),
    };
    let client = Client::new(&Config::builder().kind(kind).build())
        .map_err(|err| anyhow::anyhow!("could not open ICMP socket: {err}"))?;
    let mut pinger = client.pinger(req.ip, PingIdentifier(rand::random())).await;
    if let Some(scope) = scope {
        pinger.scope_id(scope);
    }
    pinger.timeout(Duration::from_secs(2));
    let rtt_ms = match pinger.ping(PingSequence(0), &[0; 32]).await {
        Ok((_, rtt)) => Some(rtt.as_secs_f64() * 1000.0),
        Err(SurgeError::Timeout { .. }) => None,
        Err(err) => return Err(anyhow::anyhow!("ICMP probe failed: {err}")),
    };
    Ok(CommandResult::Ping {
        ip: req.ip,
        replied: rtt_ms.is_some(),
        rtt_ms,
    })
}

#[cfg(not(target_os = "linux"))]
pub async fn probe(_req: PingRequest) -> Result<CommandResult> {
    anyhow::bail!("ping is only supported by Linux agents")
}

#[cfg(all(test, target_os = "linux"))]
#[tokio::test]
#[ignore = "requires IPv4 and IPv6 ICMP socket access"]
async fn loopback_returns_icmp_replies() {
    for ip in ["127.0.0.1", "::1"] {
        let result = probe(PingRequest {
            ip: ip.parse().unwrap(),
            interface: None,
        })
        .await
        .unwrap();
        assert!(matches!(
            result,
            CommandResult::Ping {
                replied: true,
                rtt_ms: Some(_),
                ..
            }
        ));
    }
}
