use super::*;

pub(in crate::state::store) async fn insert_audit_event(
    pool: &SqlitePool,
    key: &str,
    event: &AuditEvent,
) -> Result<()> {
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

pub(in crate::state::store) async fn insert_active_alert(
    tx: &mut Transaction<'_, Sqlite>,
    alert: &AlertState,
) -> Result<()> {
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

pub(in crate::state::store) async fn insert_active_alert_pool(
    pool: &SqlitePool,
    alert: &AlertState,
) -> Result<()> {
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

pub(in crate::state::store) async fn insert_alert_transition(
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

pub(in crate::state::store) async fn insert_alert_transition_pool(
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

pub(in crate::state::store) fn audit_event_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<AuditEvent> {
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

pub(in crate::state::store) fn alert_state_from_row(row: AlertStateRow) -> Result<AlertState> {
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

pub(in crate::state::store) fn alert_transition_from_row(
    row: AlertTransitionRow,
) -> Result<AlertTransition> {
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
