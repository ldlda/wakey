use std::{io, process::Output};

use anyhow::Context;

use super::LinkOutput;

pub async fn get(dev: Option<&str>) -> anyhow::Result<Vec<LinkOutput>> {
    let output = _get(dev).await.context("Can not run command")?;

    if !output.status.success() {
        anyhow::bail!(String::from_utf8_lossy(&output.stderr).into_owned());
    }

    serde_json::from_slice(&output.stdout).context("Deserialize failed")
}

pub async fn _get(dev: Option<&str>) -> io::Result<Output> {
    let mut cmd = tokio::process::Command::new("ip");
    cmd.args(["-j", "link", "show"]);

    if let Some(d) = dev {
        cmd.arg(d);
    }

    cmd.output().await
}
