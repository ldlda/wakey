use super::*;

impl Store {
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
}
