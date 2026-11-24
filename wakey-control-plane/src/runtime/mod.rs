use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::Router;
use axum::response::Redirect;
use axum::routing::get_service;
use axum::routing::{get, post};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, RwLock, Semaphore, mpsc, oneshot};
#[cfg(unix)]
use tokio::time::MissedTickBehavior;
use tower_http::services::ServeDir;
use tower_http::services::ServeFile;
use tracing::info;
#[cfg(unix)]
use tracing::warn;
use wakey_core::protocol::{AgentCapability, AgentCapabilityOptions, ErrorPayload, ServerMessage};

use crate::api;
use crate::config;
use crate::state;
use crate::ws;

mod access;
mod admin;
mod fleet_metrics;
mod process;
mod process_status;
pub mod terminals;
pub(crate) mod view_cache;
pub use admin::revoke_agent;
pub use admin::{issue_enroll_token, list_enroll_tokens, revoke_enroll_token, state_stats};
use process::PidFile;
pub use process::reload_daemon;

/// Shared state for HTTP handlers and websocket relay paths.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<state::Store>,
    pub sessions: Arc<RwLock<HashMap<String, AgentSession>>>,
    pub pending: Arc<Mutex<HashMap<String, oneshot::Sender<AgentReply>>>>,
    pub(crate) ping_slots: Arc<Semaphore>,
    pub public_url: String,
    pub command_timeout: Duration,
    pub enroll_token_ttl: Duration,
    pub terminals: terminals::TerminalRegistry,
    pub alerts: api::AlertsHub,
    pub(crate) fleet: Arc<api::FleetCache>,
    pub(crate) process_status: Arc<process_status::ProcessStatusCache>,
}

#[derive(Clone)]
pub struct AgentSession {
    pub connection_id: String,
    pub tx: mpsc::UnboundedSender<SessionEvent>,
    pub capabilities: Vec<AgentCapability>,
    pub capability_options: AgentCapabilityOptions,
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
            get_service(ServeDir::new(ui_dist_dir).fallback(ServeFile::new(index_file))),
        )
        .route("/healthz", get(api::healthz))
        .route("/api/v1/agents/enroll", post(api::enroll))
        .route("/api/v1/agent/ws", get(ws::agent_ws))
        .route(
            "/api/v1/agent/terminals/{terminal_id}/ws",
            get(api::agent_terminal_ws),
        )
}

fn control_api_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/control/process-status",
            get(process_status::get_process_status),
        )
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
            "/api/v1/control/fleet/devices",
            get(api::list_fleet_devices),
        )
        .route(
            "/api/v1/control/fleet/devices/refresh",
            post(api::refresh_fleet_devices),
        )
        .route(
            "/api/v1/control/fleet/devices/{device_key}",
            get(api::get_fleet_device),
        )
        .route("/api/v1/control/fleet/wake", post(api::wake_fleet_device))
        .route(
            "/api/v1/control/devices",
            get(api::list_known_devices).post(api::create_known_device),
        )
        .route(
            "/api/v1/control/devices/{device_id}",
            get(api::get_known_device)
                .patch(api::rename_known_device)
                .delete(api::forget_known_device),
        )
        .route(
            "/api/v1/control/devices/{device_id}/merge",
            post(api::merge_known_device),
        )
        .route(
            "/api/v1/control/devices/{device_id}/identifiers",
            post(api::attach_device_identifier),
        )
        .route(
            "/api/v1/control/devices/{device_id}/identifiers/{identifier_key}",
            axum::routing::delete(api::detach_device_identifier),
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
        .route(
            "/api/v1/control/terminals",
            get(api::list_terminals).post(api::create_terminal),
        )
        .route(
            "/api/v1/control/terminals/events/ws",
            get(api::terminal_events_ws),
        )
        .route(
            "/api/v1/control/terminals/{terminal_id}",
            get(api::get_terminal).delete(api::close_terminal),
        )
        .route(
            "/api/v1/control/terminals/{terminal_id}/attach",
            post(api::attach_terminal),
        )
        .route(
            "/api/v1/control/terminals/{terminal_id}/ws",
            get(api::operator_terminal_ws),
        )
}

/// Starts the control-plane HTTP and websocket surfaces and manages daemon lifecycle hooks.
pub async fn serve(daemon: config::DaemonConfig) -> Result<()> {
    // The verifier delegates enforcement to Caddy. Do not expose the other
    // handlers directly when that boundary is configured.
    let access_routes = access::routes(daemon.access.as_ref(), daemon.bind)?;
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
        ping_slots: Arc::new(Semaphore::new(8)),
        public_url: daemon.public_url.clone(),
        command_timeout: daemon.command_timeout,
        enroll_token_ttl: daemon.enroll_token_ttl,
        terminals: terminals::TerminalRegistry::new(),
        alerts: api::AlertsHub::new(),
        fleet: Arc::new(api::FleetCache::default()),
        process_status: Default::default(),
    };

    // Keep route classes explicit so edge policy can map directly:
    // - public_api_routes: intended internet-facing agent endpoints
    // - control_api_routes: intended admin-only endpoints behind Access
    let app = Router::new()
        .merge(access_routes)
        .merge(public_api_routes(daemon.ui_dist_dir.clone()))
        .merge(control_api_routes())
        .with_state(app_state.clone());

    info!(bind = %daemon.bind, data_dir = %daemon.data_dir.display(), pid_file = %daemon.pid_file.display(), state_file = %daemon.state_file.display(), ui_dist_dir = %daemon.ui_dist_dir.display(), "starting control-plane server");
    let listener = TcpListener::bind(daemon.bind)
        .await
        .with_context(|| format!("failed to bind control-plane listener to {}", daemon.bind))?;

    #[cfg(unix)]
    let (mut hup, mut terminate) = {
        use tokio::signal::unix::{SignalKind, signal};
        (
            signal(SignalKind::hangup()).context("failed to install SIGHUP handler")?,
            signal(SignalKind::terminate()).context("failed to install SIGTERM handler")?,
        )
    };

    // Publish the PID only after the database, listener, and reload handler are ready.
    let _pid_file = PidFile::write(&daemon.pid_file)?;
    info!(pid_file = %daemon.pid_file.display(), "wrote control-plane pid file");
    tokio::spawn(api::run_alert_evaluator(app_state.clone()));
    let _fleet_metrics = fleet_metrics::FleetMetricsTask::start(app_state.clone());
    #[cfg(target_os = "linux")]
    let _process_status = process_status::ProcessStatusTask::start(app_state.clone());
    let mut server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .context("control-plane server exited unexpectedly")
    });

    #[cfg(unix)]
    {
        let mut gc_tick = tokio::time::interval(Duration::from_secs(300));
        gc_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

        // Unix daemon loop: shutdown signals, fleet invalidation, and periodic maintenance.
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    info!("ctrl-c received; shutting down control-plane");
                    break;
                }
                _ = terminate.recv() => {
                    info!("SIGTERM received; shutting down control-plane");
                    break;
                }
                _ = hup.recv() => {
                    if let Err(err) = app_state.store.refresh_fleet_views().await {
                        warn!(error = %err, "failed to refresh fleet views");
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
                    match app_state.store.gc_retention(daemon.history_retention).await {
                        Ok(stats) => {
                            if stats.audit_events_removed > 0 || stats.alert_transitions_removed > 0 {
                                info!(
                                    audit_events_removed = stats.audit_events_removed,
                                    alert_transitions_removed = stats.alert_transitions_removed,
                                    "periodic gc pruned rows older than retention window"
                                );
                            }
                        }
                        Err(err) => warn!(error = %err, "periodic retention gc failed"),
                    }
                    match app_state.store.prune_devices(daemon.device_retention).await {
                        Ok(removed) if removed > 0 => info!(removed, "periodic gc pruned expired agent devices"),
                        Ok(_) => {},
                        Err(err) => warn!(error = %err, "periodic device retention gc failed"),
                    }
                }
                join = &mut server => {
                    return join.context("control-plane join failed")?;
                }
            }
        }
    }

    #[cfg(not(unix))]
    let shutdown = tokio::select! {
        signal = tokio::signal::ctrl_c() => signal.context("failed waiting for ctrl-c"),
        join = &mut server => return join.context("control-plane join failed")?,
    };

    server.abort();
    let _ = server.await;
    #[cfg(not(unix))]
    shutdown?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[tokio::test]
    async fn failed_store_startup_does_not_publish_pid() {
        let dir = std::env::temp_dir().join(format!("wakey-cc-startup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let args = crate::cli::Cli::try_parse_from([
            "wakey-control-plane",
            "serve",
            "--data-dir",
            dir.to_str().unwrap(),
            "--config-file",
            dir.join("missing.toml").to_str().unwrap(),
            "--state-file",
            dir.to_str().unwrap(),
            "--pid-file",
            "wakey-cc.pid",
        ])
        .unwrap();
        let crate::cli::Command::Serve(args) = args.command else {
            panic!("expected serve command");
        };
        let daemon = config::DaemonConfig::from_serve_args(&args).unwrap();
        let pid = daemon.pid_file.clone();

        let err = serve(daemon).await.unwrap_err();
        assert!(err.to_string().contains("failed to initialize store"));
        assert!(!pid.exists());
        std::fs::remove_dir(dir).unwrap();
    }
}
