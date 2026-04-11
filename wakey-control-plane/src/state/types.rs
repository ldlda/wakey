use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
pub struct EnrollTokenInfo {
    pub enroll_token: String,
    pub expires_at_unix: u64,
    pub expired: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateStats {
    pub db_path: PathBuf,
    pub schema_version: u32,
    pub agent_count: usize,
    pub enroll_token_count: usize,
    pub expired_enroll_token_count: usize,
}
