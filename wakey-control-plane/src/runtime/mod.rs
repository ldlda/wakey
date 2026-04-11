use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::Router;
use axum::response::Redirect;
use axum::routing::{get, post};
use axum::routing::get_service;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
use tokio::time::MissedTickBehavior;
use tower_http::services::ServeDir;
use tower_http::services::ServeFile;
use tracing::{info, warn};
use wakey_agent::protocol::{ErrorPayload, ServerMessage};

use crate::api;
use crate::config;
use crate::state;
use crate::ws;

mod admin;
mod process;
pub use admin::{
    issue_enroll_token, list_enroll_tokens, revoke_enroll_token, state_stats,
};
pub use process::reload_daemon;
use process::{remove_pid_file, write_pid_file};

/// Shared state for HTTP handlers and websocket relay paths.
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

fn public_api_routes() -> Router<AppState> {
    Router::new()
    .route("/ui", get(|| async { Redirect::temporary("/ui/") }))
        .nest_service(
            "/ui/",
            get_service(
                ServeDir::new("ui/dist").not_found_service(ServeFile::new("ui/dist/index.html")),
            ),
        )
        .route("/healthz", get(api::healthz))
        .route("/api/v1/agents/enroll", post(api::enroll))
        .route("/api/v1/agent/ws", get(ws::agent_ws))
}

fn control_api_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/control/enroll-token", post(api::issue_enroll_token))
        .route("/api/v1/control/enroll-tokens", get(api::list_enroll_tokens))
        .route(
            "/api/v1/control/enroll-tokens/{token}",
            axum::routing::delete(api::revoke_enroll_token),
        )
        .route("/api/v1/control/state-stats", get(api::state_stats))
        .route("/api/v1/control/audit/events", get(api::list_audit_events))
        .route("/api/v1/control/alerts", get(api::active_alerts))
        .route("/api/v1/control/alerts/history", get(api::alert_history))
        .route("/api/v1/control/alerts/ws", get(api::alerts_stream))
        .route("/api/v1/control/agents", get(api::list_agents))
        .route(
            "/api/v1/control/agents/{agent_id}/command",
            post(api::run_command),
        )
}

/// Starts the control-plane HTTP and websocket surfaces and manages daemon lifecycle hooks.
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

    // Keep route classes explicit so edge policy can map directly:
    // - public_api_routes: intended internet-facing agent endpoints
    // - control_api_routes: intended admin-only endpoints behind Access
    let app = Router::new()
        .merge(public_api_routes())
        .merge(control_api_routes())
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

        // Unix daemon loop: shutdown signal, config reload trigger, and periodic maintenance.
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
