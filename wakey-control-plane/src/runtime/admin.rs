use std::time::Duration;

use anyhow::{Context, Result};

use crate::api;
use crate::cli::{
    IssueEnrollTokenArgs, ListEnrollTokensArgs, RevokeEnrollTokenArgs, StateStatsArgs,
};
use crate::config;
use crate::state;

pub async fn issue_enroll_token(args: IssueEnrollTokenArgs) -> Result<()> {
    let settings = config::resolve_issue_token_settings(&args)?;

    if let Some(base) = settings.public_url.as_deref() {
        let ttl_seconds = settings.ttl.as_secs().max(1);
        let endpoint = format!(
            "{}?ttl_seconds={ttl_seconds}",
            config::issue_token_endpoint(&base)
        );
        tracing::info!(endpoint = %endpoint, "requesting live enroll token from running control-plane daemon");
        let client = reqwest::Client::new();

        let response = client
            .post(&endpoint)
            .send()
            .await
            .with_context(|| format!("failed to call live issuance endpoint {endpoint}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable error body>".to_string());
            anyhow::bail!("live issuance failed with {status}: {body}");
        }

        let payload: api::IssueEnrollTokenResponse = response
            .json()
            .await
            .context("failed to decode live issuance response")?;
        tracing::info!("received live enroll token response");

        println!("enroll_token={}", payload.enroll_token);
        println!("expires_at_unix={}", payload.expires_at_unix);
        println!(
            "agent_command=wakey-agent enroll --server-url {base} --enroll-token {}",
            payload.enroll_token
        );
        return Ok(());
    }

    tracing::info!(data_dir = %settings.data_dir.display(), state_file = %settings.state_file.display(), ttl_seconds = settings.ttl.as_secs(), "issuing enroll token via offline state file fallback");
    let store = state::Store::load_or_init(&settings.state_file, Vec::new(), settings.ttl)
        .await
        .with_context(|| {
            format!(
                "failed to initialize store {}",
                settings.state_file.display()
            )
        })?;
    let issued = store.issue_enroll_token(settings.ttl).await?;
    println!("enroll_token={}", issued.enroll_token);
    println!("expires_at_unix={}", issued.expires_at_unix);
    eprintln!(
        "note: token was written to {}. running daemon must reload state to see it",
        settings.state_file.display()
    );
    Ok(())
}

pub async fn list_enroll_tokens(args: ListEnrollTokensArgs) -> Result<()> {
    let settings = config::resolve_list_enroll_token_settings(&args)?;
    if let Some(base) = settings.public_url.as_deref() {
        let url = format!(
            "{}/api/v1/control/enroll-tokens?include_expired={}",
            base, args.include_expired
        );
        let response = reqwest::get(&url)
            .await
            .with_context(|| format!("failed to call {url}"))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable error body>".to_string());
            anyhow::bail!("live list-enroll-tokens failed with {status}: {body}");
        }
        let body: Vec<api::EnrollTokenStatus> = response
            .json()
            .await
            .context("failed to decode list-enroll-tokens response")?;
        if args.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&body).context("failed to render json")?
            );
            return Ok(());
        }
        for token in body {
            println!(
                "token={} expires_at_unix={} expired={}",
                token.enroll_token, token.expires_at_unix, token.expired
            );
        }
        return Ok(());
    }

    let store =
        state::Store::load_or_init(&settings.state_file, Vec::new(), Duration::from_secs(1))
            .await
            .with_context(|| {
                format!(
                    "failed to initialize store {}",
                    settings.state_file.display()
                )
            })?;
    let tokens = store.list_enroll_tokens(args.include_expired).await?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&tokens).context("failed to render json")?
        );
        return Ok(());
    }
    for token in tokens {
        println!(
            "token={} expires_at_unix={} expired={}",
            token.enroll_token, token.expires_at_unix, token.expired
        );
    }
    Ok(())
}

pub async fn revoke_enroll_token(args: RevokeEnrollTokenArgs) -> Result<()> {
    let settings = config::resolve_revoke_enroll_token_settings(&args)?;
    if let Some(base) = settings.public_url.as_deref() {
        let url = format!("{}/api/v1/control/enroll-tokens/{}", base, args.token);
        let client = reqwest::Client::new();
        let response = client
            .delete(&url)
            .send()
            .await
            .with_context(|| format!("failed to call {url}"))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable error body>".to_string());
            anyhow::bail!("live revoke-enroll-token failed with {status}: {body}");
        }
        let body: api::RevokeEnrollTokenResponse = response
            .json()
            .await
            .context("failed to decode revoke-enroll-token response")?;
        println!("token={} revoked={}", body.token, body.revoked);
        return Ok(());
    }

    let store =
        state::Store::load_or_init(&settings.state_file, Vec::new(), Duration::from_secs(1))
            .await
            .with_context(|| {
                format!(
                    "failed to initialize store {}",
                    settings.state_file.display()
                )
            })?;
    let removed = store.revoke_enroll_token(&args.token).await?;
    println!("token={} revoked={}", args.token, removed);
    Ok(())
}

pub async fn state_stats(args: StateStatsArgs) -> Result<()> {
    let settings = config::resolve_state_stats_settings(&args)?;
    if let Some(base) = settings.public_url.as_deref() {
        let url = format!("{}/api/v1/control/state-stats", base);
        let response = reqwest::get(&url)
            .await
            .with_context(|| format!("failed to call {url}"))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable error body>".to_string());
            anyhow::bail!("live state-stats failed with {status}: {body}");
        }
        let body: api::StateStatsResponse = response
            .json()
            .await
            .context("failed to decode state-stats response")?;
        if args.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&body).context("failed to render json")?
            );
            return Ok(());
        }
        println!("db_path={}", body.db_path);
        println!("schema_version={}", body.schema_version);
        println!("agent_count={}", body.agent_count);
        println!("enroll_token_count={}", body.enroll_token_count);
        println!(
            "expired_enroll_token_count={}",
            body.expired_enroll_token_count
        );
        return Ok(());
    }

    let store =
        state::Store::load_or_init(&settings.state_file, Vec::new(), Duration::from_secs(1))
            .await
            .with_context(|| {
                format!(
                    "failed to initialize store {}",
                    settings.state_file.display()
                )
            })?;
    let stats = store.stats().await?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&stats).context("failed to render json")?
        );
        return Ok(());
    }
    println!("db_path={}", stats.db_path.display());
    println!("schema_version={}", stats.schema_version);
    println!("agent_count={}", stats.agent_count);
    println!("enroll_token_count={}", stats.enroll_token_count);
    println!(
        "expired_enroll_token_count={}",
        stats.expired_enroll_token_count
    );
    Ok(())
}
