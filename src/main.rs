mod cli;

use clap::Parser;

#[cfg(not(target_os = "linux"))]
fn main() -> anyhow::Result<()> {
    Err(anyhow::anyhow!(
        "wakey only runs on Linux (router/OpenWrt-style targets)."
    ))
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli::run(cli::Cli::parse()).await
}
