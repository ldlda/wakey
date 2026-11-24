use std::collections::BTreeMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::{Json, extract::State};
use serde::Serialize;
use tokio::sync::RwLock;
use wakey_core::process_status::ProcessStatus;

use super::AppState;

const STALE_AFTER: Duration = Duration::from_secs(90);

#[derive(Default)]
pub(crate) struct ProcessStatusCache {
    control_plane: RwLock<Option<Sample>>,
    agents: RwLock<BTreeMap<String, AgentSample>>,
}

#[derive(Clone, Serialize)]
struct Sample {
    received_at_unix: u64,
    values: ProcessStatus,
    #[serde(skip)]
    received_at: Instant,
}

struct AgentSample {
    connection_id: String,
    sample: Sample,
}

impl Sample {
    fn new(values: ProcessStatus) -> Self {
        Self {
            received_at_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            received_at: Instant::now(),
            values,
        }
    }
}

impl ProcessStatusCache {
    pub(crate) async fn record_agent(
        &self,
        agent_id: &str,
        connection_id: &str,
        values: ProcessStatus,
    ) {
        self.agents.write().await.insert(
            agent_id.to_owned(),
            AgentSample {
                connection_id: connection_id.to_owned(),
                sample: Sample::new(values),
            },
        );
    }

    pub(crate) async fn forget_agent(&self, agent_id: &str) {
        self.agents.write().await.remove(agent_id);
    }
}

pub(super) struct ProcessStatusTask(tokio::task::JoinHandle<()>);

impl ProcessStatusTask {
    #[cfg(target_os = "linux")]
    pub(super) fn start(state: AppState) -> Self {
        Self(tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                let sample = tokio::task::spawn_blocking(|| {
                    wakey_linux::telemetry::ProcfsProcessMetrics.sample()
                })
                .await
                .ok()
                .map(Sample::new);
                *state.process_status.control_plane.write().await = sample;
            }
        }))
    }
}

impl Drop for ProcessStatusTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Serialize)]
pub(super) struct ProcessStatusResponse {
    control_plane: Option<Sample>,
    agents: BTreeMap<String, AgentStatus>,
}

#[derive(Serialize)]
struct AgentStatus {
    connected: bool,
    stale: bool,
    sample: Option<Sample>,
}

pub(super) async fn get_process_status(
    State(state): State<AppState>,
) -> Json<ProcessStatusResponse> {
    let control_plane = state.process_status.control_plane.read().await.clone();
    // Use the same lock order as heartbeat ingestion and revocation. A sample
    // from a previous socket remains visible but is stale until its replacement reports.
    let sessions = state.sessions.read().await;
    let samples = state.process_status.agents.read().await;
    let mut agents = BTreeMap::new();
    for (id, entry) in samples.iter() {
        let session = sessions.get(id);
        agents.insert(
            id.clone(),
            AgentStatus {
                connected: session.is_some(),
                stale: session.is_none_or(|session| session.connection_id != entry.connection_id)
                    || entry.sample.received_at.elapsed() >= STALE_AFTER,
                sample: Some(entry.sample.clone()),
            },
        );
    }
    for id in sessions.keys() {
        agents.entry(id.clone()).or_insert(AgentStatus {
            connected: true,
            stale: false,
            sample: None,
        });
    }
    Json(ProcessStatusResponse {
        control_plane,
        agents,
    })
}
