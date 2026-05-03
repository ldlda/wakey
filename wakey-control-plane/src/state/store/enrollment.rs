use super::*;

impl Store {
    pub async fn enroll(&self, enroll_token: &str) -> Result<IssuedAgent> {
        let mut tx = self.begin_write().await?;
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
        let rows = list_enroll_token_rows(&self.pool).await?;
        rows.into_iter()
            .map(|row| enroll_token_info_from_row(row, now))
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
        let mut tx = self.begin_write().await?;
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
}
