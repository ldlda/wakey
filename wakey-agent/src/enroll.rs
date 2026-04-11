use anyhow::{Context, Result};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::info;

use crate::config::{AgentConfig, save_config};

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

pub async fn enroll(server_url: &str, enroll_token: &str, config_path: &Path) -> Result<AgentConfig> {
    let server_url = normalize_server_url(server_url);
    let endpoint = format!("{server_url}/api/v1/agents/enroll");
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
        anyhow::bail!("enrollment failed with {status}: {body}");
    }

    let payload: EnrollResponse = response
        .json()
        .await
        .context("failed to decode enrollment response")?;

    let config = AgentConfig {
        server_url: payload.server_url.unwrap_or(server_url),
        agent_id: payload.agent_id,
        agent_token: payload.agent_token,
        reconnect_base_ms: 1_000,
        reconnect_max_ms: 30_000,
    };
    save_config(config_path, &config)?;
    info!(config_path = %config_path.display(), "wrote agent config");
    Ok(config)
}

pub fn normalize_server_url(server_url: &str) -> String {
    server_url.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener};
    use std::thread;

    fn spawn_enroll_server(response_body: &'static str, status: &'static str) -> (String, thread::JoinHandle<()>) {
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
        assert_eq!(normalize_server_url("https://example.com/"), "https://example.com");
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

        let config = enroll(&server_url, "enroll-abc", &path)
            .await
            .expect("enroll should succeed");

        assert_eq!(config.agent_id, "agent-123");
        assert_eq!(config.agent_token, "token-xyz");
        assert_eq!(config.server_url, "https://control.example.com");

        let persisted = crate::config::load_config(&path).expect("load persisted config");
        assert_eq!(persisted, config);

        handle.join().expect("server thread joined");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
