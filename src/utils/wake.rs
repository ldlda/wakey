pub async fn wake_one(
    sock: &tokio::net::UdpSocket,
    t: wakey_linux::wake::CompleteWakeTarget,
) -> wakey_core::WakeTargetResult {
    wakey_linux::wake::wake_one(sock, t).await
}

pub use wakey_linux::wake::{CompleteWakeTarget as WakeTarget, wake_many as _wake_multi};
pub use wakey_core::{WakeStatus, WakeTargetResult};
