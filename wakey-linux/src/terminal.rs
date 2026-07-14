//! Unix PTY ownership for interactive agent terminal sessions.

use std::path::Path;

use anyhow::{Context, Result};
use pty_process::{Command, OwnedReadPty, OwnedWritePty, Size};
use tokio::process::Child;

/// An interactive child process and the independently owned sides of its PTY.
///
/// Keeping the write half available is important: `OwnedWritePty` also owns the
/// resize operation used by terminal WebSocket control frames.
pub struct TerminalPty {
    pub reader: OwnedReadPty,
    pub writer: OwnedWritePty,
    pub child: Child,
}

impl TerminalPty {
    /// Starts `program` attached to a newly allocated PTY.
    pub fn spawn(program: &Path, rows: u16, cols: u16) -> Result<Self> {
        let (pty, pts) = pty_process::open().context("failed to open PTY")?;
        pty.resize(Size::new(rows, cols))
            .context("failed to set initial PTY size")?;

        let command = Command::new(program);
        let child = command
            .spawn(pts)
            .with_context(|| format!("failed to spawn {} in PTY", program.display()))?;
        let (reader, writer) = pty.into_split();

        Ok(Self {
            reader,
            writer,
            child,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    /// This test intentionally uses the same PTY implementation on the router.
    /// Run it remotely with:
    ///
    /// `./scripts/test_remote.ps1 -Package wakey-linux -Filter terminal::tests::pty_round_trip_and_resize -Exact -ShowOutput`
    #[tokio::test]
    async fn pty_round_trip_and_resize() {
        let TerminalPty {
            mut reader,
            mut writer,
            mut child,
        } = TerminalPty::spawn(Path::new("/bin/sh"), 24, 80).expect("spawn PTY shell");

        writer
            .resize(Size::new(31, 101))
            .expect("resize owned PTY writer");
        writer
            .write_all(b"read line; printf 'WAKEY_PTY_OK:%s\\n' \"$line\"; exit 0\nprobe\n")
            .await
            .expect("write shell input");
        let output = tokio::time::timeout(Duration::from_secs(5), async {
            let mut output = Vec::new();
            let mut chunk = [0_u8; 4096];
            while !String::from_utf8_lossy(&output).contains("WAKEY_PTY_OK:probe") {
                // Use a fresh ReadBuf for each PTY read. pty-process 0.5.3's
                // AsyncRead implementation cannot safely extend the partially
                // filled ReadBuf used internally by Tokio's `read_to_end`.
                let count = reader.read(&mut chunk).await.expect("read PTY output");
                assert_ne!(count, 0, "PTY closed before emitting probe marker");
                output.extend_from_slice(&chunk[..count]);
            }
            output
        })
        .await
        .expect("PTY output timed out");
        let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .expect("PTY child wait timed out")
            .expect("wait for PTY child");

        assert!(status.success(), "shell exited with {status}");
        let output = String::from_utf8_lossy(&output);
        assert!(
            output.contains("WAKEY_PTY_OK:probe"),
            "unexpected PTY output: {output:?}"
        );
    }
}
