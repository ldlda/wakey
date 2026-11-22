use anyhow::Result;

use crate::{cli::InitConfigArgs, config, serve};

pub(crate) fn run(args: InitConfigArgs) -> Result<()> {
    if let Some(config) = &args.config
        && config.exists()
        && !args.force
    {
        anyhow::bail!(
            "config {} already exists; re-run with --force to overwrite",
            config.display()
        );
    }

    let mut cfg = if let Some(from_config) = &args.from_config {
        config::load_config(from_config)?
    } else {
        (*config::DEFAULT_CONFIG).clone()
    };

    if let Some(server_url) = args.server_url {
        cfg.server_url = server_url;
    }
    if let Some(agent_id) = args.agent_id {
        cfg.agent_id = agent_id;
    }
    if let Some(agent_token) = args.agent_token {
        cfg.agent_token = agent_token;
    }
    if let Some(days) = args.observation_retention_days {
        cfg.observation_retention = std::time::Duration::from_secs(
            days.checked_mul(86_400)
                .ok_or_else(|| anyhow::anyhow!("observation retention is too large"))?,
        );
    }

    if let Some(path) = &args.config {
        config::save_config(path, &cfg)?;
        println!("config={}", path.display());
        println!("run={}", serve::command_for_config(path));
    } else {
        let rendered = toml::to_string_pretty(&cfg)?;
        print!("{}", rendered);
    }
    Ok(())
}
