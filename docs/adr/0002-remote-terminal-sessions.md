# Remote Terminal Sessions

Wakey will provide opt-in remote terminal access by pairing a browser terminal WebSocket with a dedicated outbound agent terminal WebSocket. The existing agent WebSocket remains the control channel and does not carry terminal byte streams.

**Status**: accepted

## Context

The control plane already maintains an authenticated outbound WebSocket from each connected agent. Its command protocol is intentionally request/result shaped: the control plane sends one command, the agent executes it, and one result completes the pending request.

An interactive terminal has different semantics. It is long-lived, bidirectional, byte-oriented, and must handle window resizing, process exit, disconnect cleanup, and backpressure. Treating it as an ordinary command would either block the agent session loop or turn the main agent WebSocket into a general stream multiplexer. It would also allow terminal output to compete with heartbeats, snapshots, and wake commands.

Agents commonly run behind NAT and must not expose an inbound listener. Terminal connections therefore remain agent-initiated and outbound.

## Decision

Use one dedicated agent WebSocket and one dedicated operator WebSocket for each terminal session. The control plane holds an in-memory terminal relay that pairs those sockets.

Session establishment follows this sequence:

1. An authenticated operator requests a terminal for a connected, terminal-capable agent.
2. The control plane creates an ephemeral terminal ID plus short-lived, single-use attachment credentials.
3. The control plane sends an open-terminal control message over the existing authenticated agent WebSocket.
4. The agent opens a new outbound terminal WebSocket to the control plane and authenticates it for that terminal ID.
5. The browser opens the corresponding protected operator terminal WebSocket.
6. The control plane pairs the sockets and relays terminal frames until either side exits or disconnects.

The main agent WebSocket carries terminal creation and cancellation control only. It does not carry PTY input or output.

## PTY Ownership

The agent owns the PTY, shell process, process group, and all process cleanup. The control plane never spawns or emulates a shell.

Use `pty-process` with its async feature as the initial PTY abstraction because it provides:

- Tokio `AsyncRead` and `AsyncWrite` integration;
- owned read and write halves for concurrent tasks;
- terminal resizing;
- child session-leader and controlling-terminal setup; and
- a small implementation built on `rustix`.

Adoption remains gated on successfully cross-compiling for the router target and exercising `/bin/ash` or the configured shell on a real device. Wakey does not call `forkpty` or add its own unsafe PTY implementation.

The executable, working directory, environment, UID, and GID are agent-controlled configuration. The browser cannot supply an arbitrary executable or process environment.

## Wire Protocol

WebSocket frame type separates terminal data from terminal control:

- Binary frames carry raw PTY input and output bytes.
- Text JSON frames carry lifecycle, error, and resize messages.

The terminal control vocabulary is deliberately small:

```text
operator -> agent: resize, close
agent -> operator: ready, exited, error
```

Initial terminal dimensions are part of session attachment. Resize carries rows and columns; pixel dimensions may be added later if a demonstrated program requires them.

Terminal behavior such as Ctrl-C, Ctrl-Z, Ctrl-D, cursor keys, mouse reporting, bracketed paste, colors, and terminal-title changes remains in the byte stream. The control plane does not parse ANSI escape sequences. The browser terminal emulator renders output and owns connected-session scrollback.

WebSockets already provide ordered, reliable delivery, so terminal frames do not carry sequence numbers in the initial protocol.

## Lifecycle and Cleanup

Initial terminal sessions are ephemeral and allow only one attached operator.

When the operator socket disconnects, the control plane keeps the agent socket and PTY alive for a short grace period. A protected operator request may obtain a fresh, single-use attachment credential for that existing session. The control plane retains only a bounded in-memory output buffer during the gap, replays it before live output on reattachment, and closes the session if no operator returns before the grace period expires.

An agent terminal socket disconnect closes the session immediately because the control plane can no longer control or observe the PTY. Agent reconnect creates a new agent session and cannot adopt an old terminal.

Closing a session must terminate the entire PTY process group, not only the shell process. Cleanup should send a hangup first, then escalate to termination and forced kill after bounded grace periods. Agent reconnect, agent replacement, control-plane restart, token expiry before attachment, and absolute session timeout all close the session.

The agent reports a normal exit status or terminating signal when available. The UI must distinguish an exited shell from a transport failure.

## Backpressure and Resource Limits

Terminal transport must not use unbounded queues.

Every queue between PTY, agent socket, control-plane relay, and browser socket is bounded. The brief-disconnect replay buffer is bounded as well. When the downstream consumer is slow, the producer waits rather than accumulating output in memory. Backpressure eventually stops PTY reads and allows the kernel PTY buffer to block an abusive producer.

The implementation also enforces:

- maximum terminal frame size;
- per-agent concurrent-session limits;
- idle and absolute session timeouts;
- bounded disconnect grace;
- bounded control-plane relay buffers; and
- cleanup when any relay task exits unexpectedly.

Exact limits are configuration and may be tuned from router testing. The bounded behavior is an architectural requirement.

## Security and Audit

Remote terminal access is root-equivalent on agents that run as root. It is therefore an explicit agent capability and is disabled by default.

Operator terminal endpoints belong to the protected control API surface. Session identifiers are not credentials. Agent and operator attachment credentials are short-lived, scoped to one terminal session, and single-use. The operator WebSocket must enforce the same-origin/protected-control assumptions used when the session was created.

Audit records include terminal request, open, ready, disconnect, close, timeout, and exit metadata. Wakey never stores or audits terminal input, terminal output, command history, or scrollback.

Terminal capability and session state are separate from device inventory, known-device identity, wake routes, and the debug Command Runner.

## Deferred Work

Durable or long-lived terminal sessions are not part of the initial version. A later design may preserve sessions across longer operator absences or control-plane restart. That design must define durable ownership, reconnect credentials, persisted replay bounds, secret handling, and process reconciliation before adding persistence.

File transfer, multi-operator attachment, terminal recording, shell-history storage, and arbitrary process launch are also deferred.

## Alternatives Considered

### Multiplex terminal bytes over the main agent WebSocket

Rejected for the initial version. It requires refactoring the current request/result session into a general concurrent writer and stream router, adds head-of-line coupling with fleet traffic, and requires terminal-byte framing within a shared connection.

### Model a terminal as a long-running agent command

Rejected. Commands have one result and a timeout; terminals have continuous bidirectional traffic and independent lifecycle.

### Accept an inbound connection on the agent

Rejected. Agents run behind NAT and should not expose a new network service.

### Use `portable-pty`

Viable, but not selected initially. Its cross-platform abstraction is broader than the Linux agent requires and its reader/writer API would need blocking-task bridges. It remains a fallback if `pty-process` does not build or behave correctly on the target router.

## Consequences

- Each active terminal consumes two additional WebSockets and one PTY process on the agent.
- Terminal traffic cannot delay main agent heartbeat, snapshot, or command messages.
- Control-plane terminal state is simple and ephemeral; restarting the control plane closes active terminals.
- The agent session protocol gains terminal-open control and capability advertisement but does not become a terminal-data multiplexer.
- Browser and agent terminal transports can be tested independently before adding xterm.js.
- Future resume support can extend terminal-session lifecycle without changing the basic PTY byte protocol.
