use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use tokio::select;
use tokio::sync::{RwLock, broadcast};
use tokio::time::Duration;
use tracing::warn;

use crate::api::ApiError;
use crate::runtime::AppState;
use crate::state::{AlertState, AlertTransition, AuditEvent, AuditEventFilter};

struct AlertBuildContext<'a> {
    now_unix: u64,
    enrolled_agents: &'a [String],
    connected_agents: &'a HashSet<String>,
    timeout_events: &'a [AuditEvent],
    auth_reject_events: &'a [AuditEvent],
    enroll_reject_events: &'a [AuditEvent],
    timeout_threshold: u64,
    auth_rejected_threshold: u64,
    enroll_rejected_threshold: u64,
}

#[derive(Debug, Deserialize)]
pub struct AlertHistoryQuery {
    pub since_unix: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AlertRuleConfig {
    pub lookback_seconds: Option<u64>,
    pub timeout_threshold: Option<u64>,
    pub auth_rejected_threshold: Option<u64>,
    pub enroll_rejected_threshold: Option<u64>,
}

impl Default for AlertRuleConfig {
    fn default() -> Self {
        Self {
            lookback_seconds: Some(900),
            timeout_threshold: Some(3),
            auth_rejected_threshold: Some(3),
            enroll_rejected_threshold: Some(5),
        }
    }
}

impl AlertRuleConfig {
    fn validate(&self) -> Result<(), ApiError> {
        let defaults = Self::default();
        let overrides = [
            (
                "lookback_seconds",
                self.lookback_seconds,
                defaults.lookback_seconds,
            ),
            (
                "timeout_threshold",
                self.timeout_threshold,
                defaults.timeout_threshold,
            ),
            (
                "auth_rejected_threshold",
                self.auth_rejected_threshold,
                defaults.auth_rejected_threshold,
            ),
            (
                "enroll_rejected_threshold",
                self.enroll_rejected_threshold,
                defaults.enroll_rejected_threshold,
            ),
        ];
        if let Some((field, _, _)) = overrides
            .into_iter()
            .find(|(_, requested, default)| requested.is_some() && requested != default)
        {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_alert_override",
                format!("custom alert {field} is not supported; omit it or use its default value"),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Default)]
pub struct AlertsSnapshot {
    pub ts_unix: u64,
    pub alerts: Vec<AlertState>,
    pub recent_transitions: Vec<AlertTransition>,
}

/// Holds the latest alert snapshot computed by the background evaluator and
/// broadcasts each new snapshot to connected `alerts/ws` subscribers.
#[derive(Clone)]
pub struct AlertsHub {
    tx: broadcast::Sender<String>,
    status: Arc<RwLock<HubStatus>>,
}

#[derive(Default)]
struct HubStatus {
    latest_payload: Option<String>,
    latest_alerts: Vec<AlertState>,
    last_success_unix: Option<u64>,
    failure: Option<String>,
    last_health_available: Option<bool>,
}

const ALERT_SNAPSHOT_MAX_AGE_SECS: u64 = 15;

impl Default for AlertsHub {
    fn default() -> Self {
        Self::new()
    }
}

impl AlertsHub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(16);
        Self {
            tx,
            status: Arc::new(RwLock::new(HubStatus::default())),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }

    pub async fn latest_alerts(&self) -> Vec<AlertState> {
        self.status.read().await.latest_alerts.clone()
    }

    async fn mark_unavailable(&self, reason: impl Into<String>) {
        let mut status = self.status.write().await;
        if status.failure.is_some() {
            return;
        }
        status.failure = Some(reason.into());
        let reason = status.failure.as_deref().unwrap_or_default().to_owned();
        status.last_health_available = Some(false);
        self.broadcast_health(false, Some(reason));
    }

    async fn unavailable_reason(&self) -> Option<String> {
        let status = self.status.read().await;
        evaluator_unavailable_reason(&status, now_unix())
    }

    async fn resync_payloads(&self) -> (String, Option<String>) {
        let status = self.status.read().await;
        (
            encode_health(evaluator_unavailable_reason(&status, now_unix())),
            status.latest_payload.clone(),
        )
    }

    async fn broadcast_health_if_changed(&self) {
        let mut status = self.status.write().await;
        if status.last_success_unix.is_none() || status.failure.is_some() {
            return;
        }
        let reason = evaluator_unavailable_reason(&status, now_unix());
        let available = reason.is_none();
        if status.last_health_available == Some(available) {
            return;
        }
        status.last_health_available = Some(available);
        self.broadcast_health(available, reason);
    }

    fn broadcast_health(&self, available: bool, reason: Option<String>) {
        let _ = self.tx.send(encode_health_message(available, reason));
    }

    pub async fn publish(&self, snapshot: AlertsSnapshot) {
        let payload = serde_json::json!({
            "type": "alerts_snapshot",
            "ts_unix": snapshot.ts_unix,
            "alerts": snapshot.alerts,
            "recent_transitions": snapshot.recent_transitions,
        });
        let Ok(encoded) = serde_json::to_string(&payload) else {
            warn!("failed to encode alerts snapshot payload");
            return;
        };
        let mut status = self.status.write().await;
        let was_unavailable = evaluator_unavailable_reason(&status, now_unix()).is_some();
        status.latest_alerts = snapshot.alerts;
        status.latest_payload = Some(encoded.clone());
        status.last_success_unix = Some(snapshot.ts_unix);
        status.failure = None;
        let available = evaluator_unavailable_reason(&status, now_unix()).is_none();
        if available {
            status.last_health_available = Some(true);
        }
        if was_unavailable && available {
            self.broadcast_health(true, None);
        }
        let _ = self.tx.send(encoded);
    }
}

fn evaluator_unavailable_reason(status: &HubStatus, now: u64) -> Option<String> {
    if let Some(failure) = &status.failure {
        return Some(failure.clone());
    }
    match status.last_success_unix {
        Some(ts) if now.saturating_sub(ts) <= ALERT_SNAPSHOT_MAX_AGE_SECS => None,
        Some(_) => Some("alert evaluator snapshot is stale".into()),
        None => Some("alert evaluator has not produced a snapshot yet".into()),
    }
}

fn encode_health(reason: Option<String>) -> String {
    encode_health_message(reason.is_none(), reason)
}

fn encode_health_message(available: bool, reason: Option<String>) -> String {
    let mut message = serde_json::json!({
        "type": "alerts_health",
        "available": available,
    });
    if let Some(reason) = reason {
        message["reason"] = serde_json::Value::String(reason);
    }
    message.to_string()
}

/// Background task that owns alert evaluation: on a fixed ticker it recomputes
/// the snapshot, persists transitions, and publishes to subscribers so neither
/// the GET endpoint nor individual WS clients evaluate alerts themselves.
pub async fn run_alert_evaluator(state: AppState) {
    let config = AlertRuleConfig::default();
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        let alerts = match evaluate_alerts(&state, &config).await {
            Ok(alerts) => alerts,
            Err(err) => {
                warn!(code = %err.code, "failed to evaluate alerts in background task");
                state.alerts.mark_unavailable(err.message.clone()).await;
                continue;
            }
        };

        if let Err(err) = state.store.sync_alert_transitions(&alerts).await {
            warn!(error = %err, "failed to sync alert transitions in background task");
        }

        let recent_transitions = match state.store.list_alert_transitions(None, 20).await {
            Ok(history) => history,
            Err(err) => {
                warn!(error = %err, "failed to load alert transition history for snapshot");
                Vec::new()
            }
        };

        state
            .alerts
            .publish(AlertsSnapshot {
                ts_unix: now_unix(),
                alerts,
                recent_transitions,
            })
            .await;
    }
}

pub async fn active_alerts(
    State(state): State<AppState>,
    Query(config): Query<AlertRuleConfig>,
) -> Result<impl IntoResponse, ApiError> {
    config.validate()?;
    if let Some(reason) = state.alerts.unavailable_reason().await {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "alerts_unavailable",
            reason,
        ));
    }
    let alerts = state.alerts.latest_alerts().await;
    Ok((StatusCode::OK, Json(alerts)))
}

pub async fn alert_history(
    State(state): State<AppState>,
    Query(query): Query<AlertHistoryQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let history = state
        .store
        .list_alert_transitions(query.since_unix, limit)
        .await
        .map_err(|err| {
            warn!(error = %err, "failed reading alert transition history");
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "alert_history_failed",
                err.to_string(),
            )
        })?;

    Ok((StatusCode::OK, Json(history)))
}

pub async fn alerts_stream(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| alerts_stream_socket(state, socket))
}

async fn alerts_stream_socket(state: AppState, mut socket: WebSocket) {
    let mut rx = state.alerts.subscribe();
    let mut freshness_tick = tokio::time::interval(Duration::from_secs(1));
    freshness_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let (health, snapshot) = state.alerts.resync_payloads().await;
    if socket.send(Message::Text(health.into())).await.is_err() {
        return;
    }
    if let Some(encoded) = snapshot
        && socket.send(Message::Text(encoded.into())).await.is_err()
    {
        return;
    }

    loop {
        select! {
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => {}
                }
            }
            update = rx.recv() => {
                match update {
                    Ok(encoded) => {
                        if socket.send(Message::Text(encoded.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        let (health, snapshot) = state.alerts.resync_payloads().await;
                        if socket.send(Message::Text(health.into())).await.is_err() {
                            break;
                        }
                        if let Some(encoded) = snapshot
                            && socket.send(Message::Text(encoded.into())).await.is_err()
                        {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = freshness_tick.tick() => state.alerts.broadcast_health_if_changed().await,
        }
    }
}

async fn evaluate_alerts(
    state: &AppState,
    config: &AlertRuleConfig,
) -> Result<Vec<AlertState>, ApiError> {
    let lookback_seconds = config.lookback_seconds.unwrap_or(900).clamp(60, 86_400);
    let timeout_threshold = config.timeout_threshold.unwrap_or(3).max(1);
    let auth_rejected_threshold = config.auth_rejected_threshold.unwrap_or(3).max(1);
    let enroll_rejected_threshold = config.enroll_rejected_threshold.unwrap_or(5).max(1);

    let now = now_unix();
    let since_unix = now.saturating_sub(lookback_seconds);

    let enrolled_agents = state.store.list_agents().await.map_err(|err| {
        warn!(error = %err, "failed listing agents for alert evaluation");
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "alerts_query_failed",
            err.to_string(),
        )
    })?;
    let connected_agents = state
        .sessions
        .read()
        .await
        .keys()
        .cloned()
        .collect::<HashSet<_>>();

    let timeout_events = state
        .store
        .list_audit_events(AuditEventFilter {
            event_type: Some("command_result".into()),
            outcome: Some("timeout".into()),
            since_unix: Some(since_unix),
            limit: 2_000,
            ..Default::default()
        })
        .await
        .map_err(|err| {
            warn!(error = %err, "failed reading timeout audit events");
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "alerts_query_failed",
                err.to_string(),
            )
        })?;

    let auth_reject_events = state
        .store
        .list_audit_events(AuditEventFilter {
            event_type: Some("agent_ws_auth".into()),
            outcome: Some("rejected".into()),
            since_unix: Some(since_unix),
            limit: 2_000,
            ..Default::default()
        })
        .await
        .map_err(|err| {
            warn!(error = %err, "failed reading auth-rejected audit events");
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "alerts_query_failed",
                err.to_string(),
            )
        })?;

    let enroll_reject_events = state
        .store
        .list_audit_events(AuditEventFilter {
            event_type: Some("agent_enroll".into()),
            outcome: Some("rejected".into()),
            since_unix: Some(since_unix),
            limit: 2_000,
            ..Default::default()
        })
        .await
        .map_err(|err| {
            warn!(error = %err, "failed reading enroll-rejected audit events");
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "alerts_query_failed",
                err.to_string(),
            )
        })?;

    Ok(build_alerts(AlertBuildContext {
        now_unix: now,
        enrolled_agents: &enrolled_agents,
        connected_agents: &connected_agents,
        timeout_events: &timeout_events,
        auth_reject_events: &auth_reject_events,
        enroll_reject_events: &enroll_reject_events,
        timeout_threshold,
        auth_rejected_threshold,
        enroll_rejected_threshold,
    }))
}

fn build_alerts(ctx: AlertBuildContext<'_>) -> Vec<AlertState> {
    let mut alerts = Vec::new();

    for agent_id in ctx.enrolled_agents {
        if ctx.connected_agents.contains(agent_id) {
            continue;
        }
        alerts.push(AlertState {
            alert_id: format!("agent_offline:{agent_id}"),
            kind: "agent_offline".into(),
            severity: "warning".into(),
            status: "active".into(),
            agent_id: Some(agent_id.clone()),
            message: format!("agent {agent_id} is enrolled but not currently connected"),
            value: 1,
            threshold: 1,
            last_seen_unix: ctx.now_unix,
            metadata: serde_json::json!({}),
        });
    }

    let timeout_counts = count_by_agent(ctx.timeout_events);
    for (agent_id, (count, last_seen)) in timeout_counts {
        if count < ctx.timeout_threshold {
            continue;
        }
        alerts.push(AlertState {
            alert_id: format!("command_timeout_rate:{agent_id}"),
            kind: "command_timeout_rate".into(),
            severity: "critical".into(),
            status: "active".into(),
            agent_id: Some(agent_id.clone()),
            message: format!(
                "agent {agent_id} had {count} command timeout(s) within evaluation window"
            ),
            value: count,
            threshold: ctx.timeout_threshold,
            last_seen_unix: last_seen,
            metadata: serde_json::json!({}),
        });
    }

    let auth_reject_counts = count_by_agent(ctx.auth_reject_events);
    for (agent_id, (count, last_seen)) in auth_reject_counts {
        if count < ctx.auth_rejected_threshold {
            continue;
        }
        alerts.push(AlertState {
            alert_id: format!("agent_auth_reject_spike:{agent_id}"),
            kind: "agent_auth_reject_spike".into(),
            severity: "warning".into(),
            status: "active".into(),
            agent_id: Some(agent_id.clone()),
            message: format!(
                "agent {agent_id} had {count} auth rejection(s) within evaluation window"
            ),
            value: count,
            threshold: ctx.auth_rejected_threshold,
            last_seen_unix: last_seen,
            metadata: serde_json::json!({}),
        });
    }

    let enroll_reject_count = ctx.enroll_reject_events.len() as u64;
    if enroll_reject_count >= ctx.enroll_rejected_threshold {
        let last_seen_unix = ctx
            .enroll_reject_events
            .iter()
            .map(|e| e.ts_unix)
            .max()
            .unwrap_or(ctx.now_unix);
        alerts.push(AlertState {
            alert_id: "enroll_reject_spike:global".into(),
            kind: "enroll_reject_spike".into(),
            severity: "warning".into(),
            status: "active".into(),
            agent_id: None,
            message: format!(
                "enrollment endpoint saw {enroll_reject_count} rejection(s) within evaluation window"
            ),
            value: enroll_reject_count,
            threshold: ctx.enroll_rejected_threshold,
            last_seen_unix,
            metadata: serde_json::json!({}),
        });
    }

    alerts.sort_by(|a, b| {
        b.last_seen_unix
            .cmp(&a.last_seen_unix)
            .then(a.alert_id.cmp(&b.alert_id))
    });
    alerts
}

fn count_by_agent(events: &[AuditEvent]) -> BTreeMap<String, (u64, u64)> {
    let mut counts = BTreeMap::<String, (u64, u64)>::new();
    for event in events {
        let Some(agent_id) = event.agent_id.as_deref() else {
            continue;
        };
        let entry = counts.entry(agent_id.to_string()).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(1);
        entry.1 = entry.1.max(event.ts_unix);
    }
    counts
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use axum::http::StatusCode;

    use crate::state::AuditEvent;

    use super::{AlertBuildContext, AlertRuleConfig, AlertsHub, AlertsSnapshot, build_alerts};

    fn event(agent_id: Option<&str>, event_type: &str, outcome: &str, ts_unix: u64) -> AuditEvent {
        AuditEvent {
            event_id: format!("evt-{event_type}-{ts_unix}"),
            ts_unix,
            actor_type: "test".into(),
            actor_id: None,
            agent_id: agent_id.map(|s| s.to_string()),
            request_id: None,
            event_type: event_type.into(),
            outcome: outcome.into(),
            latency_ms: None,
            message: "test".into(),
            metadata: serde_json::json!({}),
        }
    }

    #[test]
    fn raises_offline_and_timeout_alerts() {
        let enrolled = vec!["agent-a".to_string(), "agent-b".to_string()];
        let connected = HashSet::from(["agent-a".to_string()]);
        let timeouts = vec![
            event(Some("agent-a"), "command_result", "timeout", 100),
            event(Some("agent-a"), "command_result", "timeout", 101),
            event(Some("agent-a"), "command_result", "timeout", 102),
        ];

        let alerts = build_alerts(AlertBuildContext {
            now_unix: 200,
            enrolled_agents: &enrolled,
            connected_agents: &connected,
            timeout_events: &timeouts,
            auth_reject_events: &[],
            enroll_reject_events: &[],
            timeout_threshold: 3,
            auth_rejected_threshold: 3,
            enroll_rejected_threshold: 5,
        });

        assert!(
            alerts
                .iter()
                .any(|a| a.kind == "agent_offline" && a.agent_id.as_deref() == Some("agent-b"))
        );
        assert!(
            alerts
                .iter()
                .any(|a| a.kind == "command_timeout_rate"
                    && a.agent_id.as_deref() == Some("agent-a"))
        );
    }

    #[test]
    fn raises_auth_and_enroll_reject_spikes() {
        let auth_rejects = vec![
            event(Some("agent-x"), "agent_ws_auth", "rejected", 10),
            event(Some("agent-x"), "agent_ws_auth", "rejected", 11),
            event(Some("agent-x"), "agent_ws_auth", "rejected", 12),
        ];
        let enroll_rejects = vec![
            event(None, "agent_enroll", "rejected", 20),
            event(None, "agent_enroll", "rejected", 21),
            event(None, "agent_enroll", "rejected", 22),
            event(None, "agent_enroll", "rejected", 23),
            event(None, "agent_enroll", "rejected", 24),
        ];

        let alerts = build_alerts(AlertBuildContext {
            now_unix: 30,
            enrolled_agents: &[],
            connected_agents: &HashSet::new(),
            timeout_events: &[],
            auth_reject_events: &auth_rejects,
            enroll_reject_events: &enroll_rejects,
            timeout_threshold: 3,
            auth_rejected_threshold: 3,
            enroll_rejected_threshold: 5,
        });

        assert!(alerts.iter().any(
            |a| a.kind == "agent_auth_reject_spike" && a.agent_id.as_deref() == Some("agent-x")
        ));
        assert!(
            alerts
                .iter()
                .any(|a| a.kind == "enroll_reject_spike" && a.agent_id.is_none())
        );
    }

    #[test]
    fn alert_rule_config_accepts_omitted_and_default_values() {
        AlertRuleConfig::default().validate().unwrap();
        AlertRuleConfig {
            lookback_seconds: None,
            timeout_threshold: None,
            auth_rejected_threshold: None,
            enroll_rejected_threshold: None,
        }
        .validate()
        .unwrap();
    }

    #[test]
    fn alert_rule_config_rejects_custom_overrides() {
        let configs = [
            AlertRuleConfig {
                lookback_seconds: Some(300),
                ..Default::default()
            },
            AlertRuleConfig {
                timeout_threshold: Some(4),
                ..Default::default()
            },
            AlertRuleConfig {
                auth_rejected_threshold: Some(4),
                ..Default::default()
            },
            AlertRuleConfig {
                enroll_rejected_threshold: Some(6),
                ..Default::default()
            },
        ];

        for config in configs {
            let error = config.validate().unwrap_err();
            assert_eq!(error.status, StatusCode::BAD_REQUEST);
            assert_eq!(error.code, "unsupported_alert_override");
            assert!(error.message.contains("not supported"));
        }
    }

    #[tokio::test]
    async fn alert_hub_reports_initial_failure_and_recovers_on_snapshot() {
        let hub = AlertsHub::new();
        assert!(hub.unavailable_reason().await.is_some());
        let mut updates = hub.subscribe();

        hub.mark_unavailable("store query failed").await;
        assert_eq!(
            hub.unavailable_reason().await.as_deref(),
            Some("store query failed")
        );
        let failure: serde_json::Value =
            serde_json::from_str(&updates.try_recv().unwrap()).unwrap();
        assert_eq!(failure["type"], "alerts_health");
        assert_eq!(failure["available"], false);
        assert_eq!(failure["reason"], "store query failed");

        hub.mark_unavailable("same outage, later attempt").await;
        assert!(
            updates.try_recv().is_err(),
            "failure notifications deduplicate"
        );

        hub.publish(AlertsSnapshot {
            ts_unix: super::now_unix(),
            alerts: Vec::new(),
            recent_transitions: Vec::new(),
        })
        .await;
        assert_eq!(hub.unavailable_reason().await, None);
        let recovery: serde_json::Value =
            serde_json::from_str(&updates.try_recv().unwrap()).unwrap();
        assert_eq!(recovery["type"], "alerts_health");
        assert_eq!(recovery["available"], true);
        assert_eq!(recovery.get("reason"), None);
        let snapshot: serde_json::Value =
            serde_json::from_str(&updates.try_recv().unwrap()).unwrap();
        assert_eq!(snapshot["type"], "alerts_snapshot");
    }

    #[tokio::test]
    async fn alert_hub_rejects_stale_snapshots() {
        let hub = AlertsHub::new();
        hub.publish(AlertsSnapshot {
            ts_unix: 0,
            alerts: Vec::new(),
            recent_transitions: Vec::new(),
        })
        .await;
        assert_eq!(
            hub.unavailable_reason().await.as_deref(),
            Some("alert evaluator snapshot is stale")
        );
    }

    #[tokio::test]
    async fn alert_hub_initial_resync_marks_cached_snapshot_stale() {
        let hub = AlertsHub::new();
        let (initial_health, initial_snapshot) = hub.resync_payloads().await;
        let initial_health: serde_json::Value = serde_json::from_str(&initial_health).unwrap();
        assert_eq!(initial_health["available"], false);
        assert_eq!(
            initial_health["reason"],
            "alert evaluator has not produced a snapshot yet"
        );
        assert!(initial_snapshot.is_none());

        hub.publish(AlertsSnapshot {
            ts_unix: 0,
            alerts: Vec::new(),
            recent_transitions: Vec::new(),
        })
        .await;
        let (stale_health, cached_snapshot) = hub.resync_payloads().await;
        let stale_health: serde_json::Value = serde_json::from_str(&stale_health).unwrap();
        assert_eq!(stale_health["available"], false);
        assert_eq!(stale_health["reason"], "alert evaluator snapshot is stale");
        assert!(cached_snapshot.is_some());
    }

    #[tokio::test]
    async fn alert_hub_broadcasts_stale_transition_once() {
        let hub = AlertsHub::new();
        let mut updates = hub.subscribe();
        hub.publish(AlertsSnapshot {
            ts_unix: 0,
            alerts: Vec::new(),
            recent_transitions: Vec::new(),
        })
        .await;
        let _snapshot = updates.try_recv().unwrap();

        hub.broadcast_health_if_changed().await;
        let stale: serde_json::Value = serde_json::from_str(&updates.try_recv().unwrap()).unwrap();
        assert_eq!(stale["available"], false);
        assert_eq!(stale["reason"], "alert evaluator snapshot is stale");
        hub.broadcast_health_if_changed().await;
        assert!(updates.try_recv().is_err());
    }
}
