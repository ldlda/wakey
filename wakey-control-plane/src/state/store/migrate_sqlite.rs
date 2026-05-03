use super::*;
use tracing::{debug, info, warn};

impl Store {
    pub async fn migrate_sqlite_state(
        from_sqlite_state: &Path,
        to_state_file: &Path,
        force: bool,
    ) -> Result<()> {
        if !from_sqlite_state.is_file() {
            anyhow::bail!(
                "legacy sqlite state {} does not exist or is not a file",
                from_sqlite_state.display()
            );
        }

        // 1. Safety check for destination
        if to_state_file.exists() && !force {
            anyhow::bail!(
                "target state file {} already exists; use --force to overwrite",
                to_state_file.display()
            );
        }

        // 2. Setup temporary migration target
        let mut temp_target = to_state_file.to_path_buf();
        let mut temp_name = temp_target.file_name().unwrap_or_default().to_os_string();
        temp_name.push(".migration_tmp");
        temp_target.set_file_name(temp_name);

        if temp_target.exists() {
            std::fs::remove_file(&temp_target)
                .context("failed to clean up stale migration temp file")?;
        }

        // 3. Initialize fresh schema on the temp file
        let store = Store::load_or_init(&temp_target, Vec::new(), Duration::from_secs(1)).await?;

        // 4. Use a single connection for the entire migration
        // ATTACH is connection-scoped, so we can't use the pool directly.
        let mut conn = store
            .pool
            .acquire()
            .await
            .context("failed to acquire migration connection")?;

        let from_path_str = from_sqlite_state
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("invalid legacy path"))?;

        sqlx::query(&format!("ATTACH DATABASE '{}' AS legacy", from_path_str))
            .execute(&mut *conn)
            .await
            .with_context(|| format!("failed to attach legacy database at {}", from_path_str))?;

        let legacy_tables: Vec<String> =
            sqlx::query_scalar("SELECT name FROM legacy.sqlite_master WHERE type='table'")
                .fetch_all(&mut *conn)
                .await
                .context("failed to list tables in legacy database")?;

        debug!(?legacy_tables, "found tables in legacy database");

        let tables = [
            "meta",
            "enroll_tokens",
            "agents",
            "agent_meta",
            "known_devices",
            "device_identifiers",
            "audit_events",
            "active_alerts",
            "alert_transitions",
        ];

        for table in tables {
            if !legacy_tables.contains(&table.to_string()) {
                warn!(table, "table missing in legacy database; skipping");
                continue;
            }

            let q = format!(
                "INSERT OR IGNORE INTO {} SELECT * FROM legacy.{}",
                table, table
            );
            sqlx::query(&q)
                .execute(&mut *conn)
                .await
                .with_context(|| format!("failed to migrate table {}", table))?;
        }

        sqlx::query("DETACH DATABASE legacy")
            .execute(&mut *conn)
            .await
            .context("failed to detach legacy database")?;

        // 5. Finalize: Force a checkpoint to roll up WAL, then close and move
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&mut *conn)
            .await
            .context("failed to checkpoint WAL before finalization")?;

        // Explicitly drop the connection so the pool can close properly
        drop(conn);
        store.pool.close().await;

        // Clean up any "ghost" WAL/SHM files at the destination to prevent corruption
        let to_path_str = to_state_file.to_string_lossy();
        let _ = std::fs::remove_file(format!("{}-wal", to_path_str));
        let _ = std::fs::remove_file(format!("{}-shm", to_path_str));

        std::fs::rename(&temp_target, to_state_file)
            .context("failed to move migrated database into final location")?;

        // Also clean up any leftover temp WAL/SHM files just in case
        let temp_path_str = temp_target.to_string_lossy();
        let _ = std::fs::remove_file(format!("{}-wal", temp_path_str));
        let _ = std::fs::remove_file(format!("{}-shm", temp_path_str));

        info!(
            from = %from_sqlite_state.display(),
            to = %to_state_file.display(),
            "migrated legacy sqlite state into new sqlite state"
        );
        Ok(())
    }
}
