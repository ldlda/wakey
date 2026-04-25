use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};
use tracing::{info, warn};
use uuid::Uuid;

use crate::state::types::{
    AlertState, AlertTransition, AuditEvent, AuditEventFilter, AuditEventInput, EnrollTokenInfo,
    IssuedAgent, IssuedEnrollToken, StateStats,
};

pub struct Store {
    db_path: PathBuf,
    pool: SqlitePool,
}

const SCHEMA_VERSION_KEY: &str = "schema_version";
const SEEDED_ENROLL_TOKEN_PREFIX: &str = "seeded_enroll_token:";
const SCHEMA_VERSION: u32 = 1;

impl Store {
    pub async fn load_or_init(
        path: &Path,
        enroll_tokens: Vec<String>,
        seed_ttl: Duration,
    ) -> Result<Self> {
        if path.is_dir() {
            anyhow::bail!(
                "state_file {} is a directory, which looks like a legacy sled store; run `wakey-control-plane import-sled-state --from-sled-state {} --to-state-file <sqlite-file>` and update state_file",
                path.display(),
                path.display()
            );
        }

        let db_path = path.to_path_buf();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create state dir {}", parent.display()))?;
        }

        let pool = open_sqlite_pool(&db_path).await?;
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .with_context(|| format!("failed to migrate state db {}", db_path.display()))?;

        let store = Self { db_path, pool };

        store.ensure_schema_version().await?;
        store
            .seed_bootstrap_enroll_tokens(&enroll_tokens, seed_ttl)
            .await?;
        store.gc_expired_enroll_tokens_inner().await?;

        let enroll_tokens = sql_count(&store.pool, "enroll_tokens").await?;
        let agents = sql_count(&store.pool, "agents").await?;
        let audit_events = sql_count(&store.pool, "audit_events").await?;
        info!(
            path = %store.db_path.display(),
            enroll_tokens,
            agents,
            audit_events,
            "control-plane store ready"
        );
        Ok(store)
    }

    pub async fn import_sled_state(
        from_sled_state: &Path,
        to_state_file: &Path,
        force: bool,
    ) -> Result<()> {
        if !from_sled_state.is_dir() {
            anyhow::bail!(
                "legacy sled state {} does not exist or is not a directory",
                from_sled_state.display()
            );
        }
        if to_state_file.is_dir() {
            anyhow::bail!(
                "target state file {} is a directory",
                to_state_file.display()
            );
        }
        if to_state_file.exists()
            && to_state_file
                .metadata()
                .with_context(|| format!("failed to stat {}", to_state_file.display()))?
                .len()
                > 0
        {
            if !force {
                anyhow::bail!(
                    "target SQLite state file {} already exists and is non-empty; re-run with --force to overwrite",
                    to_state_file.display()
                );
            }
            std::fs::remove_file(to_state_file)
                .with_context(|| format!("failed to remove {}", to_state_file.display()))?;
        }

        let legacy = sled::open(from_sled_state).with_context(|| {
            format!(
                "failed to open legacy sled state {}",
                from_sled_state.display()
            )
        })?;
        let store = Store::load_or_init(to_state_file, Vec::new(), Duration::from_secs(1)).await?;

        import_tree_raw(&store.pool, &legacy, "meta", "meta", "key", "value").await?;
        import_enroll_tokens(&store.pool, &legacy).await?;
        import_agents(&store.pool, &legacy).await?;
        import_agent_meta(&store.pool, &legacy).await?;
        import_json_tree(
            &store.pool,
            &legacy,
            "audit_events",
            "audit_events",
            "event_key",
            "event_json",
            |raw| {
                let event: AuditEvent = serde_json::from_slice(raw)?;
                Ok(vec![
                    ("ts_unix", event.ts_unix.to_string()),
                    ("agent_id", event.agent_id.unwrap_or_default()),
                    ("request_id", event.request_id.unwrap_or_default()),
                    ("event_type", event.event_type),
                    ("outcome", event.outcome),
                ])
            },
        )
        .await?;
        import_simple_json_tree(
            &store.pool,
            &legacy,
            "active_alerts",
            "active_alerts",
            "alert_id",
            "alert_json",
        )
        .await?;
        import_json_tree(
            &store.pool,
            &legacy,
            "alert_transitions",
            "alert_transitions",
            "transition_key",
            "transition_json",
            |raw| {
                let transition: AlertTransition = serde_json::from_slice(raw)?;
                Ok(vec![("ts_unix", transition.ts_unix.to_string())])
            },
        )
        .await?;

        info!(
            from = %from_sled_state.display(),
            to = %to_state_file.display(),
            "imported legacy sled state into SQLite"
        );
        Ok(())
    }

    pub async fn enroll(&self, enroll_token: &str) -> Result<IssuedAgent> {
        let mut tx = self
            .pool
            .begin()
            .await
            .context("failed starting enroll transaction")?;
        let expires_at_unix = sqlx::query_scalar::<_, i64>(
            "SELECT expires_at_unix FROM enroll_tokens WHERE token = ?1",
        )
        .bind(enroll_token)
        .fetch_optional(&mut *tx)
        .await
        .context("failed reading enroll token")?;

        let Some(expires_at_unix) = expires_at_unix else {
            warn!("rejecting enroll attempt with invalid, expired, or consumed token");
            anyhow::bail!("invalid or already-used enroll token");
        };

        let now = now_unix();
        if expires_at_unix as u64 <= now {
            sqlx::query("DELETE FROM enroll_tokens WHERE token = ?1")
                .bind(enroll_token)
                .execute(&mut *tx)
                .await
                .context("failed removing expired enroll token")?;
            tx.commit()
                .await
                .context("failed committing expired enroll token removal")?;
            warn!(
                expires_at_unix,
                now_unix = now,
                "rejecting expired enroll token"
            );
            anyhow::bail!("enroll token has expired");
        }

        sqlx::query("DELETE FROM enroll_tokens WHERE token = ?1")
            .bind(enroll_token)
            .execute(&mut *tx)
            .await
            .context("failed consuming enroll token")?;

        let agent_id = format!("agent-{}", Uuid::new_v4());
        let agent_token = format!("tok-{}", Uuid::new_v4());

        sqlx::query("INSERT INTO agents (agent_id, agent_token) VALUES (?1, ?2)")
            .bind(&agent_id)
            .bind(&agent_token)
            .execute(&mut *tx)
            .await
            .context("failed persisting agent credentials")?;

        tx.commit()
            .await
            .context("failed committing state db after enroll")?;
        info!(agent_id = %agent_id, "issued persistent agent credentials");

        Ok(IssuedAgent {
            agent_id,
            agent_token,
        })
    }

    pub async fn issue_enroll_token(&self, ttl: Duration) -> Result<IssuedEnrollToken> {
        let token = format!("enr-{}", Uuid::new_v4());
        let expires_at_unix = now_unix().saturating_add(ttl.as_secs().max(1));
        sqlx::query("INSERT INTO enroll_tokens (token, expires_at_unix) VALUES (?1, ?2)")
            .bind(&token)
            .bind(i64::try_from(expires_at_unix).context("expiry does not fit SQLite integer")?)
            .execute(&self.pool)
            .await
            .context("failed persisting enroll token")?;
        info!(expires_at_unix, "persisted new enroll token");
        Ok(IssuedEnrollToken {
            enroll_token: token,
            expires_at_unix,
        })
    }

    pub async fn list_enroll_tokens(&self) -> Result<Vec<EnrollTokenInfo>> {
        let now = now_unix();
        let rows = sqlx::query(
            "SELECT token, expires_at_unix FROM enroll_tokens ORDER BY expires_at_unix, token",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed listing enroll tokens")?;

        rows.into_iter()
            .map(|row| {
                let enroll_token: String = row.try_get("token")?;
                let expires_at_unix: i64 = row.try_get("expires_at_unix")?;
                let expires_at_unix =
                    u64::try_from(expires_at_unix).context("negative token expiry in state db")?;
                Ok(EnrollTokenInfo {
                    enroll_token,
                    expires_at_unix,
                    expired: expires_at_unix <= now,
                })
            })
            .collect()
    }

    pub async fn revoke_enroll_token(&self, token: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM enroll_tokens WHERE token = ?1")
            .bind(token)
            .execute(&self.pool)
            .await
            .context("failed removing enroll token")?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn revoke_agent(&self, agent_id: &str) -> Result<bool> {
        let mut tx = self
            .pool
            .begin()
            .await
            .context("failed starting revoke transaction")?;
        let result = sqlx::query("DELETE FROM agents WHERE agent_id = ?1")
            .bind(agent_id)
            .execute(&mut *tx)
            .await
            .context("failed removing agent credentials")?;
        if result.rows_affected() > 0 {
            sqlx::query("DELETE FROM agent_meta WHERE agent_id = ?1")
                .bind(agent_id)
                .execute(&mut *tx)
                .await
                .context("failed removing agent metadata")?;
        }
        tx.commit()
            .await
            .context("failed committing db after agent revoke")?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn set_agent_nickname(&self, agent_id: &str, nickname: Option<&str>) -> Result<bool> {
        let exists =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agents WHERE agent_id = ?1")
                .bind(agent_id)
                .fetch_one(&self.pool)
                .await
                .context("failed checking agent existence")?;
        if exists == 0 {
            return Ok(false);
        }

        let normalized = nickname.map(str::trim).filter(|v| !v.is_empty());
        if let Some(value) = normalized {
            sqlx::query(
                "INSERT INTO agent_meta (agent_id, nickname) VALUES (?1, ?2)
                 ON CONFLICT(agent_id) DO UPDATE SET nickname = excluded.nickname",
            )
            .bind(agent_id)
            .bind(value)
            .execute(&self.pool)
            .await
            .context("failed persisting agent nickname")?;
        } else {
            sqlx::query("DELETE FROM agent_meta WHERE agent_id = ?1")
                .bind(agent_id)
                .execute(&self.pool)
                .await
                .context("failed clearing agent nickname")?;
        }

        Ok(true)
    }

    pub async fn stats(&self) -> Result<StateStats> {
        let now = i64::try_from(now_unix()).context("current time does not fit SQLite integer")?;
        let enroll_token_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM enroll_tokens")
            .fetch_one(&self.pool)
            .await
            .context("failed counting enroll tokens")?;
        let expired_enroll_token_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM enroll_tokens WHERE expires_at_unix <= ?1",
        )
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .context("failed counting expired enroll tokens")?;
        let agent_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agents")
            .fetch_one(&self.pool)
            .await
            .context("failed counting agents")?;

        Ok(StateStats {
            db_path: self.db_path.clone(),
            schema_version: self.schema_version().await?,
            agent_count: usize::try_from(agent_count).context("agent count overflow")?,
            enroll_token_count: usize::try_from(enroll_token_count)
                .context("enroll token count overflow")?,
            expired_enroll_token_count: usize::try_from(expired_enroll_token_count)
                .context("expired enroll token count overflow")?,
        })
    }

    #[cfg_attr(not(unix), allow(dead_code))]
    pub async fn gc_expired_enroll_tokens(&self) -> Result<u64> {
        self.gc_expired_enroll_tokens_inner().await
    }

    #[cfg_attr(not(unix), allow(dead_code))]
    pub async fn reload_from_disk(&self) -> Result<()> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .context("failed validating SQLite state connection")?;
        info!(path = %self.db_path.display(), "reload requested; SQLite backend does not require in-memory reload");
        Ok(())
    }

    pub async fn verify_agent_token(&self, agent_id: &str, token: &str) -> bool {
        match sqlx::query_scalar::<_, String>("SELECT agent_token FROM agents WHERE agent_id = ?1")
            .bind(agent_id)
            .fetch_optional(&self.pool)
            .await
        {
            Ok(Some(value)) => value == token,
            Ok(None) => false,
            Err(err) => {
                warn!(error = %err, agent_id = %agent_id, "failed to read agent token from state db");
                false
            }
        }
    }

    pub async fn list_agents(&self) -> Vec<String> {
        match sqlx::query_scalar::<_, String>("SELECT agent_id FROM agents ORDER BY agent_id")
            .fetch_all(&self.pool)
            .await
        {
            Ok(out) => out,
            Err(err) => {
                warn!(error = %err, "failed to list agents from state db");
                Vec::new()
            }
        }
    }

    pub async fn list_agents_with_nicknames(&self) -> Vec<(String, Option<String>)> {
        let rows = sqlx::query(
            "SELECT agents.agent_id, agent_meta.nickname
             FROM agents
             LEFT JOIN agent_meta ON agent_meta.agent_id = agents.agent_id
             ORDER BY agents.agent_id",
        )
        .fetch_all(&self.pool)
        .await;

        match rows {
            Ok(rows) => rows
                .into_iter()
                .filter_map(|row| {
                    let agent_id: String = row.try_get("agent_id").ok()?;
                    let nickname: Option<String> = row
                        .try_get::<Option<String>, _>("nickname")
                        .ok()
                        .flatten()
                        .map(|v| v.trim().to_string())
                        .filter(|v| !v.is_empty());
                    Some((agent_id, nickname))
                })
                .collect(),
            Err(err) => {
                warn!(error = %err, "failed to list agents from state db");
                Vec::new()
            }
        }
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
        let value = serde_json::to_string(&event).context("failed to encode audit event")?;
        sqlx::query(
            "INSERT INTO audit_events
             (event_key, event_json, ts_unix, agent_id, request_id, event_type, outcome)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(&key)
        .bind(value)
        .bind(i64::try_from(event.ts_unix).context("audit timestamp overflow")?)
        .bind(&event.agent_id)
        .bind(&event.request_id)
        .bind(&event.event_type)
        .bind(&event.outcome)
        .execute(&self.pool)
        .await
        .context("failed persisting audit event")?;
        Ok(event)
    }

    pub async fn list_audit_events(&self, filter: AuditEventFilter) -> Result<Vec<AuditEvent>> {
        let limit = filter.limit.clamp(1, 500);
        let mut builder =
            sqlx::QueryBuilder::new("SELECT event_json FROM audit_events WHERE 1 = 1");

        if let Some(agent_id) = filter.agent_id.as_deref() {
            builder.push(" AND agent_id = ");
            builder.push_bind(agent_id);
        }
        if let Some(request_id) = filter.request_id.as_deref() {
            builder.push(" AND request_id = ");
            builder.push_bind(request_id);
        }
        if let Some(event_type) = filter.event_type.as_deref() {
            builder.push(" AND event_type = ");
            builder.push_bind(event_type);
        }
        if let Some(outcome) = filter.outcome.as_deref() {
            builder.push(" AND outcome = ");
            builder.push_bind(outcome);
        }
        if let Some(since_unix) = filter.since_unix {
            builder.push(" AND ts_unix >= ");
            builder.push_bind(i64::try_from(since_unix).context("since_unix overflow")?);
        }
        if let Some(until_unix) = filter.until_unix {
            builder.push(" AND ts_unix <= ");
            builder.push_bind(i64::try_from(until_unix).context("until_unix overflow")?);
        }
        builder.push(" ORDER BY event_key DESC LIMIT ");
        builder.push_bind(i64::try_from(limit).context("audit limit overflow")?);

        let rows = builder
            .build()
            .fetch_all(&self.pool)
            .await
            .context("failed listing audit events")?;

        rows.into_iter()
            .map(|row| {
                let raw: String = row.try_get("event_json")?;
                serde_json::from_str(&raw).context("failed decoding audit event")
            })
            .collect()
    }

    pub async fn sync_alert_transitions(
        &self,
        current: &[AlertState],
    ) -> Result<Vec<AlertTransition>> {
        let mut previous = std::collections::HashMap::<String, AlertState>::new();
        let rows = sqlx::query("SELECT alert_json FROM active_alerts")
            .fetch_all(&self.pool)
            .await
            .context("failed iterating active_alerts table")?;
        for row in rows {
            let raw: String = row.try_get("alert_json")?;
            let alert: AlertState =
                serde_json::from_str(&raw).context("failed decoding active alert")?;
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

        let mut tx = self
            .pool
            .begin()
            .await
            .context("failed starting alert transaction")?;
        sqlx::query("DELETE FROM active_alerts")
            .execute(&mut *tx)
            .await
            .context("failed clearing active alert snapshot")?;

        for alert in current {
            let value = serde_json::to_string(alert).context("failed encoding active alert")?;
            sqlx::query("INSERT INTO active_alerts (alert_id, alert_json) VALUES (?1, ?2)")
                .bind(&alert.alert_id)
                .bind(value)
                .execute(&mut *tx)
                .await
                .context("failed writing active alert snapshot")?;
        }

        for transition in &transitions {
            let key = format!("{:020}:{}", transition.ts_unix, transition.transition_id);
            let value =
                serde_json::to_string(transition).context("failed encoding alert transition")?;
            sqlx::query(
                "INSERT INTO alert_transitions
                 (transition_key, transition_json, ts_unix)
                 VALUES (?1, ?2, ?3)",
            )
            .bind(key)
            .bind(value)
            .bind(i64::try_from(transition.ts_unix).context("alert timestamp overflow")?)
            .execute(&mut *tx)
            .await
            .context("failed persisting alert transition")?;
        }

        tx.commit()
            .await
            .context("failed committing state db after alert sync")?;
        Ok(transitions)
    }

    pub async fn list_alert_transitions(
        &self,
        since_unix: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AlertTransition>> {
        let limit = limit.clamp(1, 500);
        let rows = if let Some(since) = since_unix {
            sqlx::query(
                "SELECT transition_json FROM alert_transitions
                 WHERE ts_unix >= ?1
                 ORDER BY transition_key DESC
                 LIMIT ?2",
            )
            .bind(i64::try_from(since).context("since_unix overflow")?)
            .bind(i64::try_from(limit).context("alert limit overflow")?)
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query(
                "SELECT transition_json FROM alert_transitions
                 ORDER BY transition_key DESC
                 LIMIT ?1",
            )
            .bind(i64::try_from(limit).context("alert limit overflow")?)
            .fetch_all(&self.pool)
            .await
        }
        .context("failed listing alert transitions")?;

        rows.into_iter()
            .map(|row| {
                let raw: String = row.try_get("transition_json")?;
                serde_json::from_str(&raw).context("failed decoding alert transition")
            })
            .collect()
    }

    async fn seed_bootstrap_enroll_tokens(
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
            let marker_exists =
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM meta WHERE key = ?1")
                    .bind(&marker_key)
                    .fetch_one(&self.pool)
                    .await
                    .context("failed reading bootstrap marker")?;
            if marker_exists > 0 {
                continue;
            }

            let expires_at = now_unix().saturating_add(seed_ttl.as_secs().max(1));
            let mut tx = self
                .pool
                .begin()
                .await
                .context("failed starting bootstrap token transaction")?;
            sqlx::query(
                "INSERT OR REPLACE INTO enroll_tokens (token, expires_at_unix) VALUES (?1, ?2)",
            )
            .bind(token)
            .bind(i64::try_from(expires_at).context("token expiry overflow")?)
            .execute(&mut *tx)
            .await
            .context("failed seeding enroll token")?;
            sqlx::query("INSERT INTO meta (key, value) VALUES (?1, ?2)")
                .bind(marker_key)
                .bind(expires_at.to_le_bytes().to_vec())
                .execute(&mut *tx)
                .await
                .context("failed persisting bootstrap marker")?;
            tx.commit()
                .await
                .context("failed committing bootstrap token transaction")?;
        }
        Ok(())
    }

    async fn gc_expired_enroll_tokens_inner(&self) -> Result<u64> {
        let now = i64::try_from(now_unix()).context("current time does not fit SQLite integer")?;
        let result = sqlx::query("DELETE FROM enroll_tokens WHERE expires_at_unix <= ?1")
            .bind(now)
            .execute(&self.pool)
            .await
            .context("failed removing expired enroll tokens")?;
        let removed = result.rows_affected();
        if removed > 0 {
            info!(removed, "garbage-collected expired enroll tokens");
        }
        Ok(removed)
    }

    async fn ensure_schema_version(&self) -> Result<()> {
        match sqlx::query_scalar::<_, Vec<u8>>("SELECT value FROM meta WHERE key = ?1")
            .bind(SCHEMA_VERSION_KEY)
            .fetch_optional(&self.pool)
            .await
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
                sqlx::query("INSERT INTO meta (key, value) VALUES (?1, ?2)")
                    .bind(SCHEMA_VERSION_KEY)
                    .bind(SCHEMA_VERSION.to_le_bytes().to_vec())
                    .execute(&self.pool)
                    .await
                    .context("failed writing schema version")?;
                info!(
                    schema_version = SCHEMA_VERSION,
                    "initialized state schema version"
                );
            }
        }
        Ok(())
    }

    async fn schema_version(&self) -> Result<u32> {
        let raw = sqlx::query_scalar::<_, Vec<u8>>("SELECT value FROM meta WHERE key = ?1")
            .bind(SCHEMA_VERSION_KEY)
            .fetch_one(&self.pool)
            .await
            .context("failed reading schema version")?;
        decode_schema(raw.as_ref()).context("failed decoding schema version")
    }
}

async fn open_sqlite_pool(path: &Path) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal);
    SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .with_context(|| format!("failed to open SQLite state db {}", path.display()))
}

async fn sql_count(pool: &SqlitePool, table: &str) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    sqlx::query_scalar::<_, i64>(&sql)
        .fetch_one(pool)
        .await
        .with_context(|| format!("failed counting {table}"))
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

fn seeded_enroll_token_key(token: &str) -> String {
    format!("{SEEDED_ENROLL_TOKEN_PREFIX}{token}")
}

async fn import_tree_raw(
    pool: &SqlitePool,
    legacy: &sled::Db,
    sled_tree: &str,
    sql_table: &str,
    key_col: &str,
    value_col: &str,
) -> Result<()> {
    let tree = legacy
        .open_tree(sled_tree)
        .with_context(|| format!("failed to open legacy {sled_tree} tree"))?;
    let sql =
        format!("INSERT OR REPLACE INTO {sql_table} ({key_col}, {value_col}) VALUES (?1, ?2)");
    for item in tree.iter() {
        let (key, value) =
            item.with_context(|| format!("failed reading legacy {sled_tree} tree"))?;
        let key = String::from_utf8(key.to_vec())
            .with_context(|| format!("legacy {sled_tree} key is not utf-8"))?;
        sqlx::query(&sql)
            .bind(key)
            .bind(value.to_vec())
            .execute(pool)
            .await
            .with_context(|| format!("failed importing legacy {sled_tree} row"))?;
    }
    Ok(())
}

async fn import_enroll_tokens(pool: &SqlitePool, legacy: &sled::Db) -> Result<()> {
    let tree = legacy
        .open_tree("enroll_tokens")
        .context("failed to open legacy enroll_tokens tree")?;
    for item in tree.iter() {
        let (token, expiry) = item.context("failed reading legacy enroll token")?;
        let token =
            String::from_utf8(token.to_vec()).context("legacy enroll token key is not utf-8")?;
        let expires_at_unix = decode_expiry(expiry.as_ref())?;
        sqlx::query(
            "INSERT OR REPLACE INTO enroll_tokens (token, expires_at_unix) VALUES (?1, ?2)",
        )
        .bind(token)
        .bind(i64::try_from(expires_at_unix).context("legacy token expiry overflow")?)
        .execute(pool)
        .await
        .context("failed importing legacy enroll token")?;
    }
    Ok(())
}

async fn import_agents(pool: &SqlitePool, legacy: &sled::Db) -> Result<()> {
    let tree = legacy
        .open_tree("agents")
        .context("failed to open legacy agents tree")?;
    for item in tree.iter() {
        let (agent_id, agent_token) = item.context("failed reading legacy agent")?;
        let agent_id =
            String::from_utf8(agent_id.to_vec()).context("legacy agent id is not utf-8")?;
        let agent_token =
            String::from_utf8(agent_token.to_vec()).context("legacy agent token is not utf-8")?;
        sqlx::query("INSERT OR REPLACE INTO agents (agent_id, agent_token) VALUES (?1, ?2)")
            .bind(agent_id)
            .bind(agent_token)
            .execute(pool)
            .await
            .context("failed importing legacy agent")?;
    }
    Ok(())
}

async fn import_agent_meta(pool: &SqlitePool, legacy: &sled::Db) -> Result<()> {
    let tree = legacy
        .open_tree("agent_meta")
        .context("failed to open legacy agent_meta tree")?;
    for item in tree.iter() {
        let (agent_id, nickname) = item.context("failed reading legacy agent metadata")?;
        let agent_id =
            String::from_utf8(agent_id.to_vec()).context("legacy agent id is not utf-8")?;
        let nickname =
            String::from_utf8(nickname.to_vec()).context("legacy nickname is not utf-8")?;
        sqlx::query("INSERT OR REPLACE INTO agent_meta (agent_id, nickname) VALUES (?1, ?2)")
            .bind(agent_id)
            .bind(nickname)
            .execute(pool)
            .await
            .context("failed importing legacy agent metadata")?;
    }
    Ok(())
}

async fn import_simple_json_tree(
    pool: &SqlitePool,
    legacy: &sled::Db,
    sled_tree: &str,
    sql_table: &str,
    key_col: &str,
    value_col: &str,
) -> Result<()> {
    let tree = legacy
        .open_tree(sled_tree)
        .with_context(|| format!("failed to open legacy {sled_tree} tree"))?;
    let sql =
        format!("INSERT OR REPLACE INTO {sql_table} ({key_col}, {value_col}) VALUES (?1, ?2)");
    for item in tree.iter() {
        let (key, value) =
            item.with_context(|| format!("failed reading legacy {sled_tree} tree"))?;
        let key = String::from_utf8(key.to_vec())
            .with_context(|| format!("legacy {sled_tree} key is not utf-8"))?;
        let value = String::from_utf8(value.to_vec())
            .with_context(|| format!("legacy {sled_tree} value is not utf-8"))?;
        sqlx::query(&sql)
            .bind(key)
            .bind(value)
            .execute(pool)
            .await
            .with_context(|| format!("failed importing legacy {sled_tree} row"))?;
    }
    Ok(())
}

async fn import_json_tree<F>(
    pool: &SqlitePool,
    legacy: &sled::Db,
    sled_tree: &str,
    sql_table: &str,
    _key_col: &str,
    _value_col: &str,
    derive_cols: F,
) -> Result<()>
where
    F: Fn(&[u8]) -> Result<Vec<(&'static str, String)>>,
{
    let tree = legacy
        .open_tree(sled_tree)
        .with_context(|| format!("failed to open legacy {sled_tree} tree"))?;
    for item in tree.iter() {
        let (key, value) =
            item.with_context(|| format!("failed reading legacy {sled_tree} tree"))?;
        let key = String::from_utf8(key.to_vec())
            .with_context(|| format!("legacy {sled_tree} key is not utf-8"))?;
        let json = String::from_utf8(value.to_vec())
            .with_context(|| format!("legacy {sled_tree} value is not utf-8"))?;
        let derived = derive_cols(json.as_bytes())?;
        match (sql_table, derived.as_slice()) {
            ("audit_events", cols) => {
                let ts_unix = i64::from_str(&cols[0].1).context("invalid audit timestamp")?;
                let agent_id = empty_to_none(&cols[1].1);
                let request_id = empty_to_none(&cols[2].1);
                sqlx::query(
                    "INSERT OR REPLACE INTO audit_events
                     (event_key, event_json, ts_unix, agent_id, request_id, event_type, outcome)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )
                .bind(key)
                .bind(json)
                .bind(ts_unix)
                .bind(agent_id)
                .bind(request_id)
                .bind(&cols[3].1)
                .bind(&cols[4].1)
                .execute(pool)
                .await
                .context("failed importing legacy audit event")?;
            }
            ("alert_transitions", cols) => {
                let ts_unix = i64::from_str(&cols[0].1).context("invalid alert timestamp")?;
                sqlx::query(
                    "INSERT OR REPLACE INTO alert_transitions
                     (transition_key, transition_json, ts_unix)
                     VALUES (?1, ?2, ?3)",
                )
                .bind(key)
                .bind(json)
                .bind(ts_unix)
                .execute(pool)
                .await
                .context("failed importing legacy alert transition")?;
            }
            _ => unreachable!("unsupported import_json_tree target"),
        }
    }
    Ok(())
}

fn empty_to_none(value: &str) -> Option<&str> {
    if value.is_empty() { None } else { Some(value) }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use super::Store;

    async fn make_store() -> (Store, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("wakey-cp-store-test-{}", uuid::Uuid::new_v4()));
        let db_path = dir.join("state.sqlite3");
        let store = Store::load_or_init(&db_path, Vec::new(), Duration::from_secs(60))
            .await
            .expect("store should initialize");
        (store, dir)
    }

    fn cleanup_dir(path: &std::path::Path) {
        let _ = fs::remove_dir_all(path);
    }

    async fn insert_token(store: &Store, token: &str, expires_at_unix: u64) {
        sqlx::query(
            "INSERT OR REPLACE INTO enroll_tokens (token, expires_at_unix) VALUES (?1, ?2)",
        )
        .bind(token)
        .bind(expires_at_unix as i64)
        .execute(&store.pool)
        .await
        .expect("insert should succeed");
    }

    #[tokio::test]
    async fn rejects_directory_state_path() {
        let dir =
            std::env::temp_dir().join(format!("wakey-cp-store-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("dir should be created");
        let err = match Store::load_or_init(&dir, Vec::new(), Duration::from_secs(60)).await {
            Ok(_) => panic!("directory path should be rejected"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("legacy sled store"));
        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn gc_removes_expired_tokens() {
        let (store, dir) = make_store().await;
        insert_token(&store, "enr-expired-gc-test", 1).await;

        let removed = store
            .gc_expired_enroll_tokens()
            .await
            .expect("gc should succeed");

        assert_eq!(removed, 1);
        let exists =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM enroll_tokens WHERE token = ?1")
                .bind("enr-expired-gc-test")
                .fetch_one(&store.pool)
                .await
                .expect("read should succeed");
        assert_eq!(exists, 0);
        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn enroll_rejects_expired_token() {
        let (store, dir) = make_store().await;
        insert_token(&store, "enr-expired-enroll-test", 1).await;

        let err = store
            .enroll("enr-expired-enroll-test")
            .await
            .expect_err("expired token should be rejected");

        assert!(err.to_string().contains("expired"));
        let exists =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM enroll_tokens WHERE token = ?1")
                .bind("enr-expired-enroll-test")
                .fetch_one(&store.pool)
                .await
                .expect("read should succeed");
        assert_eq!(exists, 0);
        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn stats_counts_agents_and_expired_tokens() {
        let (store, dir) = make_store().await;
        insert_token(&store, "enr-valid-test", i64::MAX as u64).await;

        let _issued = store
            .issue_enroll_token(Duration::from_secs(60))
            .await
            .expect("issue should succeed");

        insert_token(&store, "enr-expired-stats-test", 1).await;

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

        insert_token(&store, "enr-revoke-agent-test", i64::MAX as u64).await;

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
    async fn nickname_set_and_clear_roundtrip() {
        let (store, dir) = make_store().await;

        insert_token(&store, "enr-nickname-test", i64::MAX as u64).await;

        let issued = store
            .enroll("enr-nickname-test")
            .await
            .expect("enroll should succeed");

        let updated = store
            .set_agent_nickname(&issued.agent_id, Some("kitchen-router"))
            .await
            .expect("nickname set should succeed");
        assert!(updated);

        let listed = store.list_agents_with_nicknames().await;
        assert!(listed.iter().any(|(id, name)| {
            id == &issued.agent_id && name.as_deref() == Some("kitchen-router")
        }));

        let cleared = store
            .set_agent_nickname(&issued.agent_id, None)
            .await
            .expect("nickname clear should succeed");
        assert!(cleared);

        let listed = store.list_agents_with_nicknames().await;
        assert!(
            listed
                .iter()
                .any(|(id, name)| id == &issued.agent_id && name.is_none())
        );

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
        let db_path = dir.join("state.sqlite3");

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

    #[tokio::test]
    async fn import_sled_state_copies_core_records() {
        let dir =
            std::env::temp_dir().join(format!("wakey-cp-store-test-{}", uuid::Uuid::new_v4()));
        let sled_path = dir.join("legacy-state.db");
        let sqlite_path = dir.join("state.sqlite3");
        fs::create_dir_all(&dir).expect("dir should exist");

        let legacy = sled::open(&sled_path).expect("legacy db should open");
        let meta = legacy.open_tree("meta").expect("meta should open");
        meta.insert(
            super::SCHEMA_VERSION_KEY.as_bytes(),
            &super::SCHEMA_VERSION.to_le_bytes(),
        )
        .expect("schema should insert");
        let enroll = legacy
            .open_tree("enroll_tokens")
            .expect("enroll tree should open");
        enroll
            .insert(b"enr-import-test", &(i64::MAX as u64).to_le_bytes())
            .expect("token should insert");
        let agents = legacy.open_tree("agents").expect("agents should open");
        agents
            .insert(b"agent-import", b"tok-import")
            .expect("agent should insert");
        let agent_meta = legacy.open_tree("agent_meta").expect("meta should open");
        agent_meta
            .insert(b"agent-import", b"imported-router")
            .expect("nickname should insert");
        legacy.flush().expect("legacy flush should succeed");
        drop(agent_meta);
        drop(agents);
        drop(enroll);
        drop(meta);
        drop(legacy);

        Store::import_sled_state(&sled_path, &sqlite_path, false)
            .await
            .expect("import should succeed");
        let store = Store::load_or_init(&sqlite_path, Vec::new(), Duration::from_secs(60))
            .await
            .expect("sqlite should load");

        assert!(store.verify_agent_token("agent-import", "tok-import").await);
        let agents = store.list_agents_with_nicknames().await;
        assert!(agents.iter().any(|(id, nickname)| {
            id == "agent-import" && nickname.as_deref() == Some("imported-router")
        }));
        let tokens = store
            .list_enroll_tokens()
            .await
            .expect("tokens should list");
        assert!(
            tokens
                .iter()
                .any(|token| token.enroll_token == "enr-import-test")
        );

        cleanup_dir(&dir);
    }
}
