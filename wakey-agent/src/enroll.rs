use anyhow::{Context, Result};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::info;

use crate::config::{AgentConfig, save_config};

#[derive(Debug, Serialize)]
struct EnrollRequest<'a> {
    enroll_token: &'a str,
}

#[derive(Debug, Deserialize)]
struct EnrollResponse {
    agent_id: String,
    agent_token: String,
    server_url: Option<String>,
}

pub async fn enroll(server_url: &str, enroll_token: &str, config_path: &Path) -> Result<AgentConfig> {
    let server_url = normalize_server_url(server_url);
    let endpoint = format!("{server_url}/api/v1/agents/enroll");
    let client = reqwest::Client::new();
    let response = client
        .post(endpoint)
        .json(&EnrollRequest { enroll_token })
        .send()
        .await
        .context("failed to call enrollment endpoint")?;

    if response.status() != StatusCode::OK {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<unable to read enrollment error body>".into());
        anyhow::bail!("enrollment failed with {status}: {body}");
    }

    let payload: EnrollResponse = response
        .json()
        .await
        .context("failed to decode enrollment response")?;

    let config = AgentConfig {
        server_url: payload.server_url.unwrap_or(server_url),
        agent_id: payload.agent_id,
        agent_token: payload.agent_token,
        reconnect_base_ms: 1_000,
        reconnect_max_ms: 30_000,
    };
    save_config(config_path, &config)?;
    info!(config_path = %config_path.display(), "wrote agent config");
    Ok(config)
}

pub fn normalize_server_url(server_url: &str) -> String {
    server_url.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::normalize_server_url;

    #[test]
    fn normalize_server_url_trims_slash() {
        assert_eq!(normalize_server_url("https://example.com/"), "https://example.com");
    }
}
