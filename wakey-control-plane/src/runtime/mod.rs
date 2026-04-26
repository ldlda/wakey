use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::Router;
use axum::response::Redirect;
use axum::routing::get_service;
use axum::routing::{get, post};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
#[cfg(unix)]
use tokio::time::MissedTickBehavior;
use tower_http::services::ServeDir;
use tower_http::services::ServeFile;
use tracing::info;
#[cfg(unix)]
use tracing::warn;
use wakey_agent::protocol::{ErrorPayload, ServerMessage};

use crate::api;
use crate::config;
use crate::state;
use crate::ws;

mod admin;
mod process;
pub use admin::revoke_agent;
pub use admin::{
    import_sled_state, issue_enroll_token, list_enroll_tokens, revoke_enroll_token, state_stats,
};
pub use process::reload_daemon;
use process::{remove_pid_file, write_pid_file};

/// Shared state for HTTP handlers and websocket relay paths.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<state::Store>,
    pub sessions: Arc<RwLock<HashMap<String, AgentSession>>>,
    pub pending: Arc<Mutex<HashMap<String, oneshot::Sender<AgentReply>>>>,
    pub public_url: String,
    pub command_timeout: Duration,
    pub enroll_token_ttl: Duration,
}

#[derive(Clone)]
pub struct AgentSession {
    pub connection_id: String,
    pub tx: mpsc::UnboundedSender<SessionEvent>,
}
#[derive(Clone)]
pub enum SessionEvent {
    Message(ServerMessage),
    Close,
}

pub enum AgentReply {
    Result(serde_json::Value),
    Error(ErrorPayload),
}

fn public_api_routes(ui_dist_dir: std::path::PathBuf) -> Router<AppState> {
    let index_file = ui_dist_dir.join("index.html");

    Router::new()
        .route("/ui", get(|| async { Redirect::temporary("/ui/") }))
        .route("/", get(|| async { Redirect::temporary("/ui/") })) // same as caddyfile
        .nest_service(
            "/ui/",
            get_service(ServeDir::new(ui_dist_dir).not_found_service(ServeFile::new(index_file))),
        )
        .route("/healthz", get(api::healthz))
        .route("/api/v1/agents/enroll", post(api::enroll))
        .route("/api/v1/agent/ws", get(ws::agent_ws))
}

fn control_api_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/control/enroll-token",
            post(api::issue_enroll_token),
        )
        .route(
            "/api/v1/control/enroll-tokens",
            get(api::list_enroll_tokens),
        )
        .route(
            "/api/v1/control/enroll-tokens/{token}",
            axum::routing::delete(api::revoke_enroll_token),
        )
        .route("/api/v1/control/state-stats", get(api::state_stats))
        .route(
            "/api/v1/control/devices",
            get(api::list_known_devices).post(api::create_known_device),
        )
        .route(
            "/api/v1/control/devices/{device_id}",
            axum::routing::delete(api::forget_known_device),
        )
        .route(
            "/api/v1/control/devices/{device_id}/identifiers",
            post(api::attach_device_identifier),
        )
        .route("/api/v1/control/audit/events", get(api::list_audit_events))
        .route("/api/v1/control/alerts", get(api::active_alerts))
        .route("/api/v1/control/alerts/history", get(api::alert_history))
        .route("/api/v1/control/alerts/ws", get(api::alerts_stream))
        .route("/api/v1/control/agents", get(api::list_agents))
        .route(
            "/api/v1/control/agents/{agent_id}",
            axum::routing::delete(api::revoke_agent),
        )
        .route(
            "/api/v1/control/agents/{agent_id}/nickname",
            axum::routing::patch(api::set_agent_nickname),
        )
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
        daemon.bootstrap_enroll_tokens.clone(),
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
        .merge(public_api_routes(daemon.ui_dist_dir.clone()))
        .merge(control_api_routes())
        .with_state(app_state.clone());

    info!(bind = %daemon.bind, data_dir = %daemon.data_dir.display(), pid_file = %daemon.pid_file.display(), state_file = %daemon.state_file.display(), ui_dist_dir = %daemon.ui_dist_dir.display(), "starting control-plane server");
    let listener = TcpListener::bind(daemon.bind).await?;
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .context("control-plane server exited unexpectedly")
    });

    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut server = server;
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
