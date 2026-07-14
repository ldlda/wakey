use super::*;
impl Store {
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

        let mut tx = self.begin_write().await?;
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
        let rows = list_known_device_rows(&self.pool).await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let device_id = known_device_row_device_id(&row).to_string();
            out.push(self.known_device_from_row(row, &device_id).await?);
        }
        Ok(out)
    }

    pub async fn get_known_device(&self, device_id: &str) -> Result<Option<KnownDevice>> {
        let row = get_known_device_row(&self.pool, device_id).await?;

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

    pub async fn merge_known_devices(
        &self,
        target_device_id: &str,
        source_device_id: &str,
    ) -> Result<Option<KnownDevice>> {
        if target_device_id == source_device_id {
            return self.get_known_device(target_device_id).await;
        }

        let now = now_unix();
        let now_i64 = i64::try_from(now).context("known device timestamp overflow")?;
        let mut tx = self.begin_write().await?;

        let target_exists = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM known_devices WHERE device_id = ?1"#,
            target_device_id
        )
        .fetch_one(&mut *tx)
        .await
        .context("failed checking target known device existence")?;
        if target_exists == 0 {
            return Ok(None);
        }

        let source_exists = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!: i64" FROM known_devices WHERE device_id = ?1"#,
            source_device_id
        )
        .fetch_one(&mut *tx)
        .await
        .context("failed checking source known device existence")?;
        if source_exists == 0 {
            return Ok(None);
        }

        sqlx::query!(
            "UPDATE device_identifiers SET device_id = ?1 WHERE device_id = ?2",
            target_device_id,
            source_device_id
        )
        .execute(&mut *tx)
        .await
        .context("failed moving source identifiers to target device")?;
        sqlx::query!(
            "DELETE FROM known_devices WHERE device_id = ?1",
            source_device_id
        )
        .execute(&mut *tx)
        .await
        .context("failed deleting merged source known device")?;
        sqlx::query!(
            "UPDATE known_devices SET updated_at_unix = ?1 WHERE device_id = ?2",
            now_i64,
            target_device_id
        )
        .execute(&mut *tx)
        .await
        .context("failed updating merged target known device timestamp")?;

        tx.commit()
            .await
            .context("failed committing known device merge transaction")?;
        self.get_known_device(target_device_id).await
    }

    pub async fn attach_device_identifier(
        &self,
        device_id: &str,
        input: DeviceIdentifierInput,
    ) -> Result<Option<KnownDevice>> {
        let identifier = normalize_device_identifier(input)?;
        let now = now_unix();
        let mut tx = self.begin_write().await?;
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

    pub async fn detach_device_identifier(
        &self,
        device_id: &str,
        identifier_key: &str,
    ) -> Result<Option<KnownDevice>> {
        let now = now_unix();
        let now_i64 = i64::try_from(now).context("known device timestamp overflow")?;
        let mut tx = self.begin_write().await?;
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

        sqlx::query!(
            "DELETE FROM device_identifiers WHERE device_id = ?1 AND identifier_key = ?2",
            device_id,
            identifier_key
        )
        .execute(&mut *tx)
        .await
        .context("failed detaching device identifier")?;
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
            .context("failed committing device identifier detach transaction")?;
        self.get_known_device(device_id).await
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub async fn lookup_known_device_by_identifier(
        &self,
        input: DeviceIdentifierInput,
    ) -> Result<Option<KnownDevice>> {
        let identifier = normalize_device_identifier(input)?;
        let identifier_key = identifier.identifier_key;
        let device_id = sqlx::query_scalar!(
            "SELECT device_id FROM device_identifiers WHERE identifier_key = ?1",
            identifier_key
        )
        .fetch_optional(&self.pool)
        .await
        .context("failed looking up known device identifier")?;
        match device_id {
            Some(device_id) => self.get_known_device(&device_id).await,
            None => Ok(None),
        }
    }

    async fn known_device_from_row(
        &self,
        row: KnownDeviceRow,
        device_id: &str,
    ) -> Result<KnownDevice> {
        let identifiers = self.list_device_identifiers(device_id).await?;
        known_device_from_row_and_identifiers(row, identifiers)
    }

    async fn list_device_identifiers(&self, device_id: &str) -> Result<Vec<DeviceIdentifier>> {
        let rows = list_device_identifier_rows(&self.pool, device_id).await?;
        rows.into_iter().map(device_identifier_from_row).collect()
    }
}
