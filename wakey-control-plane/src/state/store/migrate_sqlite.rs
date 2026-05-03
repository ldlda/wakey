use super::*;

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
        if to_state_file.is_dir() {
            anyhow::bail!(
                "target state file {} is a directory",
                to_state_file.display()
            );
        }
        let is_same_file = match (
            std::fs::canonicalize(from_sqlite_state),
            std::fs::canonicalize(to_state_file),
        ) {
            (Ok(from_canon), Ok(to_canon)) => from_canon == to_canon,
            _ => from_sqlite_state == to_state_file,
        };

        let actual_from_path = if is_same_file {
            let mut bak = to_state_file.to_path_buf();
            let mut file_name = bak.file_name().unwrap_or_default().to_os_string();
            file_name.push(".bak");
            bak.set_file_name(file_name);

            if bak.exists() {
                if !force {
                    anyhow::bail!(
                        "backup file {} already exists; re-run with --force to overwrite",
                        bak.display()
                    );
                }
                std::fs::remove_file(&bak).with_context(|| {
                    format!("failed to remove existing backup {}", bak.display())
                })?;
            }

            std::fs::rename(from_sqlite_state, &bak)
                .with_context(|| "failed to rename legacy state for in-place migration")?;
            bak
        } else {
            if to_state_file.exists()
                && to_state_file
                    .metadata()
                    .with_context(|| format!("failed to stat {}", to_state_file.display()))?
                    .len()
                    > 0
            {
                if !force {
                    anyhow::bail!(
                        "target SQLite state file {} already exists and is non-empty; re-run with --force to overwrite",
                        to_state_file.display()
                    );
                }
                std::fs::remove_file(to_state_file)
                    .with_context(|| format!("failed to remove {}", to_state_file.display()))?;
            }
            from_sqlite_state.to_path_buf()
        };

        let store = Store::load_or_init(to_state_file, Vec::new(), Duration::from_secs(1)).await?;

        let from_path_str = actual_from_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("invalid path"))?;

        sqlx::query(&format!("ATTACH DATABASE '{}' AS legacy", from_path_str))
            .execute(&store.pool)
            .await
            .with_context(|| "failed to attach legacy database")?;

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
            let q = format!("INSERT INTO {} SELECT * FROM legacy.{}", table, table);
            sqlx::query(&q)
                .execute(&store.pool)
                .await
                .with_context(|| format!("failed to migrate table {}", table))?;
        }

        sqlx::query("DETACH DATABASE legacy")
            .execute(&store.pool)
            .await?;

        info!(
            from = %from_sqlite_state.display(),
            to = %to_state_file.display(),
            "migrated legacy sqlite state into new sqlite state"
        );
        Ok(())
    }
}
