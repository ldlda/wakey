use super::*;

impl Store {
    pub async fn import_sled_state(
        from_sled_state: &Path,
        to_state_file: &Path,
        force: bool,
    ) -> Result<()> {
        if !from_sled_state.is_dir() {
            anyhow::bail!(
                "legacy sled state {} does not exist or is not a directory",
                from_sled_state.display()
            );
        }
        if to_state_file.is_dir() {
            anyhow::bail!(
                "target state file {} is a directory",
                to_state_file.display()
            );
        }
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

        let legacy = sled::open(from_sled_state).with_context(|| {
            format!(
                "failed to open legacy sled state {}",
                from_sled_state.display()
            )
        })?;
        let store = Store::load_or_init(to_state_file, Vec::new(), Duration::from_secs(1)).await?;

        import_tree_raw(&store.pool, &legacy, "meta", "meta", "key", "value").await?;
        import_enroll_tokens(&store.pool, &legacy).await?;
        import_agents(&store.pool, &legacy).await?;
        import_agent_meta(&store.pool, &legacy).await?;
        import_audit_events(&store.pool, &legacy).await?;
        import_active_alerts(&store.pool, &legacy).await?;
        import_alert_transitions(&store.pool, &legacy).await?;

        info!(
            from = %from_sled_state.display(),
            to = %to_state_file.display(),
            "imported legacy sled state into SQLite"
        );
        Ok(())
    }
}
