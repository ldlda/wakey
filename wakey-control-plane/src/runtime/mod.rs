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
use crate::config;
use crate::state;
use crate::ws;

mod admin;
pub use admin::{
    issue_enroll_token, list_enroll_tokens, revoke_enroll_token, state_stats,
};

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
