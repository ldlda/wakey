use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use tracing::{info, warn};
use uuid::Uuid;

use crate::state::types::{
    AgentDeviceObservation, AgentDeviceObservationInput, AgentDeviceObservationView, AlertState,
    AlertTransition, AuditEvent, AuditEventFilter, AuditEventInput, DeviceIdentifier,
    DeviceIdentifierInput, EnrollTokenInfo, IssuedAgent, IssuedEnrollToken, KnownDevice,
    KnownDeviceInput, KnownDeviceSummary, StateStats,
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
        import_audit_events(&store.pool, &legacy).await?;
        import_active_alerts(&store.pool, &legacy).await?;
        import_alert_transitions(&store.pool, &legacy).await?;

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
        let expires_at_unix = sqlx::query_scalar!(
            "SELECT expires_at_unix FROM enroll_tokens WHERE token = ?1",
            enroll_token
        )
        .fetch_optional(&mut *tx)
        .await
        .context("failed reading enroll token")?;

        let Some(expires_at_unix) = expires_at_unix else {
            warn!("rejecting enroll attempt with invalid, expired, or consumed token");
            anyhow::bail!("invalid or already-used enroll token");
        };

        let now = now_unix();
        if expires_at_unix as u64 <= now {
            sqlx::query!("DELETE FROM enroll_tokens WHERE token = ?1", enroll_token)
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

        sqlx::query!("DELETE FROM enroll_tokens WHERE token = ?1", enroll_token)
            .execute(&mut *tx)
            .await
            .context("failed consuming enroll token")?;

        let agent_id = format!("agent-{}", Uuid::new_v4());
        let agent_token = format!("tok-{}", Uuid::new_v4());

        sqlx::query!(
            "INSERT INTO agents (agent_id, agent_token) VALUES (?1, ?2)",
            agent_id,
            agent_token
        )
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
        let expires_at_unix_i64 =
            i64::try_from(expires_at_unix).context("expiry does not fit SQLite integer")?;
        sqlx::query!(
            "INSERT INTO enroll_tokens (token, expires_at_unix) VALUES (?1, ?2)",
            token,
            expires_at_unix_i64
        )
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
        let rows = sqlx::query_as!(
            EnrollTokenRow,
            r#"SELECT token as "token!", expires_at_unix FROM enroll_tokens ORDER BY expires_at_unix, token"#,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed listing enroll tokens")?;

        rows.into_iter()
            .map(|row| {
                let expires_at_unix = u64::try_from(row.expires_at_unix)
                    .context("negative token expiry in state db")?;
                Ok(EnrollTokenInfo {
                    enroll_token: row.token,
                    expires_at_unix,
                    expired: expires_at_unix <= now,
                })
            })
            .collect()
    }

    pub async fn revoke_enroll_token(&self, token: &str) -> Result<bool> {
        let result = sqlx::query!("DELETE FROM enroll_tokens WHERE token = ?1", token)
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
        let result = sqlx::query!("DELETE FROM agents WHERE agent_id = ?1", agent_id)
            .execute(&mut *tx)
            .await
            .context("failed removing agent credentials")?;
        if result.rows_affected() > 0 {
            sqlx::query!("DELETE FROM agent_meta WHERE agent_id = ?1", agent_id)
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
        let exists = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM agents WHERE agent_id = ?1"#,
            agent_id
        )
        .fetch_one(&self.pool)
        .await
        .context("failed checking agent existence")?;
        if exists == 0 {
            return Ok(false);
        }

        let normalized = nickname.map(str::trim).filter(|v| !v.is_empty());
        if let Some(value) = normalized {
            sqlx::query!(
                "INSERT INTO agent_meta (agent_id, nickname) VALUES (?1, ?2)
                 ON CONFLICT(agent_id) DO UPDATE SET nickname = excluded.nickname",
                agent_id,
                value
            )
            .execute(&self.pool)
            .await
            .context("failed persisting agent nickname")?;
        } else {
            sqlx::query!("DELETE FROM agent_meta WHERE agent_id = ?1", agent_id)
                .execute(&self.pool)
                .await
                .context("failed clearing agent nickname")?;
        }

        Ok(true)
    }

    pub async fn stats(&self) -> Result<StateStats> {
        let now = i64::try_from(now_unix()).context("current time does not fit SQLite integer")?;
        let enroll_token_count =
            sqlx::query_scalar!(r#"SELECT COUNT(*) as "count!: i64" FROM enroll_tokens"#)
                .fetch_one(&self.pool)
                .await
                .context("failed counting enroll tokens")?;
        let expired_enroll_token_count = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM enroll_tokens WHERE expires_at_unix <= ?1"#,
            now
        )
        .fetch_one(&self.pool)
        .await
        .context("failed counting expired enroll tokens")?;
        let agent_count = sqlx::query_scalar!(r#"SELECT COUNT(*) as "count!: i64" FROM agents"#)
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
        sqlx::query!(r#"SELECT 1 as "ok!""#)
            .fetch_one(&self.pool)
            .await
            .context("failed validating SQLite state connection")?;
        info!(path = %self.db_path.display(), "reload requested; SQLite backend does not require in-memory reload");
        Ok(())
    }

    pub async fn verify_agent_token(&self, agent_id: &str, token: &str) -> bool {
        match sqlx::query_scalar!(
            "SELECT agent_token FROM agents WHERE agent_id = ?1",
            agent_id
        )
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
        match sqlx::query_scalar!(r#"SELECT agent_id as "agent_id!" FROM agents ORDER BY agent_id"#)
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
        let rows = sqlx::query!(
            r#"SELECT agents.agent_id as "agent_id!", agent_meta.nickname
             FROM agents
             LEFT JOIN agent_meta ON agent_meta.agent_id = agents.agent_id
             ORDER BY agents.agent_id"#,
        )
        .fetch_all(&self.pool)
        .await;

        match rows {
            Ok(rows) => rows
                .into_iter()
                .map(|row| {
                    let nickname = row
                        .nickname
                        .map(|v| v.trim().to_string())
                        .filter(|v| !v.is_empty());
                    (row.agent_id, nickname)
                })
                .collect(),
            Err(err) => {
                warn!(error = %err, "failed to list agents from state db");
                Vec::new()
            }
        }
    }

    pub async fn create_known_device(&self, input: KnownDeviceInput) -> Result<KnownDevice> {
        let display_name = normalize_required_text(&input.display_name, "display_name")?;
        let notes = normalize_optional_text(input.notes.as_deref());
        let identifiers = input
            .identifiers
            .into_iter()
            .map(normalize_device_identifier)
            .collect::<Result<Vec<_>>>()?;
        let device_id = format!("dev-{}", Uuid::new_v4());
        let now = now_unix();

        let mut tx = self
            .pool
            .begin()
            .await
            .context("failed starting known device transaction")?;
        let pinned = if input.pinned { 1_i64 } else { 0_i64 };
        let now_i64 = i64::try_from(now).context("known device timestamp overflow")?;
        sqlx::query!(
            "INSERT INTO known_devices
             (device_id, display_name, pinned, created_at_unix, updated_at_unix, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            device_id,
            display_name,
            pinned,
            now_i64,
            now_i64,
            notes
        )
        .execute(&mut *tx)
        .await
        .context("failed persisting known device")?;

        for identifier in &identifiers {
            insert_device_identifier_tx(&mut tx, &device_id, identifier, now).await?;
        }

        tx.commit()
            .await
            .context("failed committing known device transaction")?;
        self.get_known_device(&device_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("created known device disappeared"))
    }

    pub async fn list_known_devices(&self) -> Result<Vec<KnownDevice>> {
        let rows = sqlx::query_as!(
            KnownDeviceRow,
            r#"SELECT device_id as "device_id!", display_name as "display_name!",
                    pinned, created_at_unix, updated_at_unix, notes
             FROM known_devices
             ORDER BY pinned DESC, display_name, device_id"#,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed listing known devices")?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let device_id = row.device_id.clone();
            out.push(self.known_device_from_row(row, &device_id).await?);
        }
        Ok(out)
    }

    pub async fn get_known_device(&self, device_id: &str) -> Result<Option<KnownDevice>> {
        let row = sqlx::query_as!(
            KnownDeviceRow,
            r#"SELECT device_id as "device_id!", display_name as "display_name!",
                    pinned, created_at_unix, updated_at_unix, notes
             FROM known_devices
             WHERE device_id = ?1"#,
            device_id
        )
        .fetch_optional(&self.pool)
        .await
        .context("failed reading known device")?;

        match row {
            Some(row) => self.known_device_from_row(row, device_id).await.map(Some),
            None => Ok(None),
        }
    }

    pub async fn forget_known_device(&self, device_id: &str) -> Result<bool> {
        let result = sqlx::query!("DELETE FROM known_devices WHERE device_id = ?1", device_id)
            .execute(&self.pool)
            .await
            .context("failed deleting known device")?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn attach_device_identifier(
        &self,
        device_id: &str,
        input: DeviceIdentifierInput,
    ) -> Result<Option<KnownDevice>> {
        let identifier = normalize_device_identifier(input)?;
        let now = now_unix();
        let mut tx = self
            .pool
            .begin()
            .await
            .context("failed starting device identifier transaction")?;
        let exists = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM known_devices WHERE device_id = ?1"#,
            device_id
        )
        .fetch_one(&mut *tx)
        .await
        .context("failed checking known device existence")?;
        if exists == 0 {
            return Ok(None);
        }

        insert_device_identifier_tx(&mut tx, device_id, &identifier, now).await?;
        let now_i64 = i64::try_from(now).context("known device timestamp overflow")?;
        sqlx::query!(
            "UPDATE known_devices SET updated_at_unix = ?1 WHERE device_id = ?2",
            now_i64,
            device_id
        )
        .execute(&mut *tx)
        .await
        .context("failed updating known device timestamp")?;
        tx.commit()
            .await
            .context("failed committing device identifier transaction")?;
        self.get_known_device(device_id).await
    }

    pub async fn attach_observation_identifier(
        &self,
        device_id: &str,
        observation_key: &str,
    ) -> Result<Option<KnownDevice>> {
        let observation = sqlx::query_as!(
            ObservationIdentifierRow,
            r#"SELECT mac, ip
             FROM agent_device_observations
             WHERE observation_key = ?1"#,
            observation_key
        )
        .fetch_optional(&self.pool)
        .await
        .context("failed reading observation identifier")?;

        let Some(observation) = observation else {
            anyhow::bail!("observation not found");
        };
        let input = observation
            .mac
            .map(|value| DeviceIdentifierInput {
                kind: "mac".into(),
                value,
            })
            .or_else(|| {
                observation.ip.map(|value| DeviceIdentifierInput {
                    kind: "ip".into(),
                    value,
                })
            })
            .ok_or_else(|| anyhow::anyhow!("observation has no attachable mac or ip"))?;

        self.attach_device_identifier(device_id, input).await
    }

    #[allow(unused)] // we'll get to this
    pub async fn lookup_known_device_by_identifier(
        &self,
        input: DeviceIdentifierInput,
    ) -> Result<Option<KnownDevice>> {
        let identifier = normalize_device_identifier(input)?;
        let device_id = sqlx::query_scalar!(
            "SELECT device_id FROM device_identifiers WHERE identifier_key = ?1",
            identifier.identifier_key
        )
        .fetch_optional(&self.pool)
        .await
        .context("failed looking up known device identifier")?;
        match device_id {
            Some(device_id) => self.get_known_device(&device_id).await,
            None => Ok(None),
        }
    }

    pub async fn upsert_agent_observations(
        &self,
        agent_id: &str,
        observations: Vec<AgentDeviceObservationInput>,
    ) -> Result<usize> {
        let mut tx = self
            .pool
            .begin()
            .await
            .context("failed starting observation transaction")?;
        let mut written = 0usize;
        for observation in observations {
            let observation = normalize_agent_observation(agent_id, observation)?;
            let first_seen_unix = i64::try_from(observation.first_seen_unix)
                .context("observation first_seen overflow")?;
            let last_seen_unix = i64::try_from(observation.last_seen_unix)
                .context("observation last_seen overflow")?;
            sqlx::query!(
                "INSERT INTO agent_device_observations
                 (observation_key, agent_id, kind, mac, ip, hostname,
                  first_seen_unix, last_seen_unix, last_action)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(observation_key) DO UPDATE SET
                   mac = excluded.mac,
                   ip = excluded.ip,
                   hostname = excluded.hostname,
                   first_seen_unix = MIN(agent_device_observations.first_seen_unix, excluded.first_seen_unix),
                   last_seen_unix = MAX(agent_device_observations.last_seen_unix, excluded.last_seen_unix),
                   last_action = excluded.last_action",
                observation.observation_key,
                observation.agent_id,
                observation.kind,
                observation.mac,
                observation.ip,
                observation.hostname,
                first_seen_unix,
                last_seen_unix,
                observation.last_action
            )
            .execute(&mut *tx)
            .await
            .context("failed upserting agent device observation")?;

            let event_id = format!("ode-{}", Uuid::new_v4());
            sqlx::query!(
                "INSERT INTO agent_device_observation_events
                 (event_id, agent_id, kind, action, mac, ip, hostname, ts_unix)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                event_id,
                observation.agent_id,
                observation.kind,
                observation.last_action,
                observation.mac,
                observation.ip,
                observation.hostname,
                last_seen_unix
            )
            .execute(&mut *tx)
            .await
            .context("failed appending agent device observation event")?;
            written = written.saturating_add(1);
        }
        tx.commit()
            .await
            .context("failed committing observation transaction")?;
        Ok(written)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub async fn list_agent_observations(
        &self,
        agent_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<AgentDeviceObservation>> {
        let limit = limit.clamp(1, 1000);
        let limit = i64::try_from(limit).context("observation limit overflow")?;
        let rows = if let Some(agent_id) = agent_id {
            sqlx::query_as!(
                AgentObservationRow,
                r#"SELECT observation_key as "observation_key!", agent_id as "agent_id!",
                        kind as "kind!", mac, ip, hostname,
                        first_seen_unix, last_seen_unix, last_action as "last_action!"
                 FROM agent_device_observations
                 WHERE agent_id = ?1
                 ORDER BY last_seen_unix DESC
                 LIMIT ?2"#,
                agent_id,
                limit
            )
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query_as!(
                AgentObservationRow,
                r#"SELECT observation_key as "observation_key!", agent_id as "agent_id!",
                        kind as "kind!", mac, ip, hostname,
                        first_seen_unix, last_seen_unix, last_action as "last_action!"
                 FROM agent_device_observations
                 ORDER BY last_seen_unix DESC
                 LIMIT ?1"#,
                limit
            )
            .fetch_all(&self.pool)
            .await
        }
        .context("failed listing agent observations")?;
        rows.into_iter().map(agent_observation_from_row).collect()
    }

    pub async fn list_agent_observation_views(
        &self,
        agent_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<AgentDeviceObservationView>> {
        let limit = limit.clamp(1, 1000);
        let limit = i64::try_from(limit).context("observation limit overflow")?;
        let rows = if let Some(agent_id) = agent_id {
            sqlx::query_as!(
                AgentObservationViewRow,
                r#"SELECT observations.observation_key as "observation_key!",
                        observations.agent_id as "agent_id!",
                        observations.kind as "kind!",
                        observations.mac,
                        observations.ip,
                        observations.hostname,
                        observations.first_seen_unix,
                        observations.last_seen_unix,
                        observations.last_action as "last_action!",
                        known_devices.device_id,
                        known_devices.display_name,
                        known_devices.pinned
                 FROM agent_device_observations observations
                 LEFT JOIN device_identifiers identifiers
                   ON identifiers.identifier_key =
                      CASE
                        WHEN observations.mac IS NOT NULL THEN 'mac:' || observations.mac
                        WHEN observations.ip IS NOT NULL THEN 'ip:' || observations.ip
                      END
                 LEFT JOIN known_devices ON known_devices.device_id = identifiers.device_id
                 WHERE observations.agent_id = ?1
                 ORDER BY observations.last_seen_unix DESC
                 LIMIT ?2"#,
                agent_id,
                limit
            )
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query_as!(
                AgentObservationViewRow,
                r#"SELECT observations.observation_key as "observation_key!",
                        observations.agent_id as "agent_id!",
                        observations.kind as "kind!",
                        observations.mac,
                        observations.ip,
                        observations.hostname,
                        observations.first_seen_unix,
                        observations.last_seen_unix,
                        observations.last_action as "last_action!",
                        known_devices.device_id,
                        known_devices.display_name,
                        known_devices.pinned
                 FROM agent_device_observations observations
                 LEFT JOIN device_identifiers identifiers
                   ON identifiers.identifier_key =
                      CASE
                        WHEN observations.mac IS NOT NULL THEN 'mac:' || observations.mac
                        WHEN observations.ip IS NOT NULL THEN 'ip:' || observations.ip
                      END
                 LEFT JOIN known_devices ON known_devices.device_id = identifiers.device_id
                 ORDER BY observations.last_seen_unix DESC
                 LIMIT ?1"#,
                limit
            )
            .fetch_all(&self.pool)
            .await
        }
        .context("failed listing agent observation views")?;
        rows.into_iter()
            .map(agent_observation_view_from_row)
            .collect()
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
        let metadata_json =
            serde_json::to_string(&event.metadata).context("failed to encode audit metadata")?;
        let ts_unix = i64::try_from(event.ts_unix).context("audit timestamp overflow")?;
        let latency_ms = event
            .latency_ms
            .map(i64::try_from)
            .transpose()
            .context("audit latency overflow")?;
        sqlx::query!(
            "INSERT INTO audit_events
             (event_key, event_id, ts_unix, actor_type, actor_id, agent_id, request_id,
              event_type, outcome, latency_ms, message, metadata_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            key,
            event.event_id,
            ts_unix,
            event.actor_type,
            event.actor_id,
            event.agent_id,
            event.request_id,
            event.event_type,
            event.outcome,
            latency_ms,
            event.message,
            metadata_json
        )
        .execute(&self.pool)
        .await
        .context("failed persisting audit event")?;
        Ok(event)
    }

    pub async fn list_audit_events(&self, filter: AuditEventFilter) -> Result<Vec<AuditEvent>> {
        let limit = filter.limit.clamp(1, 500);
        let mut builder = sqlx::QueryBuilder::new(
            "SELECT event_id, ts_unix, actor_type, actor_id, agent_id, request_id,
                    event_type, outcome, latency_ms, message, metadata_json
             FROM audit_events
             WHERE 1 = 1",
        );

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

        rows.into_iter().map(audit_event_from_row).collect()
    }

    pub async fn sync_alert_transitions(
        &self,
        current: &[AlertState],
    ) -> Result<Vec<AlertTransition>> {
        let mut previous = std::collections::HashMap::<String, AlertState>::new();
        let rows = sqlx::query_as!(
            AlertStateRow,
            r#"SELECT alert_id as "alert_id!", kind as "kind!", severity as "severity!",
                    status as "status!", agent_id, message as "message!",
                    value, threshold, last_seen_unix, metadata_json as "metadata_json!"
             FROM active_alerts"#,
        )
        .fetch_all(&self.pool)
        .await
        .context("failed iterating active_alerts table")?;
        for row in rows {
            let alert = alert_state_from_row(row).context("failed decoding active alert")?;
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
        sqlx::query!("DELETE FROM active_alerts")
            .execute(&mut *tx)
            .await
            .context("failed clearing active alert snapshot")?;

        for alert in current {
            insert_active_alert(&mut tx, alert).await?;
        }

        for transition in &transitions {
            let key = format!("{:020}:{}", transition.ts_unix, transition.transition_id);
            insert_alert_transition(&mut tx, &key, transition).await?;
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
        let limit = i64::try_from(limit).context("alert limit overflow")?;
        let rows = if let Some(since) = since_unix {
            let since = i64::try_from(since).context("since_unix overflow")?;
            sqlx::query_as!(
                AlertTransitionRow,
                r#"SELECT transition_id as "transition_id!", ts_unix, alert_id as "alert_id!",
                        kind as "kind!", agent_id, from_status, to_status as "to_status!",
                        message as "message!", metadata_json as "metadata_json!"
                 FROM alert_transitions
                 WHERE ts_unix >= ?1
                 ORDER BY transition_key DESC
                 LIMIT ?2"#,
                since,
                limit
            )
            .fetch_all(&self.pool)
            .await
        } else {
            sqlx::query_as!(
                AlertTransitionRow,
                r#"SELECT transition_id as "transition_id!", ts_unix, alert_id as "alert_id!",
                        kind as "kind!", agent_id, from_status, to_status as "to_status!",
                        message as "message!", metadata_json as "metadata_json!"
                 FROM alert_transitions
                 ORDER BY transition_key DESC
                 LIMIT ?1"#,
                limit
            )
            .fetch_all(&self.pool)
            .await
        }
        .context("failed listing alert transitions")?;

        rows.into_iter().map(alert_transition_from_row).collect()
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
            let marker_exists = sqlx::query_scalar!(
                r#"SELECT COUNT(*) as "count!: i64" FROM meta WHERE key = ?1"#,
                marker_key
            )
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
            let expires_at_i64 = i64::try_from(expires_at).context("token expiry overflow")?;
            sqlx::query!(
                "INSERT OR REPLACE INTO enroll_tokens (token, expires_at_unix) VALUES (?1, ?2)",
                token,
                expires_at_i64
            )
            .execute(&mut *tx)
            .await
            .context("failed seeding enroll token")?;
            let marker_value = expires_at.to_le_bytes().to_vec();
            sqlx::query!(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)",
                marker_key,
                marker_value
            )
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
        let result = sqlx::query!("DELETE FROM enroll_tokens WHERE expires_at_unix <= ?1", now)
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
        match sqlx::query_scalar!("SELECT value FROM meta WHERE key = ?1", SCHEMA_VERSION_KEY)
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
                let schema_version = SCHEMA_VERSION.to_le_bytes().to_vec();
                sqlx::query!(
                    "INSERT INTO meta (key, value) VALUES (?1, ?2)",
                    SCHEMA_VERSION_KEY,
                    schema_version
                )
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
        let raw = sqlx::query_scalar!("SELECT value FROM meta WHERE key = ?1", SCHEMA_VERSION_KEY)
            .fetch_one(&self.pool)
            .await
            .context("failed reading schema version")?;
        decode_schema(raw.as_ref()).context("failed decoding schema version")
    }

    async fn known_device_from_row(
        &self,
        row: KnownDeviceRow,
        device_id: &str,
    ) -> Result<KnownDevice> {
        let identifiers = self.list_device_identifiers(device_id).await?;
        Ok(KnownDevice {
            device_id: row.device_id,
            display_name: row.display_name,
            pinned: row.pinned != 0,
            created_at_unix: u64::try_from(row.created_at_unix)
                .context("negative known device created timestamp in state db")?,
            updated_at_unix: u64::try_from(row.updated_at_unix)
                .context("negative known device updated timestamp in state db")?,
            notes: row.notes,
            identifiers,
        })
    }

    async fn list_device_identifiers(&self, device_id: &str) -> Result<Vec<DeviceIdentifier>> {
        let rows = sqlx::query_as!(
            DeviceIdentifierRow,
            r#"SELECT identifier_key as "identifier_key!", device_id as "device_id!",
                    kind as "kind!", value as "value!", created_at_unix
             FROM device_identifiers
             WHERE device_id = ?1
             ORDER BY kind, value"#,
            device_id
        )
        .fetch_all(&self.pool)
        .await
        .context("failed listing device identifiers")?;
        rows.into_iter().map(device_identifier_from_row).collect()
    }
}

#[derive(Debug, Clone)]
struct NormalizedDeviceIdentifier {
    identifier_key: String,
    kind: String,
    value: String,
}

struct EnrollTokenRow {
    token: String,
    expires_at_unix: i64,
}

struct KnownDeviceRow {
    device_id: String,
    display_name: String,
    pinned: i64,
    created_at_unix: i64,
    updated_at_unix: i64,
    notes: Option<String>,
}

struct DeviceIdentifierRow {
    identifier_key: String,
    device_id: String,
    kind: String,
    value: String,
    created_at_unix: i64,
}

#[cfg_attr(not(test), allow(dead_code))]
struct AgentObservationRow {
    observation_key: String,
    agent_id: String,
    kind: String,
    mac: Option<String>,
    ip: Option<String>,
    hostname: Option<String>,
    first_seen_unix: i64,
    last_seen_unix: i64,
    last_action: String,
}

struct AgentObservationViewRow {
    observation_key: String,
    agent_id: String,
    kind: String,
    mac: Option<String>,
    ip: Option<String>,
    hostname: Option<String>,
    first_seen_unix: i64,
    last_seen_unix: i64,
    last_action: String,
    device_id: Option<String>,
    display_name: Option<String>,
    pinned: Option<i64>,
}

struct ObservationIdentifierRow {
    mac: Option<String>,
    ip: Option<String>,
}

struct AlertStateRow {
    alert_id: String,
    kind: String,
    severity: String,
    status: String,
    agent_id: Option<String>,
    message: String,
    value: i64,
    threshold: i64,
    last_seen_unix: i64,
    metadata_json: String,
}

struct AlertTransitionRow {
    transition_id: String,
    ts_unix: i64,
    alert_id: String,
    kind: String,
    agent_id: Option<String>,
    from_status: Option<String>,
    to_status: String,
    message: String,
    metadata_json: String,
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

fn normalize_required_text(value: &str, field: &str) -> Result<String> {
    let normalized = value.trim();
    if normalized.is_empty() {
        anyhow::bail!("{field} must not be empty");
    }
    Ok(normalized.to_string())
}

fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn normalize_device_identifier(input: DeviceIdentifierInput) -> Result<NormalizedDeviceIdentifier> {
    let kind = normalize_required_text(&input.kind, "identifier kind")?.to_ascii_lowercase();
    let value = normalize_required_text(&input.value, "identifier value")?.to_ascii_lowercase();
    let identifier_key = format!("{kind}:{value}");
    Ok(NormalizedDeviceIdentifier {
        identifier_key,
        kind,
        value,
    })
}

fn normalize_agent_observation(
    agent_id: &str,
    input: AgentDeviceObservationInput,
) -> Result<AgentDeviceObservation> {
    let kind = normalize_required_text(&input.kind, "observation kind")?.to_ascii_lowercase();
    let action = normalize_required_text(&input.action, "observation action")?.to_ascii_lowercase();
    let mac = normalize_optional_text(input.mac.as_deref()).map(|value| value.to_ascii_lowercase());
    let ip = normalize_optional_text(input.ip.as_deref());
    let hostname = normalize_optional_text(input.hostname.as_deref());
    let identifier = mac
        .as_ref()
        .map(|value| format!("mac:{value}"))
        .or_else(|| ip.as_ref().map(|value| format!("ip:{value}")))
        .ok_or_else(|| anyhow::anyhow!("observation requires mac or ip"))?;
    let observation_key = format!("agent:{agent_id}:{kind}:{identifier}");
    Ok(AgentDeviceObservation {
        observation_key,
        agent_id: agent_id.to_string(),
        kind,
        mac,
        ip,
        hostname,
        first_seen_unix: input.first_seen_unix,
        last_seen_unix: input.last_seen_unix,
        last_action: action,
    })
}

async fn insert_device_identifier_tx(
    tx: &mut Transaction<'_, Sqlite>,
    device_id: &str,
    identifier: &NormalizedDeviceIdentifier,
    created_at_unix: u64,
) -> Result<()> {
    let created_at_unix =
        i64::try_from(created_at_unix).context("device identifier timestamp overflow")?;
    sqlx::query!(
        "INSERT INTO device_identifiers
         (identifier_key, device_id, kind, value, created_at_unix)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        identifier.identifier_key,
        device_id,
        identifier.kind,
        identifier.value,
        created_at_unix
    )
    .execute(&mut **tx)
    .await
    .with_context(|| {
        format!(
            "failed attaching device identifier {} to {}",
            identifier.identifier_key, device_id
        )
    })?;
    Ok(())
}

fn device_identifier_from_row(row: DeviceIdentifierRow) -> Result<DeviceIdentifier> {
    Ok(DeviceIdentifier {
        identifier_key: row.identifier_key,
        device_id: row.device_id,
        kind: row.kind,
        value: row.value,
        created_at_unix: u64::try_from(row.created_at_unix)
            .context("negative device identifier timestamp in state db")?,
    })
}

#[cfg_attr(not(test), allow(dead_code))]
fn agent_observation_from_row(row: AgentObservationRow) -> Result<AgentDeviceObservation> {
    Ok(AgentDeviceObservation {
        observation_key: row.observation_key,
        agent_id: row.agent_id,
        kind: row.kind,
        mac: row.mac,
        ip: row.ip,
        hostname: row.hostname,
        first_seen_unix: u64::try_from(row.first_seen_unix)
            .context("negative observation first_seen timestamp in state db")?,
        last_seen_unix: u64::try_from(row.last_seen_unix)
            .context("negative observation last_seen timestamp in state db")?,
        last_action: row.last_action,
    })
}

fn agent_observation_view_from_row(
    row: AgentObservationViewRow,
) -> Result<AgentDeviceObservationView> {
    let known_device = match (row.device_id, row.display_name, row.pinned) {
        (Some(device_id), Some(display_name), Some(pinned)) => Some(KnownDeviceSummary {
            device_id,
            display_name,
            pinned: pinned != 0,
        }),
        _ => None,
    };
    Ok(AgentDeviceObservationView {
        observation_key: row.observation_key,
        agent_id: row.agent_id,
        kind: row.kind,
        mac: row.mac,
        ip: row.ip,
        hostname: row.hostname,
        first_seen_unix: u64::try_from(row.first_seen_unix)
            .context("negative observation first_seen timestamp in state db")?,
        last_seen_unix: u64::try_from(row.last_seen_unix)
            .context("negative observation last_seen timestamp in state db")?,
        last_action: row.last_action,
        known_device,
    })
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

async fn import_audit_events(pool: &SqlitePool, legacy: &sled::Db) -> Result<()> {
    let tree = legacy
        .open_tree("audit_events")
        .context("failed to open legacy audit_events tree")?;
    for item in tree.iter() {
        let (key, value) = item.context("failed reading legacy audit event")?;
        let key = String::from_utf8(key.to_vec()).context("legacy audit key is not utf-8")?;
        let event: AuditEvent =
            serde_json::from_slice(value.as_ref()).context("failed decoding legacy audit event")?;
        insert_audit_event(pool, &key, &event)
            .await
            .context("failed importing legacy audit event")?;
    }
    Ok(())
}

async fn import_active_alerts(pool: &SqlitePool, legacy: &sled::Db) -> Result<()> {
    let tree = legacy
        .open_tree("active_alerts")
        .context("failed to open legacy active_alerts tree")?;
    for item in tree.iter() {
        let (_, value) = item.context("failed reading legacy active alert")?;
        let alert: AlertState = serde_json::from_slice(value.as_ref())
            .context("failed decoding legacy active alert")?;
        insert_active_alert_pool(pool, &alert)
            .await
            .context("failed importing legacy active alert")?;
    }
    Ok(())
}

async fn import_alert_transitions(pool: &SqlitePool, legacy: &sled::Db) -> Result<()> {
    let tree = legacy
        .open_tree("alert_transitions")
        .context("failed to open legacy alert_transitions tree")?;
    for item in tree.iter() {
        let (key, value) = item.context("failed reading legacy alert transition")?;
        let key =
            String::from_utf8(key.to_vec()).context("legacy alert transition key is not utf-8")?;
        let transition: AlertTransition = serde_json::from_slice(value.as_ref())
            .context("failed decoding legacy alert transition")?;
        insert_alert_transition_pool(pool, &key, &transition)
            .await
            .context("failed importing legacy alert transition")?;
    }
    Ok(())
}

async fn insert_audit_event(pool: &SqlitePool, key: &str, event: &AuditEvent) -> Result<()> {
    let metadata_json =
        serde_json::to_string(&event.metadata).context("failed to encode audit metadata")?;
    sqlx::query(
        "INSERT OR REPLACE INTO audit_events
         (event_key, event_id, ts_unix, actor_type, actor_id, agent_id, request_id,
          event_type, outcome, latency_ms, message, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )
    .bind(key)
    .bind(&event.event_id)
    .bind(i64::try_from(event.ts_unix).context("audit timestamp overflow")?)
    .bind(&event.actor_type)
    .bind(&event.actor_id)
    .bind(&event.agent_id)
    .bind(&event.request_id)
    .bind(&event.event_type)
    .bind(&event.outcome)
    .bind(
        event
            .latency_ms
            .map(i64::try_from)
            .transpose()
            .context("audit latency overflow")?,
    )
    .bind(&event.message)
    .bind(metadata_json)
    .execute(pool)
    .await
    .context("failed persisting audit event")?;
    Ok(())
}

async fn insert_active_alert(tx: &mut Transaction<'_, Sqlite>, alert: &AlertState) -> Result<()> {
    let metadata_json =
        serde_json::to_string(&alert.metadata).context("failed to encode active alert metadata")?;
    sqlx::query(
        "INSERT INTO active_alerts
         (alert_id, kind, severity, status, agent_id, message, value, threshold,
          last_seen_unix, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )
    .bind(&alert.alert_id)
    .bind(&alert.kind)
    .bind(&alert.severity)
    .bind(&alert.status)
    .bind(&alert.agent_id)
    .bind(&alert.message)
    .bind(i64::try_from(alert.value).context("active alert value overflow")?)
    .bind(i64::try_from(alert.threshold).context("active alert threshold overflow")?)
    .bind(i64::try_from(alert.last_seen_unix).context("active alert timestamp overflow")?)
    .bind(metadata_json)
    .execute(&mut **tx)
    .await
    .context("failed writing active alert snapshot")?;
    Ok(())
}

async fn insert_active_alert_pool(pool: &SqlitePool, alert: &AlertState) -> Result<()> {
    let metadata_json =
        serde_json::to_string(&alert.metadata).context("failed to encode active alert metadata")?;
    sqlx::query(
        "INSERT OR REPLACE INTO active_alerts
         (alert_id, kind, severity, status, agent_id, message, value, threshold,
          last_seen_unix, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )
    .bind(&alert.alert_id)
    .bind(&alert.kind)
    .bind(&alert.severity)
    .bind(&alert.status)
    .bind(&alert.agent_id)
    .bind(&alert.message)
    .bind(i64::try_from(alert.value).context("active alert value overflow")?)
    .bind(i64::try_from(alert.threshold).context("active alert threshold overflow")?)
    .bind(i64::try_from(alert.last_seen_unix).context("active alert timestamp overflow")?)
    .bind(metadata_json)
    .execute(pool)
    .await
    .context("failed writing active alert snapshot")?;
    Ok(())
}

async fn insert_alert_transition(
    tx: &mut Transaction<'_, Sqlite>,
    key: &str,
    transition: &AlertTransition,
) -> Result<()> {
    let metadata_json = serde_json::to_string(&transition.metadata)
        .context("failed to encode alert transition metadata")?;
    sqlx::query(
        "INSERT INTO alert_transitions
         (transition_key, transition_id, ts_unix, alert_id, kind, agent_id,
          from_status, to_status, message, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )
    .bind(key)
    .bind(&transition.transition_id)
    .bind(i64::try_from(transition.ts_unix).context("alert timestamp overflow")?)
    .bind(&transition.alert_id)
    .bind(&transition.kind)
    .bind(&transition.agent_id)
    .bind(&transition.from_status)
    .bind(&transition.to_status)
    .bind(&transition.message)
    .bind(metadata_json)
    .execute(&mut **tx)
    .await
    .context("failed persisting alert transition")?;
    Ok(())
}

async fn insert_alert_transition_pool(
    pool: &SqlitePool,
    key: &str,
    transition: &AlertTransition,
) -> Result<()> {
    let metadata_json = serde_json::to_string(&transition.metadata)
        .context("failed to encode alert transition metadata")?;
    sqlx::query(
        "INSERT OR REPLACE INTO alert_transitions
         (transition_key, transition_id, ts_unix, alert_id, kind, agent_id,
          from_status, to_status, message, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )
    .bind(key)
    .bind(&transition.transition_id)
    .bind(i64::try_from(transition.ts_unix).context("alert timestamp overflow")?)
    .bind(&transition.alert_id)
    .bind(&transition.kind)
    .bind(&transition.agent_id)
    .bind(&transition.from_status)
    .bind(&transition.to_status)
    .bind(&transition.message)
    .bind(metadata_json)
    .execute(pool)
    .await
    .context("failed persisting alert transition")?;
    Ok(())
}

fn audit_event_from_row(row: sqlx::sqlite::SqliteRow) -> Result<AuditEvent> {
    let ts_unix: i64 = row.try_get("ts_unix")?;
    let latency_ms: Option<i64> = row.try_get("latency_ms")?;
    let metadata_json: String = row.try_get("metadata_json")?;
    Ok(AuditEvent {
        event_id: row.try_get("event_id")?,
        ts_unix: u64::try_from(ts_unix).context("negative audit timestamp in state db")?,
        actor_type: row.try_get("actor_type")?,
        actor_id: row.try_get("actor_id")?,
        agent_id: row.try_get("agent_id")?,
        request_id: row.try_get("request_id")?,
        event_type: row.try_get("event_type")?,
        outcome: row.try_get("outcome")?,
        latency_ms: latency_ms
            .map(u64::try_from)
            .transpose()
            .context("negative audit latency in state db")?,
        message: row.try_get("message")?,
        metadata: serde_json::from_str(&metadata_json).context("failed decoding audit metadata")?,
    })
}

fn alert_state_from_row(row: AlertStateRow) -> Result<AlertState> {
    Ok(AlertState {
        alert_id: row.alert_id,
        kind: row.kind,
        severity: row.severity,
        status: row.status,
        agent_id: row.agent_id,
        message: row.message,
        value: u64::try_from(row.value).context("negative active alert value in state db")?,
        threshold: u64::try_from(row.threshold)
            .context("negative active alert threshold in state db")?,
        last_seen_unix: u64::try_from(row.last_seen_unix)
            .context("negative active alert timestamp in state db")?,
        metadata: serde_json::from_str(&row.metadata_json)
            .context("failed decoding active alert metadata")?,
    })
}

fn alert_transition_from_row(row: AlertTransitionRow) -> Result<AlertTransition> {
    Ok(AlertTransition {
        transition_id: row.transition_id,
        ts_unix: u64::try_from(row.ts_unix).context("negative alert timestamp in state db")?,
        alert_id: row.alert_id,
        kind: row.kind,
        agent_id: row.agent_id,
        from_status: row.from_status,
        to_status: row.to_status,
        message: row.message,
        metadata: serde_json::from_str(&row.metadata_json)
            .context("failed decoding alert transition metadata")?,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use crate::state::{DeviceIdentifierInput, KnownDeviceInput};

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
    async fn known_device_can_hold_multiple_manual_mac_identifiers() {
        let (store, dir) = make_store().await;

        let created = store
            .create_known_device(KnownDeviceInput {
                display_name: "lda".into(),
                pinned: true,
                notes: Some("windows pc".into()),
                identifiers: vec![DeviceIdentifierInput {
                    kind: "mac".into(),
                    value: "AA:BB:CC:DD:EE:01".into(),
                }],
            })
            .await
            .expect("known device should create");

        assert_eq!(created.display_name, "lda");
        assert!(created.pinned);
        assert_eq!(created.identifiers.len(), 1);
        assert_eq!(created.identifiers[0].value, "aa:bb:cc:dd:ee:01");

        let updated = store
            .attach_device_identifier(
                &created.device_id,
                DeviceIdentifierInput {
                    kind: "MAC".into(),
                    value: "AA:BB:CC:DD:EE:02".into(),
                },
            )
            .await
            .expect("identifier attach should succeed")
            .expect("device should exist");

        assert_eq!(updated.identifiers.len(), 2);
        assert!(
            updated
                .identifiers
                .iter()
                .any(|identifier| identifier.value == "aa:bb:cc:dd:ee:02")
        );

        let matched = store
            .lookup_known_device_by_identifier(DeviceIdentifierInput {
                kind: "mac".into(),
                value: "aa:bb:cc:dd:ee:02".into(),
            })
            .await
            .expect("lookup should succeed")
            .expect("identifier should match");
        assert_eq!(matched.device_id, created.device_id);

        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn known_device_identifier_is_unique_across_devices() {
        let (store, dir) = make_store().await;
        let first = store
            .create_known_device(KnownDeviceInput {
                display_name: "lda".into(),
                pinned: true,
                notes: None,
                identifiers: vec![DeviceIdentifierInput {
                    kind: "mac".into(),
                    value: "aa:bb:cc:dd:ee:ff".into(),
                }],
            })
            .await
            .expect("first device should create");
        let second = store
            .create_known_device(KnownDeviceInput {
                display_name: "other".into(),
                pinned: false,
                notes: None,
                identifiers: Vec::new(),
            })
            .await
            .expect("second device should create");

        let err = store
            .attach_device_identifier(
                &second.device_id,
                DeviceIdentifierInput {
                    kind: "mac".into(),
                    value: "aa:bb:cc:dd:ee:ff".into(),
                },
            )
            .await
            .expect_err("duplicate identifier should be rejected");
        assert!(
            err.to_string()
                .contains("failed attaching device identifier")
        );

        let listed = store.list_known_devices().await.expect("list should work");
        assert_eq!(listed.len(), 2);
        assert!(
            listed
                .iter()
                .find(|device| device.device_id == first.device_id)
                .expect("first should remain")
                .identifiers
                .len()
                == 1
        );

        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn agent_observations_upsert_current_state_and_events() {
        let (store, dir) = make_store().await;

        let accepted = store
            .upsert_agent_observations(
                "agent-a",
                vec![crate::state::AgentDeviceObservationInput {
                    kind: "dhcp".into(),
                    action: "update".into(),
                    mac: Some("AA:BB:CC:DD:EE:FF".into()),
                    ip: Some("192.168.1.10".into()),
                    hostname: Some("lda".into()),
                    first_seen_unix: 10,
                    last_seen_unix: 20,
                }],
            )
            .await
            .expect("observation upsert should succeed");
        assert_eq!(accepted, 1);

        let rows = store
            .list_agent_observations(Some("agent-a"), 10)
            .await
            .expect("observations should list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].mac.as_deref(), Some("aa:bb:cc:dd:ee:ff"));
        assert_eq!(rows[0].hostname.as_deref(), Some("lda"));

        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn agent_observation_views_include_matching_known_device() {
        let (store, dir) = make_store().await;

        let device = store
            .create_known_device(KnownDeviceInput {
                display_name: "lda".into(),
                pinned: true,
                notes: None,
                identifiers: vec![DeviceIdentifierInput {
                    kind: "mac".into(),
                    value: "aa:bb:cc:dd:ee:ff".into(),
                }],
            })
            .await
            .expect("known device should create");

        store
            .upsert_agent_observations(
                "agent-a",
                vec![
                    crate::state::AgentDeviceObservationInput {
                        kind: "dhcp".into(),
                        action: "update".into(),
                        mac: Some("AA:BB:CC:DD:EE:FF".into()),
                        ip: Some("192.168.1.10".into()),
                        hostname: Some("lda".into()),
                        first_seen_unix: 10,
                        last_seen_unix: 20,
                    },
                    crate::state::AgentDeviceObservationInput {
                        kind: "dhcp".into(),
                        action: "update".into(),
                        mac: Some("00:11:22:33:44:55".into()),
                        ip: Some("192.168.1.11".into()),
                        hostname: Some("guest".into()),
                        first_seen_unix: 11,
                        last_seen_unix: 21,
                    },
                ],
            )
            .await
            .expect("observation upsert should succeed");

        let rows = store
            .list_agent_observation_views(Some("agent-a"), 10)
            .await
            .expect("observation views should list");
        assert_eq!(rows.len(), 2);

        let known = rows
            .iter()
            .find(|row| row.mac.as_deref() == Some("aa:bb:cc:dd:ee:ff"))
            .expect("known observation should be present");
        let known_device = known
            .known_device
            .as_ref()
            .expect("known observation should join device");
        assert_eq!(known_device.device_id, device.device_id);
        assert_eq!(known_device.display_name, "lda");
        assert!(known_device.pinned);

        let unknown = rows
            .iter()
            .find(|row| row.mac.as_deref() == Some("00:11:22:33:44:55"))
            .expect("unknown observation should be present");
        assert!(unknown.known_device.is_none());

        cleanup_dir(&dir);
    }

    #[tokio::test]
    async fn observation_identifier_can_be_attached_to_known_device() {
        let (store, dir) = make_store().await;

        let device = store
            .create_known_device(KnownDeviceInput {
                display_name: "lda".into(),
                pinned: true,
                notes: None,
                identifiers: Vec::new(),
            })
            .await
            .expect("known device should create");

        store
            .upsert_agent_observations(
                "agent-a",
                vec![crate::state::AgentDeviceObservationInput {
                    kind: "dhcp".into(),
                    action: "update".into(),
                    mac: Some("AA:BB:CC:DD:EE:FF".into()),
                    ip: Some("192.168.1.10".into()),
                    hostname: Some("lda".into()),
                    first_seen_unix: 10,
                    last_seen_unix: 20,
                }],
            )
            .await
            .expect("observation upsert should succeed");

        let observation = store
            .list_agent_observations(Some("agent-a"), 10)
            .await
            .expect("observations should list")
            .pop()
            .expect("observation should exist");

        let updated = store
            .attach_observation_identifier(&device.device_id, &observation.observation_key)
            .await
            .expect("observation identifier should attach")
            .expect("device should exist");

        assert_eq!(updated.identifiers.len(), 1);
        assert_eq!(updated.identifiers[0].kind, "mac");
        assert_eq!(updated.identifiers[0].value, "aa:bb:cc:dd:ee:ff");

        let views = store
            .list_agent_observation_views(Some("agent-a"), 10)
            .await
            .expect("observation views should list");
        assert_eq!(
            views[0]
                .known_device
                .as_ref()
                .map(|device| device.device_id.as_str()),
            Some(device.device_id.as_str())
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

        let rejected = store
            .list_audit_events(crate::state::AuditEventFilter {
                outcome: Some("rejected".into()),
                limit: 10,
                ..Default::default()
            })
            .await
            .expect("rejected list should succeed");
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].latency_ms, None);
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
