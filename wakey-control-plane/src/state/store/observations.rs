use super::*;

impl Store {
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
            let current = get_observation_current_row(&mut tx, &observation.observation_key)
                .await
                .context("failed checking existing observation")?;
            let append_event = observation_current_changed(
                current.as_ref(),
                &observation,
                first_seen_unix,
                last_seen_unix,
            );
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

            if append_event {
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
            }
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
}
