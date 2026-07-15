//! Unix PTY ownership for interactive agent terminal sessions.

use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};

use anyhow::{Context, Result};
use pty_process::{Command, OwnedReadPty, OwnedWritePty, Size};
use tokio::io::AsyncWrite;
use tokio::process::Child;

/// An interactive child process and the independently owned sides of its PTY.
///
/// Keeping the write half available is important: `OwnedWritePty` also owns the
/// resize operation used by terminal WebSocket control frames.
pub struct TerminalPty {
    reader: OwnedReadPty,
    writer: TerminalWriter,
    child: Child,
}

/// Writable PTY half with the control operations needed by the agent.
pub struct TerminalWriter {
    io: OwnedWritePty,
    control: OwnedFd,
}

impl TerminalPty {
    /// Starts `program` attached to a newly allocated PTY.
    pub fn spawn(program: &Path, rows: u16, cols: u16) -> Result<Self> {
        let (pty, pts) = pty_process::open().context("failed to open PTY")?;
        pty.resize(Size::new(rows, cols))
            .context("failed to set initial PTY size")?;
        let control = pty
            .as_fd()
            .try_clone_to_owned()
            .context("failed to duplicate PTY control descriptor")?;

        // The remote frontend is xterm.js regardless of the daemon's own
        // environment, so advertise the terminal the child actually receives.
        let command = Command::new(program)
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .kill_on_drop(true);
        let child = command
            .spawn(pts)
            .with_context(|| format!("failed to spawn {} in PTY", program.display()))?;
        let (reader, io) = pty.into_split();

        Ok(Self {
            reader,
            writer: TerminalWriter { io, control },
            child,
        })
    }

    /// Consumes the terminal into independently driven async parts.
    pub fn into_parts(self) -> (OwnedReadPty, TerminalWriter, Child) {
        (self.reader, self.writer, self.child)
    }
}

impl TerminalWriter {
    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        self.io
            .resize(Size::new(rows, cols))
            .context("failed to resize PTY")
    }

    /// Requests a full-screen application to redraw after reattachment.
    ///
    /// Job-control shells give foreground programs their own process groups,
    /// so signaling the original shell group would miss programs like `btop`.
    pub fn refresh(&self) -> Result<()> {
        let foreground = nix::unistd::tcgetpgrp(&self.control)
            .context("failed to get PTY foreground process group")?;
        nix::sys::signal::killpg(foreground, nix::sys::signal::Signal::SIGWINCH)
            .context("failed to signal PTY foreground process group")
    }
}

impl AsyncWrite for TerminalWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.io).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.io).poll_shutdown(cx)
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
        let (mut reader, mut writer, mut child) = TerminalPty::spawn(Path::new("/bin/sh"), 24, 80)
            .expect("spawn PTY shell")
            .into_parts();

        writer.resize(31, 101).expect("resize owned PTY writer");
        writer
            .write_all(
                b"trap 'printf \"WAKEY_WINCH\\n\"' WINCH; \
                  printf 'WAKEY_ENV:%s:%s\\n' \"$TERM\" \"$COLORTERM\"; \
                  read line; printf 'WAKEY_PTY_OK:%s\\n' \"$line\"; exit 0\n",
            )
            .await
            .expect("write shell input");
        let mut output = Vec::new();
        read_until(
            &mut reader,
            &mut output,
            "WAKEY_ENV:xterm-256color:truecolor",
        )
        .await;
        writer.refresh().expect("signal foreground process group");
        writer
            .write_all(b"probe\n")
            .await
            .expect("write shell probe");
        let output = tokio::time::timeout(Duration::from_secs(5), async {
            read_until(&mut reader, &mut output, "WAKEY_PTY_OK:probe").await;
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
        assert!(
            output.contains("WAKEY_WINCH"),
            "foreground process did not receive SIGWINCH: {output:?}"
        );
    }

    async fn read_until(reader: &mut OwnedReadPty, output: &mut Vec<u8>, marker: &str) {
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut chunk = [0_u8; 4096];
            while !String::from_utf8_lossy(output).contains(marker) {
                // Use a fresh ReadBuf for each PTY read. pty-process 0.5.3's
                // AsyncRead implementation cannot safely extend Tokio's
                // partially filled buffer used by `read_to_end`.
                let count = reader.read(&mut chunk).await.expect("read PTY output");
                assert_ne!(count, 0, "PTY closed before emitting {marker}");
                output.extend_from_slice(&chunk[..count]);
            }
        })
        .await
        .unwrap_or_else(|_| panic!("PTY output timed out waiting for {marker}"));
    }
}
