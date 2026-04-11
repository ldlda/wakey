use std::collections::{BTreeMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use tracing::warn;

use crate::api::json_error;
use crate::runtime::AppState;
use crate::state::{AlertState, AuditEvent, AuditEventFilter};

#[derive(Debug, Deserialize)]
pub struct ActiveAlertsQuery {
    pub lookback_seconds: Option<u64>,
    pub timeout_threshold: Option<u64>,
    pub auth_rejected_threshold: Option<u64>,
    pub enroll_rejected_threshold: Option<u64>,
}

pub async fn active_alerts(
    State(state): State<AppState>,
    Query(query): Query<ActiveAlertsQuery>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let lookback_seconds = query.lookback_seconds.unwrap_or(900).clamp(60, 86_400);
    let timeout_threshold = query.timeout_threshold.unwrap_or(3).max(1);
    let auth_rejected_threshold = query.auth_rejected_threshold.unwrap_or(3).max(1);
    let enroll_rejected_threshold = query.enroll_rejected_threshold.unwrap_or(5).max(1);

    let now = now_unix();
    let since_unix = now.saturating_sub(lookback_seconds);

    let enrolled_agents = state.store.list_agents().await;
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
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "alerts_query_failed",
                &err.to_string(),
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
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "alerts_query_failed",
                &err.to_string(),
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
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "alerts_query_failed",
                &err.to_string(),
            )
        })?;

    let alerts = build_alerts(
        now,
        &enrolled_agents,
        &connected_agents,
        &timeout_events,
        &auth_reject_events,
        &enroll_reject_events,
        timeout_threshold,
        auth_rejected_threshold,
        enroll_rejected_threshold,
    );

    Ok((StatusCode::OK, Json(alerts)))
}

fn build_alerts(
    now_unix: u64,
    enrolled_agents: &[String],
    connected_agents: &HashSet<String>,
    timeout_events: &[AuditEvent],
    auth_reject_events: &[AuditEvent],
    enroll_reject_events: &[AuditEvent],
    timeout_threshold: u64,
    auth_rejected_threshold: u64,
    enroll_rejected_threshold: u64,
) -> Vec<AlertState> {
    let mut alerts = Vec::new();

    for agent_id in enrolled_agents {
        if connected_agents.contains(agent_id) {
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
            last_seen_unix: now_unix,
            metadata: serde_json::json!({}),
        });
    }

    let timeout_counts = count_by_agent(timeout_events);
    for (agent_id, (count, last_seen)) in timeout_counts {
        if count < timeout_threshold {
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
            threshold: timeout_threshold,
            last_seen_unix: last_seen,
            metadata: serde_json::json!({}),
        });
    }

    let auth_reject_counts = count_by_agent(auth_reject_events);
    for (agent_id, (count, last_seen)) in auth_reject_counts {
        if count < auth_rejected_threshold {
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
            threshold: auth_rejected_threshold,
            last_seen_unix: last_seen,
            metadata: serde_json::json!({}),
        });
    }

    let enroll_reject_count = enroll_reject_events.len() as u64;
    if enroll_reject_count >= enroll_rejected_threshold {
        let last_seen_unix = enroll_reject_events
            .iter()
            .map(|e| e.ts_unix)
            .max()
            .unwrap_or(now_unix);
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
            threshold: enroll_rejected_threshold,
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

    use crate::state::AuditEvent;

    use super::build_alerts;

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

        let alerts = build_alerts(200, &enrolled, &connected, &timeouts, &[], &[], 3, 3, 5);

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

        let alerts = build_alerts(
            30,
            &[],
            &HashSet::new(),
            &[],
            &auth_rejects,
            &enroll_rejects,
            3,
            3,
            5,
        );

        assert!(alerts.iter().any(
            |a| a.kind == "agent_auth_reject_spike" && a.agent_id.as_deref() == Some("agent-x")
        ));
        assert!(
            alerts
                .iter()
                .any(|a| a.kind == "enroll_reject_spike" && a.agent_id.is_none())
        );
    }
}
