use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::state::types::{
    AlertState, AlertTransition, AuditEvent, AuditEventFilter, AuditEventInput, EnrollTokenInfo,
    IssuedAgent, IssuedEnrollToken, StateStats,
};

pub struct Store {
    db_path: PathBuf,
    meta: sled::Tree,
    enroll_tokens: sled::Tree,
    agents: sled::Tree,
    audit_events: sled::Tree,
    active_alerts: sled::Tree,
    alert_transitions: sled::Tree,
}

const SCHEMA_VERSION_KEY: &[u8] = b"schema_version";
const SEEDED_ENROLL_TOKEN_PREFIX: &[u8] = b"seeded_enroll_token:";
const SCHEMA_VERSION: u32 = 1;

impl Store {
    pub async fn load_or_init(
        path: &Path,
        enroll_tokens: Vec<String>,
        seed_ttl: Duration,
    ) -> Result<Self> {
        let db_path = path.to_path_buf();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create state dir {}", parent.display()))?;
        }

        let db = sled::open(&db_path)
            .with_context(|| format!("failed to open state db {}", db_path.display()))?;
        let meta_tree = db.open_tree("meta").context("failed to open meta tree")?;
        let enroll_tree = db
            .open_tree("enroll_tokens")
            .context("failed to open enroll_tokens tree")?;
        let agents_tree = db
            .open_tree("agents")
            .context("failed to open agents tree")?;
        let audit_events_tree = db
            .open_tree("audit_events")
            .context("failed to open audit_events tree")?;
        let active_alerts_tree = db
            .open_tree("active_alerts")
            .context("failed to open active_alerts tree")?;
        let alert_transitions_tree = db
            .open_tree("alert_transitions")
            .context("failed to open alert_transitions tree")?;

        let store = Self {
            db_path,
            meta: meta_tree,
            enroll_tokens: enroll_tree,
            agents: agents_tree,
            audit_events: audit_events_tree,
            active_alerts: active_alerts_tree,
            alert_transitions: alert_transitions_tree,
        };

        store.ensure_schema_version()?;

        store.seed_bootstrap_enroll_tokens(&enroll_tokens, seed_ttl)?;

        store.gc_expired_enroll_tokens_inner()?;

        store
            .flush()
            .with_context(|| format!("failed to flush state db {}", store.db_path.display()))?;

        let enroll_tokens = store.enroll_tokens.iter().count();
        let agents = store.agents.iter().count();
        let audit_events = store.audit_events.iter().count();
        info!(
            path = %store.db_path.display(),
            enroll_tokens,
            agents,
            audit_events,
            "control-plane store ready"
        );
        Ok(store)
    }

    pub async fn enroll(&self, enroll_token: &str) -> Result<IssuedAgent> {
        let Some(raw_expiry) = self
            .enroll_tokens
            .get(enroll_token.as_bytes())
            .context("failed reading enroll token")?
        else {
            warn!("rejecting enroll attempt with invalid, expired, or consumed token");
            anyhow::bail!("invalid or already-used enroll token");
        };

        let expires_at_unix =
            decode_expiry(raw_expiry.as_ref()).context("failed decoding enroll token expiry")?;
        let now = now_unix();
        if expires_at_unix <= now {
            let _ = self.enroll_tokens.remove(enroll_token.as_bytes());
            self.flush().ok();
            warn!(
                expires_at_unix,
                now_unix = now,
                "rejecting expired enroll token"
            );
            anyhow::bail!("enroll token has expired");
        }

        self.enroll_tokens
            .remove(enroll_token.as_bytes())
            .context("failed consuming enroll token")?;

        let agent_id = format!("agent-{}", Uuid::new_v4());
        let agent_token = format!("tok-{}", Uuid::new_v4());

        self.agents
            .insert(agent_id.as_bytes(), agent_token.as_bytes())
            .context("failed persisting agent credentials")?;
        self.flush()
            .context("failed flushing state db after enroll")?;
        info!(agent_id = %agent_id, "issued persistent agent credentials");

        Ok(IssuedAgent {
            agent_id,
            agent_token,
        })
    }

    pub async fn issue_enroll_token(&self, ttl: Duration) -> Result<IssuedEnrollToken> {
        let token = format!("enr-{}", Uuid::new_v4());
        let expires_at_unix = now_unix().saturating_add(ttl.as_secs().max(1));
        self.enroll_tokens
            .insert(token.as_bytes(), &expires_at_unix.to_le_bytes())
            .context("failed persisting enroll token")?;
        self.flush()
            .context("failed flushing state db after token issuance")?;
        info!(expires_at_unix, "persisted new enroll token");
        Ok(IssuedEnrollToken {
            enroll_token: token,
            expires_at_unix,
        })
    }

    pub async fn list_enroll_tokens(&self, include_expired: bool) -> Result<Vec<EnrollTokenInfo>> {
        let now = now_unix();
        let mut out = Vec::new();
        for item in self.enroll_tokens.iter() {
            let (token, value) = item.context("failed iterating enroll token tree")?;
            let expires_at_unix =
                decode_expiry(value.as_ref()).context("failed decoding token expiry")?;
            let expired = expires_at_unix <= now;
            if !include_expired && expired {
                continue;
            }
            let enroll_token =
                String::from_utf8(token.to_vec()).context("invalid utf-8 enroll token in db")?;
            out.push(EnrollTokenInfo {
                enroll_token,
                expires_at_unix,
                expired,
            });
        }
        out.sort_by(|a, b| {
            a.expires_at_unix
                .cmp(&b.expires_at_unix)
                .then(a.enroll_token.cmp(&b.enroll_token))
        });
        Ok(out)
    }

    pub async fn revoke_enroll_token(&self, token: &str) -> Result<bool> {
        let removed = self
            .enroll_tokens
            .remove(token.as_bytes())
            .context("failed removing enroll token")?
            .is_some();
        if removed {
            self.flush()
                .context("failed flushing db after enroll token revoke")?;
        }
        Ok(removed)
    }

    pub async fn revoke_agent(&self, agent_id: &str) -> Result<bool> {
        let removed = self
            .agents
            .remove(agent_id.as_bytes())
            .context("failed removing agent credentials")?
            .is_some();
        if removed {
            self.flush()
                .context("failed flushing db after agent revoke")?;
        }
        Ok(removed)
    }

    pub async fn stats(&self) -> Result<StateStats> {
        let now = now_unix();
        let mut enroll_token_count = 0usize;
        let mut expired_enroll_token_count = 0usize;
        for item in self.enroll_tokens.iter() {
            let (_, value) = item.context("failed iterating enroll token tree")?;
            let expires_at = decode_expiry(value.as_ref())
                .context("failed decoding token expiry during stats")?;
            enroll_token_count = enroll_token_count.saturating_add(1);
            if expires_at <= now {
                expired_enroll_token_count = expired_enroll_token_count.saturating_add(1);
            }
        }

        Ok(StateStats {
            db_path: self.db_path.clone(),
            schema_version: self.schema_version()?,
            agent_count: self.agents.iter().count(),
            enroll_token_count,
            expired_enroll_token_count,
        })
    }

    #[cfg_attr(not(unix), allow(dead_code))]
    pub async fn gc_expired_enroll_tokens(&self) -> Result<u64> {
        self.gc_expired_enroll_tokens_inner()
    }

    #[cfg_attr(not(unix), allow(dead_code))]
    pub async fn reload_from_disk(&self) -> Result<()> {
        // sled is durable and read-through; explicit reload is a no-op.
        info!(path = %self.db_path.display(), "reload requested; sled backend does not require in-memory reload");
        Ok(())
    }

    pub async fn verify_agent_token(&self, agent_id: &str, token: &str) -> bool {
        match self.agents.get(agent_id.as_bytes()) {
            Ok(Some(value)) => value.as_ref() == token.as_bytes(),
            Ok(None) => false,
            Err(err) => {
                warn!(error = %err, agent_id = %agent_id, "failed to read agent token from state db");
                false
            }
        }
    }

    pub async fn list_agents(&self) -> Vec<String> {
        let mut out = self
            .agents
            .iter()
            .filter_map(|item| item.ok())
            .filter_map(|(key, _)| String::from_utf8(key.to_vec()).ok())
            .collect::<Vec<_>>();
        out.sort();
        out
    }

    pub async fn append_audit_event(&self, input: AuditEventInput) -> Result<AuditEvent> {
        let event = AuditEvent {
            event_id: format!("evt-{}", Uuid::new_v4()),
            ts_unix: now_unix(),
            actor_type: input.actor_type,
            actor_id: input.actor_id,
            agent_id: input.agent_id,
            request_id: input.request_id,
            event_type: input.event_type,
            outcome: input.outcome,
            latency_ms: input.latency_ms,
            message: input.message,
            metadata: input.metadata,
        };

        let key = format!("{:020}:{}", event.ts_unix, event.event_id);
        let value = serde_json::to_vec(&event).context("failed to encode audit event")?;
        self.audit_events
            .insert(key.as_bytes(), value)
            .context("failed persisting audit event")?;
        self.flush()
            .context("failed flushing state db after audit append")?;
        Ok(event)
    }

    pub async fn list_audit_events(&self, filter: AuditEventFilter) -> Result<Vec<AuditEvent>> {
        let limit = filter.limit.clamp(1, 500);
        let mut out = Vec::new();

        for item in self.audit_events.iter().rev() {
            let (_, raw) = item.context("failed iterating audit event tree")?;
            let event: AuditEvent =
                serde_json::from_slice(raw.as_ref()).context("failed decoding audit event")?;

            if !matches_audit_filter(&event, &filter) {
                continue;
            }

            out.push(event);
            if out.len() >= limit {
                break;
            }
        }

        Ok(out)
    }

    pub async fn sync_alert_transitions(
        &self,
        current: &[AlertState],
    ) -> Result<Vec<AlertTransition>> {
        let mut previous = std::collections::HashMap::<String, AlertState>::new();
        for item in self.active_alerts.iter() {
            let (_, raw) = item.context("failed iterating active_alerts tree")?;
            let alert: AlertState =
                serde_json::from_slice(raw.as_ref()).context("failed decoding active alert")?;
            previous.insert(alert.alert_id.clone(), alert);
        }

        let mut current_map = std::collections::HashMap::<String, AlertState>::new();
        for alert in current {
            current_map.insert(alert.alert_id.clone(), alert.clone());
        }

        let now = now_unix();
        let mut transitions = Vec::new();

        for (alert_id, current_alert) in &current_map {
            if previous.contains_key(alert_id) {
                continue;
            }
            transitions.push(AlertTransition {
                transition_id: format!("atr-{}", Uuid::new_v4()),
                ts_unix: now,
                alert_id: current_alert.alert_id.clone(),
                kind: current_alert.kind.clone(),
                agent_id: current_alert.agent_id.clone(),
                from_status: None,
                to_status: "active".into(),
                message: current_alert.message.clone(),
                metadata: current_alert.metadata.clone(),
            });
        }

        for (alert_id, previous_alert) in &previous {
            if current_map.contains_key(alert_id) {
                continue;
            }
            transitions.push(AlertTransition {
                transition_id: format!("atr-{}", Uuid::new_v4()),
                ts_unix: now,
                alert_id: previous_alert.alert_id.clone(),
                kind: previous_alert.kind.clone(),
                agent_id: previous_alert.agent_id.clone(),
                from_status: Some("active".into()),
                to_status: "resolved".into(),
                message: format!("resolved alert {}", previous_alert.alert_id),
                metadata: previous_alert.metadata.clone(),
            });
        }

        for item in self.active_alerts.iter() {
            let (key, _) = item.context("failed iterating active_alerts keys")?;
            self.active_alerts
                .remove(key)
                .context("failed clearing active alert snapshot")?;
        }

        for alert in current {
            let key = alert.alert_id.as_bytes();
            let value = serde_json::to_vec(alert).context("failed encoding active alert")?;
            self.active_alerts
                .insert(key, value)
                .context("failed writing active alert snapshot")?;
        }

        for transition in &transitions {
            let key = format!("{:020}:{}", transition.ts_unix, transition.transition_id);
            let value =
                serde_json::to_vec(transition).context("failed encoding alert transition")?;
            self.alert_transitions
                .insert(key.as_bytes(), value)
                .context("failed persisting alert transition")?;
        }

        self.flush()
            .context("failed flushing state db after alert sync")?;
        Ok(transitions)
    }

    pub async fn list_alert_transitions(
        &self,
        since_unix: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AlertTransition>> {
        let limit = limit.clamp(1, 500);
        let mut out = Vec::new();
        for item in self.alert_transitions.iter().rev() {
            let (_, raw) = item.context("failed iterating alert transition tree")?;
            let transition: AlertTransition =
                serde_json::from_slice(raw.as_ref()).context("failed decoding alert transition")?;
            if let Some(since) = since_unix
                && transition.ts_unix < since
            {
                continue;
            }
            out.push(transition);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    fn flush(&self) -> Result<()> {
        self.meta.flush().context("failed to flush meta tree")?;
        self.enroll_tokens
            .flush()
            .context("failed to flush enroll token tree")?;
        self.agents.flush().context("failed to flush agents tree")?;
        self.audit_events
            .flush()
            .context("failed to flush audit event tree")?;
        self.active_alerts
            .flush()
            .context("failed to flush active alerts tree")?;
        self.alert_transitions
            .flush()
            .context("failed to flush alert transitions tree")?;
        debug!(path = %self.db_path.display(), "flushed sled state db");
        Ok(())
    }

    fn seed_bootstrap_enroll_tokens(
        &self,
        enroll_tokens: &[String],
        seed_ttl: Duration,
    ) -> Result<()> {
        for token in enroll_tokens {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }

            let marker_key = seeded_enroll_token_key(token);
            if self.meta.contains_key(&marker_key).with_context(|| {
                format!(
                    "failed reading bootstrap marker in {}",
                    self.db_path.display()
                )
            })? {
                continue;
            }

            let expires_at = now_unix().saturating_add(seed_ttl.as_secs().max(1));
            self.enroll_tokens
                .insert(token.as_bytes(), &expires_at.to_le_bytes())
                .with_context(|| {
                    format!(
                        "failed to seed enroll token into {}",
                        self.db_path.display()
                    )
                })?;
            self.meta
                .insert(marker_key, &expires_at.to_le_bytes())
                .with_context(|| {
                    format!(
                        "failed to persist bootstrap marker into {}",
                        self.db_path.display()
                    )
                })?;
        }
        Ok(())
    }

    fn gc_expired_enroll_tokens_inner(&self) -> Result<u64> {
        let now = now_unix();
        let mut removed = 0u64;
        for item in self.enroll_tokens.iter() {
            let (token, value) = item.context("failed iterating enroll token tree")?;
            let expires_at =
                decode_expiry(value.as_ref()).context("failed decoding token expiry during gc")?;
            if expires_at <= now {
                self.enroll_tokens
                    .remove(token)
                    .context("failed removing expired enroll token")?;
                removed = removed.saturating_add(1);
            }
        }
        if removed > 0 {
            self.flush().context("failed flushing db after gc")?;
            info!(removed, "garbage-collected expired enroll tokens");
        }
        Ok(removed)
    }

    fn ensure_schema_version(&self) -> Result<()> {
        match self
            .meta
            .get(SCHEMA_VERSION_KEY)
            .context("failed reading schema version")?
        {
            Some(raw) => {
                let schema =
                    decode_schema(raw.as_ref()).context("failed decoding schema version")?;
                if schema != SCHEMA_VERSION {
                    anyhow::bail!(
                        "unsupported db schema version {}; expected {}",
                        schema,
                        SCHEMA_VERSION
                    );
                }
            }
            None => {
                self.meta
                    .insert(SCHEMA_VERSION_KEY, &SCHEMA_VERSION.to_le_bytes())
                    .context("failed writing schema version")?;
                self.flush()
                    .context("failed flushing db after schema init")?;
                info!(
                    schema_version = SCHEMA_VERSION,
                    "initialized state schema version"
                );
            }
        }
        Ok(())
    }

    fn schema_version(&self) -> Result<u32> {
        let raw = self
            .meta
            .get(SCHEMA_VERSION_KEY)
            .context("failed reading schema version")?
            .ok_or_else(|| anyhow::anyhow!("missing schema version in state db"))?;
        decode_schema(raw.as_ref()).context("failed decoding schema version")
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn decode_expiry(raw: &[u8]) -> Result<u64> {
    if raw.len() != 8 {
        anyhow::bail!("invalid token expiry length {}", raw.len());
    }
    let mut arr = [0u8; 8];
    arr.copy_from_slice(raw);
    Ok(u64::from_le_bytes(arr))
}

fn decode_schema(raw: &[u8]) -> Result<u32> {
    if raw.len() != 4 {
        anyhow::bail!("invalid schema version length {}", raw.len());
    }
    let mut arr = [0u8; 4];
    arr.copy_from_slice(raw);
    Ok(u32::from_le_bytes(arr))
}

fn seeded_enroll_token_key(token: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(SEEDED_ENROLL_TOKEN_PREFIX.len() + token.len());
    key.extend_from_slice(SEEDED_ENROLL_TOKEN_PREFIX);
    key.extend_from_slice(token.as_bytes());
    key
}

fn matches_audit_filter(event: &AuditEvent, filter: &AuditEventFilter) -> bool {
    if let Some(agent_id) = filter.agent_id.as_deref()
        && event.agent_id.as_deref() != Some(agent_id)
    {
        return false;
    }
    if let Some(request_id) = filter.request_id.as_deref()
        && event.request_id.as_deref() != Some(request_id)
    {
        return false;
    }
    if let Some(event_type) = filter.event_type.as_deref()
        && event.event_type != event_type
    {
        return false;
    }
    if let Some(outcome) = filter.outcome.as_deref()
        && event.outcome != outcome
    {
        return false;
    }
    if let Some(since_unix) = filter.since_unix
        && event.ts_unix < since_unix
    {
        return false;
    }
    if let Some(until_unix) = filter.until_unix
        && event.ts_unix > until_unix
    {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use super::Store;

    async fn make_store() -> (Store, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("wakey-cp-store-test-{}", uuid::Uuid::new_v4()));
        let db_path = dir.join("state.db");
        let store = Store::load_or_init(&db_path, Vec::new(), Duration::from_secs(60))
            .await
            .expect("store should initialize");
        (store, dir)
    }

    fn cleanup_dir(path: &std::path::Path) {
        let _ = fs::remove_dir_all(path);
    }

    #[tokio::test]
    async fn gc_removes_expired_tokens() {
        let (store, dir) = make_store().await;
        let key = b"enr-expired-gc-test";
        let expired = 1u64.to_le_bytes();
        store
            .enroll_tokens
            .insert(key, &expired)
            .expect("insert should succeed");

        let removed = store
            .gc_expired_enroll_tokens()
            .await
            .expect("gc should succeed");

        assert_eq!(removed, 1);
        assert!(
            store
                .enroll_tokens
                .get(key)
                .expect("read should succeed")
                .is_none()
        );
        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn enroll_rejects_expired_token() {
        let (store, dir) = make_store().await;
        let key = b"enr-expired-enroll-test";
        let expired = 1u64.to_le_bytes();
        store
            .enroll_tokens
            .insert(key, &expired)
            .expect("insert should succeed");

        let err = store
            .enroll("enr-expired-enroll-test")
            .await
            .expect_err("expired token should be rejected");

        assert!(err.to_string().contains("expired"));
        assert!(
            store
                .enroll_tokens
                .get(key)
                .expect("read should succeed")
                .is_none()
        );
        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn stats_counts_agents_and_expired_tokens() {
        let (store, dir) = make_store().await;
        store
            .enroll_tokens
            .insert(b"enr-valid-test", &(u64::MAX - 10).to_le_bytes())
            .expect("insert valid should succeed");

        let _issued = store
            .issue_enroll_token(Duration::from_secs(60))
            .await
            .expect("issue should succeed");

        store
            .enroll_tokens
            .insert(b"enr-expired-stats-test", &1u64.to_le_bytes())
            .expect("insert expired should succeed");

        let issued_agent = store
            .enroll("enr-valid-test")
            .await
            .expect("enroll should succeed for valid token");
        assert!(!issued_agent.agent_id.is_empty());

        let stats = store.stats().await.expect("stats should succeed");

        assert_eq!(stats.agent_count, 1);
        assert_eq!(stats.enroll_token_count, 2);
        assert_eq!(stats.expired_enroll_token_count, 1);
        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn revoke_agent_removes_credentials() {
        let (store, dir) = make_store().await;

        store
            .enroll_tokens
            .insert(b"enr-revoke-agent-test", &(u64::MAX - 10).to_le_bytes())
            .expect("insert should succeed");

        let issued = store
            .enroll("enr-revoke-agent-test")
            .await
            .expect("enroll should succeed");

        assert!(
            store
                .verify_agent_token(&issued.agent_id, &issued.agent_token)
                .await
        );

        let removed = store
            .revoke_agent(&issued.agent_id)
            .await
            .expect("revoke should succeed");
        assert!(removed);
        assert!(
            !store
                .verify_agent_token(&issued.agent_id, &issued.agent_token)
                .await
        );

        let removed_again = store
            .revoke_agent(&issued.agent_id)
            .await
            .expect("second revoke should succeed");
        assert!(!removed_again);

        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn audit_events_append_and_filter() {
        let (store, dir) = make_store().await;

        store
            .append_audit_event(crate::state::AuditEventInput {
                actor_type: "admin_api".into(),
                actor_id: None,
                agent_id: Some("agent-1".into()),
                request_id: Some("req-1".into()),
                event_type: "command_result".into(),
                outcome: "ok".into(),
                latency_ms: Some(12),
                message: "command completed".into(),
                metadata: serde_json::json!({"command":"devs"}),
            })
            .await
            .expect("append first event should succeed");

        store
            .append_audit_event(crate::state::AuditEventInput {
                actor_type: "agent".into(),
                actor_id: Some("agent-2".into()),
                agent_id: Some("agent-2".into()),
                request_id: Some("req-2".into()),
                event_type: "agent_ws_auth".into(),
                outcome: "rejected".into(),
                latency_ms: None,
                message: "auth rejected".into(),
                metadata: serde_json::json!({}),
            })
            .await
            .expect("append second event should succeed");

        let all = store
            .list_audit_events(crate::state::AuditEventFilter {
                limit: 10,
                ..Default::default()
            })
            .await
            .expect("list all should succeed");
        assert_eq!(all.len(), 2);

        let filtered = store
            .list_audit_events(crate::state::AuditEventFilter {
                agent_id: Some("agent-1".into()),
                event_type: Some("command_result".into()),
                outcome: Some("ok".into()),
                limit: 10,
                ..Default::default()
            })
            .await
            .expect("filtered list should succeed");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].request_id.as_deref(), Some("req-1"));
        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn alert_transitions_track_open_and_resolve() {
        let (store, dir) = make_store().await;
        let alert = crate::state::AlertState {
            alert_id: "agent_offline:agent-a".into(),
            kind: "agent_offline".into(),
            severity: "warning".into(),
            status: "active".into(),
            agent_id: Some("agent-a".into()),
            message: "agent agent-a offline".into(),
            value: 1,
            threshold: 1,
            last_seen_unix: 10,
            metadata: serde_json::json!({}),
        };

        let opened = store
            .sync_alert_transitions(std::slice::from_ref(&alert))
            .await
            .expect("open transition should succeed");
        assert_eq!(opened.len(), 1);
        assert_eq!(opened[0].to_status, "active");

        let resolved = store
            .sync_alert_transitions(&[])
            .await
            .expect("resolve transition should succeed");
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].to_status, "resolved");

        let history = store
            .list_alert_transitions(None, 10)
            .await
            .expect("history should load");
        assert!(history.len() >= 2);
        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn bootstrap_seed_tokens_are_not_reseeded_after_consumption() {
        let dir =
            std::env::temp_dir().join(format!("wakey-cp-store-test-{}", uuid::Uuid::new_v4()));
        let db_path = dir.join("state.db");

        let first = Store::load_or_init(
            &db_path,
            vec!["enr-bootstrap-once".to_string()],
            Duration::from_secs(60),
        )
        .await
        .expect("initial store should initialize");

        let issued = first
            .enroll("enr-bootstrap-once")
            .await
            .expect("bootstrap token should enroll once");
        assert!(!issued.agent_id.is_empty());

        drop(first);

        let second = Store::load_or_init(
            &db_path,
            vec!["enr-bootstrap-once".to_string()],
            Duration::from_secs(60),
        )
        .await
        .expect("reloaded store should initialize");

        let err = second
            .enroll("enr-bootstrap-once")
            .await
            .expect_err("bootstrap token should not resurrect after restart");
        assert!(
            err.to_string()
                .contains("invalid or already-used enroll token")
        );

        cleanup_dir(&dir);
    }
}
