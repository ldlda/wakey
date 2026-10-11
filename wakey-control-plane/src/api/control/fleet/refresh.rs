//! Refresh agents independently: one failed relay or snapshot does not cancel others.

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use futures_util::StreamExt;
use tracing::{info, warn};
use wakey_agent::protocol::{AgentCommand, InventoryRequest};

use super::types::{
    RefreshFleetAgentResult, RefreshFleetDevicesRequest, RefreshFleetDevicesResponse,
};
use crate::api::{
    ApiError,
    commands::{RelayCommandResponse, relay_agent_command},
};
use crate::runtime::AppState;
use crate::state::Store;

const REFRESH_CONCURRENCY: usize = 8;

#[tracing::instrument(name = "fleet_refresh", skip_all, fields(agents = tracing::field::Empty, succeeded = tracing::field::Empty, failed = tracing::field::Empty, accepted = tracing::field::Empty))]
pub async fn refresh_fleet_devices(
    State(state): State<AppState>,
    Json(req): Json<RefreshFleetDevicesRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let agent_ids = select_agents(&state, req.agent_ids).await;
    tracing::Span::current().record("agents", agent_ids.len());
    let results = futures_util::stream::iter(
        agent_ids
            .into_iter()
            .map(|agent_id| refresh_agent(&state, agent_id, req.timeout_ms)),
    )
    .buffer_unordered(REFRESH_CONCURRENCY)
    .collect::<Vec<_>>()
    .await;
    let response = summarize(results);
    let succeeded = response
        .agents
        .iter()
        .filter(|result| result.status == "ok")
        .count();
    let span = tracing::Span::current();
    span.record("succeeded", succeeded);
    span.record("failed", response.agents.len() - succeeded);
    span.record("accepted", response.total_accepted);
    info!("fleet refresh completed");
    Ok((StatusCode::OK, Json(response)))
}

/// Default to a point-in-time connected set. Explicit IDs remain eligible so
/// the relay can report disconnected agents as per-agent errors, not omit them.
async fn select_agents(state: &AppState, requested: Vec<String>) -> Vec<String> {
    let ids = if requested.is_empty() {
        state.sessions.read().await.keys().cloned().collect()
    } else {
        requested
    };
    normalize_refresh_agent_ids(ids)
}

pub(super) fn normalize_refresh_agent_ids(mut agent_ids: Vec<String>) -> Vec<String> {
    agent_ids.sort();
    agent_ids.dedup();
    agent_ids
}

#[tracing::instrument(name = "fleet_refresh_agent", skip_all, fields(agent_id = %agent_id, outcome = tracing::field::Empty, accepted = tracing::field::Empty))]
async fn refresh_agent(
    state: &AppState,
    agent_id: String,
    timeout_ms: Option<u64>,
) -> RefreshFleetAgentResult {
    let response = relay_agent_command(
        state,
        &agent_id,
        AgentCommand::Inventory(InventoryRequest {
            query: None,
            name: None,
            ips: Vec::new(),
            devs: Vec::new(),
            nuds: Vec::new(),
            macs: Vec::new(),
        }),
        timeout_ms,
    )
    .await;
    let result = store_inventory(&state.store, &agent_id, response).await;
    tracing::Span::current().record("outcome", result.status.as_str());
    tracing::Span::current().record("accepted", result.accepted);
    if result.status != "ok" {
        // The response retains diagnostic detail; do not export payloads.
        warn!("fleet agent refresh failed");
    }
    result
}

/// Validate the relay result before replacing anything. An empty *valid*
/// inventory is still a complete snapshot and intentionally clears old rows.
fn decode_inventory(
    response: Result<RelayCommandResponse, ApiError>,
) -> Result<wakey_core::DeviceInventory, String> {
    let response = response.map_err(|err| format!("{}: {}", err.status, err.message))?;
    if response.status != "ok" {
        return Err(response
            .error
            .map(|error| error.message)
            .unwrap_or_else(|| "inventory command failed".into()));
    }
    let result = response
        .result
        .ok_or_else(|| "inventory command returned no result".to_string())?;
    serde_json::from_value(result).map_err(|err| err.to_string())
}

async fn store_inventory(
    store: &Store,
    agent_id: &str,
    response: Result<RelayCommandResponse, ApiError>,
) -> RefreshFleetAgentResult {
    let accepted = async {
        let inventory = decode_inventory(response)?;
        store
            .replace_agent_device_snapshot(agent_id, &inventory.devices)
            .await
            .map_err(|err| err.to_string())
    }
    .await;
    match accepted {
        Ok(accepted) => RefreshFleetAgentResult {
            agent_id: agent_id.into(),
            status: "ok".into(),
            accepted,
            error: None,
        },
        Err(error) => RefreshFleetAgentResult {
            agent_id: agent_id.into(),
            status: "error".into(),
            accepted: 0,
            error: Some(error),
        },
    }
}

/// Concurrent completion order is deliberately not part of the response contract.
fn summarize(mut agents: Vec<RefreshFleetAgentResult>) -> RefreshFleetDevicesResponse {
    agents.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
    RefreshFleetDevicesResponse {
        total_accepted: agents.iter().map(|result| result.accepted).sum(),
        agents,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn response(result: Option<serde_json::Value>) -> RelayCommandResponse {
        RelayCommandResponse {
            request_id: "refresh-test".into(),
            status: "ok".into(),
            result,
            error: None,
        }
    }

    #[tokio::test]
    async fn refresh_updates_cached_fleet_despite_a_disconnected_agent() {
        use crate::runtime::{AgentReply, AgentSession, SessionEvent};
        use std::{collections::HashMap, sync::Arc, time::Duration};
        use tokio::sync::{Mutex, RwLock, mpsc};
        use wakey_agent::protocol::{CommandResult, ServerMessage};

        let dir =
            std::env::temp_dir().join(format!("wakey-refresh-handler-{}", uuid::Uuid::new_v4()));
        let state = AppState {
            store: Arc::new(
                Store::load_or_init(&dir.join("state.sqlite3"), vec![], Duration::from_secs(60))
                    .await
                    .unwrap(),
            ),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            public_url: "http://localhost".into(),
            command_timeout: Duration::from_secs(30),
            enroll_token_ttl: Duration::from_secs(60),
            terminals: crate::runtime::terminals::TerminalRegistry::new(),
            alerts: crate::api::AlertsHub::new(),
            fleet: Arc::new(super::super::FleetCache::default()),
        };
        let token = state
            .store
            .issue_enroll_token(Duration::from_secs(60))
            .await
            .unwrap();
        let agent_id = state
            .store
            .enroll(&token.enroll_token)
            .await
            .unwrap()
            .agent_id;
        let device = |ip: &str| {
            wakey_core::Device::from_parts(
                vec![wakey_core::NeighborEntry {
                    ip: ip.parse().unwrap(),
                    mac: Some("aa:bb:cc:dd:ee:01".parse().unwrap()),
                    dev: Some("br-lan".into()),
                    state: wakey_core::NeighborState::Reachable,
                }],
                vec![],
            )
        };
        state
            .store
            .replace_agent_device_snapshot(&agent_id, &[device("192.168.1.10")])
            .await
            .unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        state.sessions.write().await.insert(
            agent_id.clone(),
            AgentSession {
                connection_id: "test".into(),
                tx,
                capabilities: vec![],
                capability_options: Default::default(),
            },
        );
        assert_eq!(
            select_agents(&state, vec![]).await.as_slice(),
            std::slice::from_ref(&agent_id)
        );

        // Prime the real cache with old data before refresh touches the store.
        let query = super::super::types::ListFleetDevicesQuery {
            visibility: Some("all".into()),
            ..Default::default()
        };
        let before = super::super::load_fleet_devices(&state, &query)
            .await
            .unwrap();
        assert_eq!(before.len(), 1);
        assert_eq!(
            before[0].ips,
            ["192.168.1.10".parse::<std::net::IpAddr>().unwrap()]
        );

        let refresh = refresh_fleet_devices(
            State(state.clone()),
            Json(RefreshFleetDevicesRequest {
                agent_ids: vec!["disconnected".into(), agent_id.clone(), agent_id.clone()],
                timeout_ms: None,
            }),
        );
        // Answer through the production relay's pending-request channel. No
        // sleep or polling: receiving the command proves its waiter exists.
        let reply = async {
            let SessionEvent::Message(ServerMessage::Command {
                request_id,
                command,
                ..
            }) = rx.recv().await.unwrap()
            else {
                panic!("expected inventory command");
            };
            let AgentCommand::Inventory(request) = command else {
                panic!("expected inventory command");
            };
            assert!(request.query.is_none() && request.name.is_none());
            assert!(
                request.ips.is_empty()
                    && request.macs.is_empty()
                    && request.devs.is_empty()
                    && request.nuds.is_empty()
            );
            let result =
                serde_json::to_value(CommandResult::Inventory(wakey_core::DeviceInventory {
                    devices: vec![device("192.168.1.20")],
                }))
                .unwrap();
            state
                .pending
                .lock()
                .await
                .remove(request_id.as_str())
                .unwrap()
                .send(AgentReply::Result(result))
                .unwrap_or_else(|_| panic!("relay waiter dropped"));
        };
        let (response, ()) = tokio::join!(refresh, reply);
        let response = response.unwrap().into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let result: RefreshFleetDevicesResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(result.total_accepted, 1);
        assert_eq!(result.agents.len(), 2);
        assert!(
            result
                .agents
                .windows(2)
                .all(|pair| pair[0].agent_id < pair[1].agent_id)
        );
        let success = result
            .agents
            .iter()
            .find(|row| row.agent_id == agent_id)
            .unwrap();
        assert_eq!(success.status, "ok");
        assert_eq!(success.accepted, 1);
        assert!(success.error.is_none());
        let failed = result
            .agents
            .iter()
            .find(|row| row.agent_id == "disconnected")
            .unwrap();
        assert_eq!(failed.status, "error");
        assert_eq!(failed.accepted, 0);
        assert_eq!(
            failed.error.as_deref(),
            Some("404 Not Found: agent is not connected")
        );
        assert!(state.pending.lock().await.is_empty());
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));

        let after = super::super::load_fleet_devices(&state, &query)
            .await
            .unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].device_key, before[0].device_key);
        assert_eq!(
            after[0].ips,
            ["192.168.1.20".parse::<std::net::IpAddr>().unwrap()]
        );
        let stored = state
            .store
            .list_agent_device_rows_for_agent(&agent_id)
            .await
            .unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].endpoints.len(), 1);
        assert_eq!(
            stored[0].endpoints[0].key.ip,
            Some("192.168.1.20".parse().unwrap())
        );
        drop(state);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn refresh_stores_snapshots_but_invalid_results_leave_them_untouched() {
        let dir = std::env::temp_dir().join(format!("wakey-refresh-{}", uuid::Uuid::new_v4()));
        let store = Store::load_or_init(
            &dir.join("state.sqlite3"),
            vec![],
            std::time::Duration::from_secs(60),
        )
        .await
        .unwrap();
        let device = wakey_core::Device::from_parts(
            vec![wakey_core::NeighborEntry {
                ip: "192.168.1.10".parse().unwrap(),
                mac: Some("aa:bb:cc:dd:ee:01".parse().unwrap()),
                dev: Some("br-lan".into()),
                state: wakey_core::NeighborState::Reachable,
            }],
            vec![],
        );
        let inventory = serde_json::to_value(wakey_core::DeviceInventory {
            devices: vec![device],
        })
        .unwrap();
        let stored = store_inventory(&store, "agent", Ok(response(Some(inventory)))).await;
        assert_eq!(stored.status, "ok");
        assert_eq!(stored.accepted, 1);
        let revision = store.fleet_revision();
        let mut command_failed = response(None);
        command_failed.status = "error".into();
        command_failed.error = Some(wakey_agent::protocol::ErrorPayload {
            code: "inventory_failed".into(),
            message: "source unavailable".into(),
            retryable: None,
        });
        for failed in [
            Ok(response(None)),
            Ok(response(Some(json!({"devices": "invalid"})))),
            Ok(command_failed),
            Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                "agent_send_failed",
                "send failed",
            )),
        ] {
            let invalid = store_inventory(&store, "agent", failed).await;
            assert_eq!(invalid.status, "error");
            assert_eq!(invalid.accepted, 0);
            assert!(invalid.error.is_some());
            assert_eq!(store.fleet_revision(), revision);
            let rows = store
                .list_agent_device_rows_for_agent("agent")
                .await
                .unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].endpoints.len(), 1);
            assert_eq!(
                rows[0].endpoints[0].key.ip,
                Some("192.168.1.10".parse().unwrap())
            );
        }
        let rows = store
            .list_agent_device_rows_for_agent("agent")
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].endpoints.len(), 1);
        let empty =
            store_inventory(&store, "agent", Ok(response(Some(json!({"devices": []}))))).await;
        assert_eq!(empty.status, "ok");
        assert_eq!(empty.accepted, 0);
        assert!(
            store
                .list_agent_device_rows_for_agent("agent")
                .await
                .unwrap()
                .is_empty()
        );
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
