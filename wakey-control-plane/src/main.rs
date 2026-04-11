mod state;
mod tracing;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use ::tracing::{debug, info, warn};
use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::{ArgAction, Parser};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
use uuid::Uuid;
use wakey_agent::protocol::{AgentCommand, ErrorPayload, RequestId, ServerMessage};

#[derive(Parser)]
#[command(name = "wakey-control-plane")]
#[command(version, about = "Control plane server for wakey-agent fleets")]
struct Cli {
    #[arg(short = 'v', long = "verbose", action = ArgAction::Count, global = true)]
    verbose: u8,

    #[arg(long, default_value = "0.0.0.0:8080")]
    bind: SocketAddr,

    #[arg(long, default_value = "http://127.0.0.1:8080")]
    public_url: String,

    #[arg(long, default_value = "/var/lib/wakey-control-plane/state.json")]
    state_file: std::path::PathBuf,

    #[arg(long = "enroll-token")]
    enroll_tokens: Vec<String>,

    #[arg(long, default_value_t = 30_000)]
    command_timeout_ms: u64,
}

#[derive(Clone)]
struct AppState {
    store: Arc<state::Store>,
    sessions: Arc<RwLock<HashMap<String, mpsc::UnboundedSender<ServerMessage>>>>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<AgentReply>>>>,
    public_url: String,
    command_timeout: Duration,
}

enum AgentReply {
    Result(serde_json::Value),
    Error(ErrorPayload),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum IncomingClientMessage {
    Hello {
        agent_id: String,
    },
    Auth {
        agent_id: String,
        agent_token: String,
    },
    Heartbeat {
        agent_id: String,
    },
    Result {
        request_id: RequestId,
        result: serde_json::Value,
    },
    Error {
        request_id: RequestId,
        error: ErrorPayload,
    },
}

#[derive(Debug, Deserialize)]
struct EnrollRequest {
    enroll_token: String,
}

#[derive(Debug, Serialize)]
struct EnrollResponse {
    agent_id: String,
    agent_token: String,
    server_url: String,
}

#[derive(Debug, Serialize)]
struct AgentStatus {
    agent_id: String,
    connected: bool,
}

#[derive(Debug, Deserialize)]
struct RelayCommandRequest {
    command: AgentCommand,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
struct RelayCommandResponse {
    request_id: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ErrorPayload>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    tracing::init(cli.verbose);

    let store = state::Store::load_or_init(&cli.state_file, cli.enroll_tokens)
        .await
        .with_context(|| format!("failed to initialize store {}", cli.state_file.display()))?;

    let app_state = AppState {
        store: Arc::new(store),
        sessions: Arc::new(RwLock::new(HashMap::new())),
        pending: Arc::new(Mutex::new(HashMap::new())),
        public_url: cli.public_url.trim_end_matches('/').to_string(),
        command_timeout: Duration::from_millis(cli.command_timeout_ms.max(1)),
    };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/api/v1/agents/enroll", post(enroll))
        .route("/api/v1/agent/ws", get(agent_ws))
        .route("/api/v1/control/agents", get(list_agents))
        .route(
            "/api/v1/control/agents/{agent_id}/command",
            post(run_command),
        )
        .with_state(app_state);

    info!(bind = %cli.bind, "starting control-plane server");
    let listener = TcpListener::bind(cli.bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn healthz() -> &'static str {
    "ok"
}

async fn enroll(
    State(state): State<AppState>,
    Json(req): Json<EnrollRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    match state.store.enroll(&req.enroll_token).await {
        Ok(issued) => Ok((
            StatusCode::OK,
            Json(EnrollResponse {
                agent_id: issued.agent_id,
                agent_token: issued.agent_token,
                server_url: state.public_url,
            }),
        )),
        Err(err) => Err(json_error(
            StatusCode::UNAUTHORIZED,
            "enrollment_rejected",
            &err.to_string(),
        )),
    }
}

async fn list_agents(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let enrolled = state.store.list_agents().await;
    let sessions = state.sessions.read().await;

    let agents = enrolled
        .into_iter()
        .map(|agent_id| AgentStatus {
            connected: sessions.contains_key(&agent_id),
            agent_id,
        })
        .collect::<Vec<_>>();

    Ok((StatusCode::OK, Json(agents)))
}

async fn run_command(
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    Json(req): Json<RelayCommandRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let request_id_string = format!("req-{}", Uuid::new_v4());
    let request_id = RequestId::try_from(request_id_string.clone()).map_err(|err| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid_request_id",
            &err,
        )
    })?;

    let tx = {
        let sessions = state.sessions.read().await;
        sessions.get(&agent_id).cloned()
    }
    .ok_or_else(|| {
        json_error(
            StatusCode::NOT_FOUND,
            "agent_not_connected",
            "agent is not connected",
        )
    })?;

    let (pending_tx, pending_rx) = oneshot::channel();
    state
        .pending
        .lock()
        .await
        .insert(request_id_string.clone(), pending_tx);

    if let Err(err) = tx.send(ServerMessage::Command {
        request_id,
        command: req.command,
    }) {
        state.pending.lock().await.remove(&request_id_string);
        return Err(json_error(
            StatusCode::BAD_GATEWAY,
            "agent_send_failed",
            &format!("failed to send command to agent: {err}"),
        ));
    }

    let timeout = Duration::from_millis(
        req.timeout_ms
            .unwrap_or(state.command_timeout.as_millis() as u64)
            .max(1),
    );
    let outcome = tokio::time::timeout(timeout, pending_rx).await;
    let response = match outcome {
        Ok(Ok(AgentReply::Result(result))) => RelayCommandResponse {
            request_id: request_id_string,
            status: "ok".into(),
            result: Some(result),
            error: None,
        },
        Ok(Ok(AgentReply::Error(error))) => RelayCommandResponse {
            request_id: request_id_string,
            status: "error".into(),
            result: None,
            error: Some(error),
        },
        Ok(Err(_)) => {
            return Err(json_error(
                StatusCode::BAD_GATEWAY,
                "agent_response_dropped",
                "agent response channel dropped",
            ));
        }
        Err(_) => {
            state.pending.lock().await.remove(&request_id_string);
            return Err(json_error(
                StatusCode::GATEWAY_TIMEOUT,
                "agent_timeout",
                "agent did not answer before timeout",
            ));
        }
    };

    Ok((StatusCode::OK, Json(response)))
}

async fn agent_ws(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_agent_socket(state, socket))
}

async fn handle_agent_socket(state: AppState, socket: WebSocket) {
    let (mut write, mut read) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMessage>();

    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let encoded = match serde_json::to_string(&msg) {
                Ok(s) => s,
                Err(err) => {
                    warn!(error = %err, "failed to encode server websocket message");
                    continue;
                }
            };
            if let Err(err) = write.send(Message::Text(encoded.into())).await {
                warn!(error = %err, "failed to send websocket message");
                break;
            }
        }
    });

    let mut authed_agent_id: Option<String> = None;

    loop {
        let frame = read.next().await;
        let msg = match frame {
            Some(Ok(msg)) => msg,
            Some(Err(err)) => {
                warn!(error = %err, "agent websocket receive error");
                break;
            }
            None => break,
        };

        match msg {
            Message::Text(text) => {
                if let Err(err) = process_agent_text(&state, &tx, &mut authed_agent_id, &text).await
                {
                    warn!(error = %err, "closing agent websocket due to protocol/auth error");
                    break;
                }
            }
            Message::Ping(_) => {}
            Message::Pong(_) => {}
            Message::Close(_) => break,
            Message::Binary(_) => {
                debug!("ignoring unexpected binary websocket frame");
            }
        }
    }

    if let Some(agent_id) = authed_agent_id {
        info!(agent_id = %agent_id, "agent disconnected");
        state.sessions.write().await.remove(&agent_id);
    }

    writer.abort();
}

async fn process_agent_text(
    state: &AppState,
    tx: &mpsc::UnboundedSender<ServerMessage>,
    authed_agent_id: &mut Option<String>,
    text: &str,
) -> Result<()> {
    let message: IncomingClientMessage =
        serde_json::from_str(text).context("invalid client websocket payload")?;

    match message {
        IncomingClientMessage::Hello { agent_id } => {
            debug!(agent_id = %agent_id, "agent hello received");
        }
        IncomingClientMessage::Auth {
            agent_id,
            agent_token,
        } => {
            if !state
                .store
                .verify_agent_token(&agent_id, &agent_token)
                .await
            {
                anyhow::bail!("agent auth rejected");
            }
            state
                .sessions
                .write()
                .await
                .insert(agent_id.clone(), tx.clone());
            *authed_agent_id = Some(agent_id.clone());
            info!(agent_id = %agent_id, "agent authenticated");
        }
        IncomingClientMessage::Heartbeat { agent_id } => {
            if authed_agent_id.as_deref() != Some(agent_id.as_str()) {
                anyhow::bail!("heartbeat for unauthenticated or mismatched agent");
            }
            debug!(agent_id = %agent_id, "heartbeat received");
        }
        IncomingClientMessage::Result { request_id, result } => {
            if authed_agent_id.is_none() {
                anyhow::bail!("result before auth");
            }
            let key = request_id.as_str().to_string();
            if let Some(waiter) = state.pending.lock().await.remove(&key) {
                let _ = waiter.send(AgentReply::Result(result));
            }
        }
        IncomingClientMessage::Error { request_id, error } => {
            if authed_agent_id.is_none() {
                anyhow::bail!("error before auth");
            }
            let key = request_id.as_str().to_string();
            if let Some(waiter) = state.pending.lock().await.remove(&key) {
                let _ = waiter.send(AgentReply::Error(error));
            }
        }
    }

    Ok(())
}

fn json_error(
    status: StatusCode,
    code: &str,
    message: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "error": {
                "code": code,
                "message": message,
            }
        })),
    )
}
