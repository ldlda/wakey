use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssuedAgent {
    pub agent_id: String,
    pub agent_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssuedEnrollToken {
    pub enroll_token: String,
    pub expires_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyPersistedState {
    enroll_tokens: std::collections::HashSet<String>,
    agents: HashMap<String, String>,
}

pub struct Store {
    db_path: PathBuf,
    enroll_tokens: sled::Tree,
    agents: sled::Tree,
}

impl Store {
    pub async fn load_or_init(path: &Path, enroll_tokens: Vec<String>, seed_ttl: Duration) -> Result<Self> {
        let db_path = canonical_db_path(path);
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create state dir {}", parent.display()))?;
        }

        let db = sled::open(&db_path)
            .with_context(|| format!("failed to open state db {}", db_path.display()))?;
        let enroll_tree = db
            .open_tree("enroll_tokens")
            .context("failed to open enroll_tokens tree")?;
        let agents_tree = db.open_tree("agents").context("failed to open agents tree")?;

        let store = Self {
            db_path,
            enroll_tokens: enroll_tree,
            agents: agents_tree,
        };

        store.maybe_migrate_legacy_json(path)?;

        for token in enroll_tokens {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            let expires_at = now_unix().saturating_add(seed_ttl.as_secs().max(1));
            store
                .enroll_tokens
                .insert(token.as_bytes(), &expires_at.to_le_bytes())
                .with_context(|| format!("failed to seed enroll token into {}", store.db_path.display()))?;
        }

        store.gc_expired_enroll_tokens()?;

        store
            .flush()
            .with_context(|| format!("failed to flush state db {}", store.db_path.display()))?;

        let enroll_tokens = store.enroll_tokens.iter().count();
        let agents = store.agents.iter().count();
        info!(
            path = %store.db_path.display(),
            enroll_tokens,
            agents,
            "control-plane store ready"
        );
        Ok(store)
    }

    pub async fn enroll(&self, enroll_token: &str) -> Result<IssuedAgent> {
        let Some(raw_expiry) = self
            .enroll_tokens
            .get(enroll_token.as_bytes())
            .context("failed reading enroll token")?
        else {
            warn!("rejecting enroll attempt with invalid, expired, or consumed token");
            anyhow::bail!("invalid or already-used enroll token");
        };

        let expires_at_unix = decode_expiry(raw_expiry.as_ref())
            .context("failed decoding enroll token expiry")?;
        let now = now_unix();
        if expires_at_unix <= now {
            let _ = self.enroll_tokens.remove(enroll_token.as_bytes());
            self.flush().ok();
            warn!(expires_at_unix, now_unix = now, "rejecting expired enroll token");
            anyhow::bail!("enroll token has expired");
        }

        self.enroll_tokens
            .remove(enroll_token.as_bytes())
            .context("failed consuming enroll token")?;

        let agent_id = format!("agent-{}", Uuid::new_v4());
        let agent_token = format!("tok-{}", Uuid::new_v4());

        self.agents
            .insert(agent_id.as_bytes(), agent_token.as_bytes())
            .context("failed persisting agent credentials")?;
        self.flush().context("failed flushing state db after enroll")?;
        info!(agent_id = %agent_id, "issued persistent agent credentials");

        Ok(IssuedAgent {
            agent_id,
            agent_token,
        })
    }

    pub async fn issue_enroll_token(&self, ttl: Duration) -> Result<IssuedEnrollToken> {
        let token = format!("enr-{}", Uuid::new_v4());
        let expires_at_unix = now_unix().saturating_add(ttl.as_secs().max(1));
        self.enroll_tokens
            .insert(token.as_bytes(), &expires_at_unix.to_le_bytes())
            .context("failed persisting enroll token")?;
        self.flush()
            .context("failed flushing state db after token issuance")?;
        info!(expires_at_unix, "persisted new enroll token");
        Ok(IssuedEnrollToken {
            enroll_token: token,
            expires_at_unix,
        })
    }

    pub async fn reload_from_disk(&self) -> Result<()> {
        // sled is durable and read-through; explicit reload is a no-op.
        info!(path = %self.db_path.display(), "reload requested; sled backend does not require in-memory reload");
        Ok(())
    }

    pub async fn verify_agent_token(&self, agent_id: &str, token: &str) -> bool {
        match self.agents.get(agent_id.as_bytes()) {
            Ok(Some(value)) => value.as_ref() == token.as_bytes(),
            Ok(None) => false,
            Err(err) => {
                warn!(error = %err, agent_id = %agent_id, "failed to read agent token from state db");
                false
            }
        }
    }

    pub async fn list_agents(&self) -> Vec<String> {
        let mut out = self
            .agents
            .iter()
            .filter_map(|item| item.ok())
            .filter_map(|(key, _)| String::from_utf8(key.to_vec()).ok())
            .collect::<Vec<_>>();
        out.sort();
        out
    }

    fn flush(&self) -> Result<()> {
        self.enroll_tokens
            .flush()
            .context("failed to flush enroll token tree")?;
        self.agents
            .flush()
            .context("failed to flush agents tree")?;
        debug!(path = %self.db_path.display(), "flushed sled state db");
        Ok(())
    }

    fn maybe_migrate_legacy_json(&self, configured_path: &Path) -> Result<()> {
        let legacy_path = match configured_path.extension().and_then(|x| x.to_str()) {
            Some("json") => configured_path.to_path_buf(),
            _ => configured_path.with_extension("json"),
        };
        if self.enroll_tokens.iter().next().is_some() || self.agents.iter().next().is_some() {
            return Ok(());
        }
        if !legacy_path.exists() {
            return Ok(());
        }

        let raw = std::fs::read_to_string(&legacy_path)
            .with_context(|| format!("failed to read legacy state {}", legacy_path.display()))?;
        let legacy: LegacyPersistedState = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse legacy state {}", legacy_path.display()))?;

        let expires_at_unix = now_unix().saturating_add(86_400);
        for token in legacy.enroll_tokens {
            self.enroll_tokens
            .insert(token.as_bytes(), &expires_at_unix.to_le_bytes())
                .context("failed to migrate enroll token")?;
        }
        for (agent_id, token) in legacy.agents {
            self.agents
                .insert(agent_id.as_bytes(), token.as_bytes())
                .context("failed to migrate agent token")?;
        }

        self.flush().context("failed flushing migrated legacy state")?;
        std::fs::remove_file(&legacy_path)
            .with_context(|| format!("failed to delete legacy state {}", legacy_path.display()))?;
        info!(legacy = %legacy_path.display(), db = %self.db_path.display(), "migrated legacy json state into sled db and deleted legacy file");
        Ok(())
    }

    fn gc_expired_enroll_tokens(&self) -> Result<()> {
        let now = now_unix();
        let mut removed = 0u64;
        for item in self.enroll_tokens.iter() {
            let (token, value) = item.context("failed iterating enroll token tree")?;
            let expires_at = decode_expiry(value.as_ref()).context("failed decoding token expiry during gc")?;
            if expires_at <= now {
                self.enroll_tokens
                    .remove(token)
                    .context("failed removing expired enroll token")?;
                removed = removed.saturating_add(1);
            }
        }
        if removed > 0 {
            self.flush().context("failed flushing db after gc")?;
            info!(removed, "garbage-collected expired enroll tokens");
        }
        Ok(())
    }
}

fn canonical_db_path(configured: &Path) -> PathBuf {
    match configured.extension().and_then(|x| x.to_str()) {
        Some("json") => configured.with_extension("db"),
        _ => configured.to_path_buf(),
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn decode_expiry(raw: &[u8]) -> Result<u64> {
    if raw.len() != 8 {
        anyhow::bail!("invalid token expiry length {}", raw.len());
    }
    let mut arr = [0u8; 8];
    arr.copy_from_slice(raw);
    Ok(u64::from_le_bytes(arr))
}
