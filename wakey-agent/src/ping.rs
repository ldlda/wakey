use anyhow::Result;

use crate::protocol::{CommandResult, PingRequest};

pub(crate) fn is_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        wakey_linux::ping::is_available()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

#[cfg(target_os = "linux")]
pub async fn probe(req: PingRequest) -> Result<CommandResult> {
    let rtt = wakey_linux::ping::probe(req.ip, req.interface.as_deref()).await?;
    Ok(CommandResult::Ping {
        ip: req.ip,
        replied: rtt.is_some(),
        rtt_ms: rtt.map(|duration| duration.as_secs_f64() * 1000.0),
    })
}

#[cfg(not(target_os = "linux"))]
pub async fn probe(_req: PingRequest) -> Result<CommandResult> {
    anyhow::bail!("ping is only supported by Linux agents")
}
