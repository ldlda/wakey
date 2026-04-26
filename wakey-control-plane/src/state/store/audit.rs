use super::*;

impl Store {
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
}
