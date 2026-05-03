use super::*;

impl Store {
    pub async fn load_or_init(
        path: &Path,
        enroll_tokens: Vec<String>,
        seed_ttl: Duration,
    ) -> Result<Self> {
        if path.is_dir() {
            anyhow::bail!(
                "state_file {} is a directory, which looks like a legacy sled store; sled state is no longer supported",
                path.display(),
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
            let mut tx = self.begin_write().await?;
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
}
