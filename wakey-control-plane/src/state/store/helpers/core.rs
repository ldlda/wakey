use super::*;

pub async fn open_sqlite_pool(path: &Path) -> Result<SqlitePool> {
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

pub async fn sql_count(pool: &SqlitePool, table: &'static str) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    sqlx::query_scalar::<_, i64>(&sql)
        .fetch_one(pool)
        .await
        .with_context(|| format!("failed counting {table}"))
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn decode_schema(raw: &[u8]) -> Result<u32> {
    if raw.len() != 4 {
        anyhow::bail!("invalid schema version length {}", raw.len());
    }
    let mut arr = [0u8; 4];
    arr.copy_from_slice(raw);
    Ok(u32::from_le_bytes(arr))
}

pub fn seeded_enroll_token_key(token: &str) -> String {
    format!("{SEEDED_ENROLL_TOKEN_PREFIX}{token}")
}

pub fn normalize_required_text(value: &str, field: &str) -> Result<String> {
    let normalized = value.trim();
    if normalized.is_empty() {
        anyhow::bail!("{field} must not be empty");
    }
    Ok(normalized.to_string())
}

pub fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub fn normalize_device_identifier(
    input: DeviceIdentifierInput,
) -> Result<NormalizedDeviceIdentifier> {
    let kind = normalize_required_text(&input.kind, "identifier kind")?.to_ascii_lowercase();
    let value = normalize_required_text(&input.value, "identifier value")?.to_ascii_lowercase();
    let identifier_key = format!("{kind}:{value}");
    Ok(NormalizedDeviceIdentifier {
        identifier_key,
        kind,
        value,
    })
}

pub async fn list_enroll_token_rows(pool: &SqlitePool) -> Result<Vec<EnrollTokenRow>> {
    sqlx::query_as!(
        EnrollTokenRow,
        r#"SELECT token as "token!", expires_at_unix FROM enroll_tokens ORDER BY expires_at_unix, token"#,
    )
    .fetch_all(pool)
    .await
    .context("failed listing enroll tokens")
}

pub fn enroll_token_info_from_row(row: EnrollTokenRow, now: u64) -> Result<EnrollTokenInfo> {
    let expires_at_unix =
        u64::try_from(row.expires_at_unix).context("negative token expiry in state db")?;
    Ok(EnrollTokenInfo {
        enroll_token: row.token,
        expires_at_unix,
        expired: expires_at_unix <= now,
    })
}

pub async fn list_known_device_rows(pool: &SqlitePool) -> Result<Vec<KnownDeviceRow>> {
    sqlx::query_as!(
        KnownDeviceRow,
        r#"SELECT device_id as "device_id!", display_name as "display_name!",
                pinned, created_at_unix, updated_at_unix, notes
         FROM known_devices
         ORDER BY pinned DESC, display_name, device_id"#,
    )
    .fetch_all(pool)
    .await
    .context("failed listing known devices")
}

pub async fn get_known_device_row(
    pool: &SqlitePool,
    device_id: &str,
) -> Result<Option<KnownDeviceRow>> {
    sqlx::query_as!(
        KnownDeviceRow,
        r#"SELECT device_id as "device_id!", display_name as "display_name!",
                pinned, created_at_unix, updated_at_unix, notes
         FROM known_devices
         WHERE device_id = ?1"#,
        device_id
    )
    .fetch_optional(pool)
    .await
    .context("failed reading known device")
}

pub fn known_device_row_device_id(row: &KnownDeviceRow) -> &str {
    &row.device_id
}

pub fn known_device_from_row_and_identifiers(
    row: KnownDeviceRow,
    identifiers: Vec<DeviceIdentifier>,
) -> Result<KnownDevice> {
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

pub async fn list_device_identifier_rows(
    pool: &SqlitePool,
    device_id: &str,
) -> Result<Vec<DeviceIdentifierRow>> {
    sqlx::query_as!(
        DeviceIdentifierRow,
        r#"SELECT identifier_key as "identifier_key!", device_id as "device_id!",
                kind as "kind!", value as "value!", created_at_unix
         FROM device_identifiers
         WHERE device_id = ?1
         ORDER BY kind, value"#,
        device_id
    )
    .fetch_all(pool)
    .await
    .context("failed listing device identifiers")
}

pub async fn insert_device_identifier_tx(
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

pub fn device_identifier_from_row(row: DeviceIdentifierRow) -> Result<DeviceIdentifier> {
    Ok(DeviceIdentifier {
        identifier_key: row.identifier_key,
        device_id: row.device_id,
        kind: row.kind,
        value: row.value,
        created_at_unix: u64::try_from(row.created_at_unix)
            .context("negative device identifier timestamp in state db")?,
    })
}
