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

    #[test]
    fn decode_keeps_relay_and_payload_failures_distinct() {
        assert_eq!(
            decode_inventory(Ok(response(None))).unwrap_err(),
            "inventory command returned no result"
        );
        assert!(decode_inventory(Ok(response(Some(json!({"devices": "invalid"}))))).is_err());
        let mut failed = response(Some(json!({"devices": []})));
        failed.status = "error".into();
        assert_eq!(
            decode_inventory(Ok(failed)).unwrap_err(),
            "inventory command failed"
        );
        assert_eq!(
            decode_inventory(Err(ApiError::new(
                StatusCode::NOT_FOUND,
                "agent_not_connected",
                "agent is not connected"
            )))
            .unwrap_err(),
            "404 Not Found: agent is not connected"
        );
        assert!(
            decode_inventory(Ok(response(Some(json!({"devices": []})))))
                .unwrap()
                .devices
                .is_empty()
        );
    }

    #[test]
    fn summary_keeps_partial_failures_and_sorts_by_agent() {
        let result = summarize(vec![
            RefreshFleetAgentResult {
                agent_id: "z".into(),
                status: "ok".into(),
                accepted: 2,
                error: None,
            },
            RefreshFleetAgentResult {
                agent_id: "b".into(),
                status: "error".into(),
                accepted: 0,
                error: Some("disconnected".into()),
            },
            RefreshFleetAgentResult {
                agent_id: "a".into(),
                status: "ok".into(),
                accepted: 3,
                error: None,
            },
        ]);
        assert_eq!(result.total_accepted, 5);
        assert_eq!(
            result
                .agents
                .iter()
                .map(|row| row.agent_id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b", "z"]
        );
        assert_eq!(result.agents[1].error.as_deref(), Some("disconnected"));
        assert!(summarize(vec![]).agents.is_empty());
    }

    #[tokio::test]
    async fn handler_reports_explicit_disconnected_agents_without_omitting_them() {
        use std::{collections::HashMap, sync::Arc, time::Duration};
        use tokio::sync::{Mutex, RwLock};

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
        assert!(select_agents(&state, vec![]).await.is_empty());
        let response = refresh_fleet_devices(
            State(state.clone()),
            Json(RefreshFleetDevicesRequest {
                agent_ids: vec!["missing-b".into(), "missing-a".into(), "missing-b".into()],
                timeout_ms: None,
            }),
        )
        .await
        .unwrap()
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let result: RefreshFleetDevicesResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(result.total_accepted, 0);
        assert_eq!(result.agents.len(), 2);
        assert_eq!(result.agents[0].agent_id, "missing-a");
        assert_eq!(result.agents[1].agent_id, "missing-b");
        for agent in result.agents {
            assert_eq!(agent.status, "error");
            assert_eq!(agent.accepted, 0);
            assert_eq!(
                agent.error.as_deref(),
                Some("404 Not Found: agent is not connected")
            );
        }
        assert!(state.pending.lock().await.is_empty());
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
        let invalid = store_inventory(&store, "agent", Ok(response(None))).await;
        assert_eq!(invalid.status, "error");
        assert_eq!(invalid.accepted, 0);
        assert_eq!(store.fleet_revision(), revision);
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
