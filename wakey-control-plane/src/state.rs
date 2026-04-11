use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssuedAgent {
    pub agent_id: String,
    pub agent_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedState {
    enroll_tokens: HashSet<String>,
    agents: HashMap<String, String>,
}

pub struct Store {
    path: PathBuf,
    state: RwLock<PersistedState>,
}

impl Store {
    pub async fn load_or_init(path: &Path, enroll_tokens: Vec<String>) -> Result<Self> {
        let seeded_tokens = enroll_tokens
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<HashSet<_>>();

        let initial = if path.exists() {
            let raw = tokio::fs::read_to_string(path)
                .await
                .with_context(|| format!("failed to read store {}", path.display()))?;
            debug!(path = %path.display(), bytes = raw.len(), "loading persisted control-plane store");
            serde_json::from_str::<PersistedState>(&raw)
                .with_context(|| format!("failed to decode store {}", path.display()))?
        } else {
            info!(path = %path.display(), seeded_enroll_tokens = seeded_tokens.len(), "initializing new control-plane store");
            PersistedState {
                enroll_tokens: seeded_tokens,
                agents: HashMap::new(),
            }
        };

        let store = Self {
            path: path.to_path_buf(),
            state: RwLock::new(initial),
        };
        store.save().await?;
        let (enroll_tokens, agents) = {
            let snapshot = store.state.read().await;
            (snapshot.enroll_tokens.len(), snapshot.agents.len())
        };
        info!(
            path = %store.path.display(),
            enroll_tokens,
            agents,
            "control-plane store ready"
        );
        Ok(store)
    }

    pub async fn enroll(&self, enroll_token: &str) -> Result<IssuedAgent> {
        let mut state = self.state.write().await;
        if !state.enroll_tokens.remove(enroll_token) {
            warn!("rejecting enroll attempt with invalid or consumed token");
            anyhow::bail!("invalid or already-used enroll token");
        }

        let agent_id = format!("agent-{}", Uuid::new_v4());
        let agent_token = format!("tok-{}", Uuid::new_v4());
        state.agents.insert(agent_id.clone(), agent_token.clone());
        drop(state);
        self.save().await?;
        info!(agent_id = %agent_id, "issued persistent agent credentials");

        Ok(IssuedAgent {
            agent_id,
            agent_token,
        })
    }

    pub async fn issue_enroll_token(&self) -> Result<String> {
        let token = format!("enr-{}", Uuid::new_v4());
        let mut state = self.state.write().await;
        state.enroll_tokens.insert(token.clone());
        drop(state);
        self.save().await?;
        info!("persisted new enroll token");
        Ok(token)
    }

    pub async fn reload_from_disk(&self) -> Result<()> {
        if !self.path.exists() {
            warn!(path = %self.path.display(), "reload requested but store file is missing");
            return Ok(());
        }

        let raw = tokio::fs::read_to_string(&self.path)
            .await
            .with_context(|| format!("failed to read store {}", self.path.display()))?;
        let decoded = serde_json::from_str::<PersistedState>(&raw)
            .with_context(|| format!("failed to decode store {}", self.path.display()))?;

        let enroll_tokens = decoded.enroll_tokens.len();
        let agents = decoded.agents.len();
        *self.state.write().await = decoded;
        info!(path = %self.path.display(), enroll_tokens, agents, "reloaded control-plane store from disk");
        Ok(())
    }

    pub async fn verify_agent_token(&self, agent_id: &str, token: &str) -> bool {
        self.state
            .read()
            .await
            .agents
            .get(agent_id)
            .map(|stored| stored == token)
            .unwrap_or(false)
    }

    pub async fn list_agents(&self) -> Vec<String> {
        let mut out = self
            .state
            .read()
            .await
            .agents
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        out.sort();
        out
    }

    async fn save(&self) -> Result<()> {
        let snapshot = self.state.read().await;
        let body =
            serde_json::to_string_pretty(&*snapshot).context("failed to serialize store state")?;
        let body_len = body.len();

        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("failed to create store dir {}", parent.display()))?;
        }

        let tmp = self.path.with_extension("json.tmp");
        tokio::fs::write(&tmp, body)
            .await
            .with_context(|| format!("failed to write temp store {}", tmp.display()))?;
        tokio::fs::rename(&tmp, &self.path).await.with_context(|| {
            format!(
                "failed to atomically move temp store {} into {}",
                tmp.display(),
                self.path.display()
            )
        })?;
        debug!(
            path = %self.path.display(),
            bytes = body_len,
            enroll_tokens = snapshot.enroll_tokens.len(),
            agents = snapshot.agents.len(),
            "saved control-plane store"
        );
        Ok(())
    }
}
