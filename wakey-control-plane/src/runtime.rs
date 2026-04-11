use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::Router;
use axum::routing::{get, post};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
use tokio::time::MissedTickBehavior;
use tracing::{info, warn};
use wakey_agent::protocol::{ErrorPayload, ServerMessage};

use crate::api;
use crate::cli::{IssueEnrollTokenArgs, ListEnrollTokensArgs, RevokeEnrollTokenArgs, StateStatsArgs};
use crate::config;
use crate::state;
use crate::ws;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<state::Store>,
    pub sessions: Arc<RwLock<HashMap<String, mpsc::UnboundedSender<ServerMessage>>>>,
    pub pending: Arc<Mutex<HashMap<String, oneshot::Sender<AgentReply>>>>,
    pub public_url: String,
    pub command_timeout: Duration,
    pub enroll_token_ttl: Duration,
}

pub enum AgentReply {
    Result(serde_json::Value),
    Error(ErrorPayload),
}

pub async fn serve(daemon: config::DaemonConfig) -> Result<()> {
    write_pid_file(&daemon.pid_file)?;
    info!(pid_file = %daemon.pid_file.display(), "wrote control-plane pid file");

    let store = state::Store::load_or_init(
        &daemon.state_file,
        daemon.enroll_tokens.clone(),
        daemon.enroll_token_ttl,
    )
        .await
        .with_context(|| format!("failed to initialize store {}", daemon.state_file.display()))?;

    let app_state = AppState {
        store: Arc::new(store),
        sessions: Arc::new(RwLock::new(HashMap::new())),
        pending: Arc::new(Mutex::new(HashMap::new())),
        public_url: daemon.public_url.clone(),
        command_timeout: daemon.command_timeout,
        enroll_token_ttl: daemon.enroll_token_ttl,
    };

    let app = Router::new()
        .route("/healthz", get(api::healthz))
        .route("/api/v1/agents/enroll", post(api::enroll))
        .route("/api/v1/control/enroll-token", post(api::issue_enroll_token))
        .route("/api/v1/control/enroll-tokens", get(api::list_enroll_tokens))
        .route(
            "/api/v1/control/enroll-tokens/{token}",
            axum::routing::delete(api::revoke_enroll_token),
        )
        .route("/api/v1/control/state-stats", get(api::state_stats))
        .route("/api/v1/agent/ws", get(ws::agent_ws))
        .route("/api/v1/control/agents", get(api::list_agents))
        .route(
            "/api/v1/control/agents/{agent_id}/command",
            post(api::run_command),
        )
        .with_state(app_state.clone());

    info!(bind = %daemon.bind, data_dir = %daemon.data_dir.display(), pid_file = %daemon.pid_file.display(), state_file = %daemon.state_file.display(), "starting control-plane server");
    let listener = TcpListener::bind(daemon.bind).await?;
    let mut server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .context("control-plane server exited unexpectedly")
    });

    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut hup = signal(SignalKind::hangup()).context("failed to install SIGHUP handler")?;
        let mut gc_tick = tokio::time::interval(Duration::from_secs(300));
        gc_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    info!("ctrl-c received; shutting down control-plane");
                    server.abort();
                    break;
                }
                _ = hup.recv() => {
                    match app_state.store.reload_from_disk().await {
                        Ok(()) => info!("reloaded state from disk"),
                        Err(err) => warn!(error = %err, "failed to reload state from disk"),
                    }
                }
                _ = gc_tick.tick() => {
                    match app_state.store.gc_expired_enroll_tokens().await {
                        Ok(removed) => {
                            if removed > 0 {
                                info!(removed, "periodic gc removed expired enroll tokens");
                            }
                        }
                        Err(err) => warn!(error = %err, "periodic gc failed"),
                    }
                }
                join = &mut server => {
                    let _ = remove_pid_file(&daemon.pid_file);
                    return join.context("control-plane join failed")?;
                }
            }
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .context("failed waiting for ctrl-c")?;
        server.abort();
    }

    let _ = remove_pid_file(&daemon.pid_file);
    Ok(())
}

pub async fn issue_enroll_token(args: IssueEnrollTokenArgs) -> Result<()> {
    let settings = config::resolve_issue_token_settings(&args)?;

    if let Some(url) = args.public_url {
        let base = config::normalize_public_url(&url);
        let ttl_seconds = settings.ttl.as_secs().max(1);
        let endpoint = format!("{}?ttl_seconds={ttl_seconds}", config::issue_token_endpoint(&base));
        info!(endpoint = %endpoint, "requesting live enroll token from running control-plane daemon");
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
        info!("received live enroll token response");

        println!("enroll_token={}", payload.enroll_token);
        println!("expires_at_unix={}", payload.expires_at_unix);
        println!(
            "agent_command=wakey-agent enroll --server-url {base} --enroll-token {}",
            payload.enroll_token
        );
        return Ok(());
    }

    // Fallback for offline tooling: writes to state file, requires daemon reload to pick up.
    info!(data_dir = %settings.data_dir.display(), state_file = %settings.state_file.display(), ttl_seconds = settings.ttl.as_secs(), "issuing enroll token via offline state file fallback");
    let store = state::Store::load_or_init(&settings.state_file, args.enroll_tokens, settings.ttl)
        .await
        .with_context(|| format!("failed to initialize store {}", settings.state_file.display()))?;
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
            base,
            args.include_expired
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

    let store = state::Store::load_or_init(&settings.state_file, Vec::new(), Duration::from_secs(1))
        .await
        .with_context(|| format!("failed to initialize store {}", settings.state_file.display()))?;
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

    let store = state::Store::load_or_init(&settings.state_file, Vec::new(), Duration::from_secs(1))
        .await
        .with_context(|| format!("failed to initialize store {}", settings.state_file.display()))?;
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
        println!("expired_enroll_token_count={}", body.expired_enroll_token_count);
        return Ok(());
    }

    let store = state::Store::load_or_init(&settings.state_file, Vec::new(), Duration::from_secs(1))
        .await
        .with_context(|| format!("failed to initialize store {}", settings.state_file.display()))?;
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
    println!("expired_enroll_token_count={}", stats.expired_enroll_token_count);
    Ok(())
}

pub fn reload_daemon(pid_file: &Path) -> Result<()> {
    let pid = read_pid(pid_file)?;
    info!(pid, pid_file = %pid_file.display(), "sending control-plane reload signal");
    send_hup(pid)
}

fn write_pid_file(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create pid dir {}", parent.display()))?;
    }
    std::fs::write(path, format!("{}\n", std::process::id()))
        .with_context(|| format!("failed to write pid file {}", path.display()))
}

fn remove_pid_file(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => {
            Err(err).with_context(|| format!("failed to remove pid file {}", path.display()))
        }
    }
}

fn read_pid(path: &Path) -> Result<i32> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read pid file {}", path.display()))?;
    let pid = raw
        .trim()
        .parse::<i32>()
        .with_context(|| format!("invalid pid in {}", path.display()))?;
    if pid <= 0 {
        anyhow::bail!("invalid non-positive pid {pid}");
    }
    Ok(pid)
}

fn send_hup(pid: i32) -> Result<()> {
    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;

        kill(Pid::from_raw(pid), Signal::SIGHUP)
            .with_context(|| format!("failed to send SIGHUP to pid {pid}"))?;
        Ok(())
    }

    #[cfg(not(unix))]
    {
        let _ = pid;
        anyhow::bail!("reload is only supported on Unix (SIGHUP unavailable on this platform)")
    }
}
