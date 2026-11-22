use anyhow::{Context, Result};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::cli::EnrollArgs;
use crate::config::{self, AgentConfig, DEFAULT_CONFIG, save_config_with_backup};
use crate::serve;

struct EnrollOutcome {
    config: AgentConfig,
    backup_path: Option<PathBuf>,
}

pub(crate) async fn run(args: EnrollArgs) -> Result<()> {
    let existing_config = config::load_optional_config(&args.config)?;
    let server_url = args
        .server_url
        .as_deref()
        .or_else(|| existing_config.as_ref().map(|cfg| cfg.server_url.as_str()))
        .with_context(|| {
            format!(
                "missing control-plane URL: pass --server-url or provide server_url in {}",
                args.config.display()
            )
        })?;

    info!(%server_url, config = %args.config.display(), "wakey-agent command: enroll");
    let outcome = enroll(
        server_url,
        &args.enroll_token,
        &args.config,
        existing_config.as_ref(),
    )
    .await?;
    let pid_file = args.pid_file.as_deref().unwrap_or(&outcome.config.pid_file);

    println!("agent_id={}", outcome.config.agent_id);
    println!("config={}", args.config.display());
    println!("config_write=updated");
    if let Some(backup_path) = &outcome.backup_path {
        println!("config_backup={}", backup_path.display());
    }

    if args.reload_running {
        return reload_or_restore(pid_file, &args.config, outcome.backup_path.as_deref());
    }

    println!("reload=not_requested");
    println!("runtime_config=unchanged_until_reload_or_restart");
    println!(
        "next=wakey-agent reload --pid-file {}  # if daemon is already running",
        pid_file.display()
    );
    println!("run={}", serve::command_for_config(&args.config));
    Ok(())
}

fn reload_or_restore(pid_file: &Path, config_path: &Path, backup: Option<&Path>) -> Result<()> {
    let Err(err) = serve::reload_daemon(pid_file) else {
        println!("reload=signaled pid_file={}", pid_file.display());
        return Ok(());
    };

    warn!(error = %err, pid_file = %pid_file.display(), "enroll completed but daemon reload failed");
    let Some(backup_path) = backup else {
        return Err(
            err.context("reload failed after enroll; no previous config backup was available")
        );
    };
    if let Err(restore_err) = config::restore_backup(config_path, backup_path) {
        // Keep the reload failure as the source and report the separate restore failure too.
        return Err(err.context(format!(
            "reload failed after enroll and rollback failed: {restore_err:#}"
        )));
    }
    println!("rollback=restored backup={}", backup_path.display());
    Err(err.context(format!(
        "reload failed after enroll; restored previous config from {}",
        backup_path.display()
    )))
}

#[derive(Debug, Serialize)]
struct EnrollRequest<'a> {
    enroll_token: &'a str,
}

#[derive(Debug, Deserialize)]
struct EnrollResponse {
    agent_id: String,
    agent_token: String,
    server_url: Option<String>,
}

async fn enroll(
    server_url: &str,
    enroll_token: &str,
    config_path: &Path,
    base_config: Option<&AgentConfig>,
) -> Result<EnrollOutcome> {
    let server_url = normalize_server_url(server_url);
    let endpoint = format!("{server_url}/api/v1/agents/enroll");
    info!(endpoint = %endpoint, config_path = %config_path.display(), "starting agent enrollment");
    let client = reqwest::Client::new();
    let response = client
        .post(endpoint)
        .json(&EnrollRequest { enroll_token })
        .send()
        .await
        .context("failed to call enrollment endpoint")?;

    if response.status() != StatusCode::OK {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<unable to read enrollment error body>".into());
        warn!(status = %status, "agent enrollment request rejected by control-plane");
        anyhow::bail!("enrollment failed with {status}: {body}");
    }

    let payload: EnrollResponse = response
        .json()
        .await
        .context("failed to decode enrollment response")?;

    let mut config = base_config
        .cloned()
        .unwrap_or_else(|| (*DEFAULT_CONFIG).clone());
    config.server_url = payload.server_url.unwrap_or(server_url);
    config.agent_id = payload.agent_id;
    config.agent_token = payload.agent_token;
    let backup_path = save_config_with_backup(config_path, &config)?;
    info!(agent_id = %config.agent_id, config_path = %config_path.display(), "agent enrollment succeeded and config was written");
    Ok(EnrollOutcome {
        config,
        backup_path,
    })
}

fn normalize_server_url(server_url: &str) -> String {
    server_url.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener};
    use std::thread;

    fn spawn_enroll_server(
        response_body: &'static str,
        status: &'static str,
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let addr: SocketAddr = listener.local_addr().expect("local addr");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept connection");

            // Read until end-of-headers; body content is irrelevant for this test.
            let mut buf = [0u8; 4096];
            let mut req = Vec::new();
            loop {
                let n = stream.read(&mut buf).expect("read request");
                if n == 0 {
                    break;
                }
                req.extend_from_slice(&buf[..n]);
                if req.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }

            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
        });

        (format!("http://{}", addr), handle)
    }

    #[test]
    fn normalize_server_url_trims_slash() {
        assert_eq!(
            normalize_server_url("https://example.com/"),
            "https://example.com"
        );
    }

    #[test]
    fn reload_failure_restores_backup_and_preserves_cause() {
        let dir = std::env::temp_dir().join(format!(
            "wakey-agent-enroll-rollback-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join("config.toml");
        let backup = dir.join("config.toml.bak");
        std::fs::write(&config, "new").unwrap();
        std::fs::write(&backup, "old").unwrap();

        let err = reload_or_restore(&dir.join("missing.pid"), &config, Some(&backup))
            .expect_err("reload should fail for a missing PID file");

        assert_eq!(std::fs::read_to_string(&config).unwrap(), "old");
        assert!(err.to_string().contains("restored previous config"));
        assert_eq!(
            err.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::NotFound
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn enroll_response_persists_config() {
        let response = r#"{"agent_id":"agent-123","agent_token":"token-xyz","server_url":"https://control.example.com"}"#;
        let (server_url, handle) = spawn_enroll_server(response, "200 OK");

        let dir = std::env::temp_dir().join(format!(
            "wakey-agent-enroll-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let path = dir.join("config.toml");

        let base_config = AgentConfig {
            server_url: "https://old.example.com".into(),
            agent_id: "old-agent".into(),
            agent_token: "old-token".into(),
            reconnect_base: std::time::Duration::from_secs(2),
            reconnect_max: std::time::Duration::from_secs(60),
            observation_sync_interval: std::time::Duration::from_secs(30),
            observation_retention: std::time::Duration::from_secs(11 * 86_400),
            pid_file: "/tmp/custom-wakey-agent.pid".into(),
            dhcp_leases_path: "/tmp/custom-dhcp.leases".into(),
            mac_name_cache_path: "/tmp/custom-names.json".into(),
            observation_store_path: "/tmp/custom-observations.json".into(),
            telemetry: wakey_core::telemetry::TelemetryConfig {
                otlp_endpoint: Some("http://10.0.0.1:4317".into()),
                authorization_env: Some("WAKEY_OTLP_AUTHORIZATION".into()),
                service_name: "router-agent".into(),
                json_logs: true,
                ..Default::default()
            },
            terminal: crate::config::TerminalConfig {
                enabled: true,
                shell: "/bin/ash".into(),
                args: vec!["-l".into()],
                current_dir: Some("/root".into()),
                max_sessions: 2,
                session_ttl: std::time::Duration::from_secs(600),
            },
        };

        let outcome = enroll(&server_url, "enroll-abc", &path, Some(&base_config))
            .await
            .expect("enroll should succeed");
        let config = outcome.config;

        assert_eq!(config.agent_id, "agent-123");
        assert_eq!(config.agent_token, "token-xyz");
        assert_eq!(config.server_url, "https://control.example.com");
        assert_eq!(
            config.observation_retention,
            base_config.observation_retention
        );
        assert_eq!(config.pid_file, base_config.pid_file);
        assert_eq!(config.dhcp_leases_path, base_config.dhcp_leases_path);
        assert_eq!(config.terminal, base_config.terminal);
        assert!(outcome.backup_path.is_none());

        let persisted = crate::config::load_config(&path).expect("load persisted config");
        assert_eq!(persisted, config);

        handle.join().expect("server thread joined");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
