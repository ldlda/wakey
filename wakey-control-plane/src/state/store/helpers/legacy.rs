use super::*;

pub(in crate::state::store) async fn import_tree_raw(
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

pub(in crate::state::store) async fn import_enroll_tokens(
    pool: &SqlitePool,
    legacy: &sled::Db,
) -> Result<()> {
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

pub(in crate::state::store) async fn import_agents(
    pool: &SqlitePool,
    legacy: &sled::Db,
) -> Result<()> {
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

pub(in crate::state::store) async fn import_agent_meta(
    pool: &SqlitePool,
    legacy: &sled::Db,
) -> Result<()> {
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

pub(in crate::state::store) async fn import_audit_events(
    pool: &SqlitePool,
    legacy: &sled::Db,
) -> Result<()> {
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

pub(in crate::state::store) async fn import_active_alerts(
    pool: &SqlitePool,
    legacy: &sled::Db,
) -> Result<()> {
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

pub(in crate::state::store) async fn import_alert_transitions(
    pool: &SqlitePool,
    legacy: &sled::Db,
) -> Result<()> {
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
