pub async fn wake_one(
    sock: &tokio::net::UdpSocket,
    t: wakey_linux::wake::CompleteWakeTarget,
) -> wakey_core::WakeTargetResult {
    wakey_linux::wake::wake_one(sock, t).await
}
