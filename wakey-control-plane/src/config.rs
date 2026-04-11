use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use crate::cli::ServeArgs;

#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub bind: SocketAddr,
    pub public_url: String,
    pub state_file: PathBuf,
    pub command_timeout: Duration,
    pub pid_file: PathBuf,
}

impl DaemonConfig {
    pub fn from_serve_args(args: &ServeArgs) -> Self {
        Self {
            bind: args.bind,
            public_url: normalize_public_url(&args.public_url),
            state_file: args.state_file.clone(),
            command_timeout: Duration::from_millis(args.command_timeout_ms.max(1)),
            pid_file: args.pid_file.clone(),
        }
    }
}

pub fn normalize_public_url(url: &str) -> String {
    url.trim_end_matches('/').to_string()
}

pub fn issue_token_endpoint(base_url: &str) -> String {
    format!(
        "{}/api/v1/control/enroll-token",
        normalize_public_url(base_url)
    )
}
