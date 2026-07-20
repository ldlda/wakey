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
6. The control plane pairs the sockets and relays terminal frames while an operator is attached.

The main agent WebSocket carries terminal creation and cancellation control only. It does not carry PTY input or output.

## PTY Ownership

The agent owns the PTY, shell process, process group, and all process cleanup. The control plane never spawns or emulates a shell.

Use `pty-process` with its async feature as the initial PTY abstraction because it provides:

- Tokio `AsyncRead` and `AsyncWrite` integration;
- owned read and write halves for concurrent tasks;
- terminal resizing;
- child session-leader and controlling-terminal setup; and
- a small implementation built on `rustix`.

The PTY wrapper is cross-compiled for `armv7-unknown-linux-musleabihf` and its input, output, resize, and exit test has passed on the target router. That focused test remains part of the remote compatibility workflow. Wakey does not call `forkpty` or add its own unsafe PTY implementation.

The executable, working directory, environment, UID, and GID are agent-controlled configuration. The browser cannot supply an arbitrary executable or process environment.

## Wire Protocol

WebSocket frame type separates terminal data from terminal control:

- Binary frames carry raw PTY input and output bytes.
- Text JSON frames carry lifecycle, error, and resize messages.

The terminal control vocabulary is deliberately small:

```text
operator -> agent: resize, close
control plane -> agent: snapshot
agent -> operator: ready, exited, error
```

Initial terminal dimensions are part of session attachment. Resize carries rows and columns; pixel dimensions may be added later if a demonstrated program requires them.

Terminal behavior such as Ctrl-C, Ctrl-Z, Ctrl-D, cursor keys, mouse reporting, bracketed paste, colors, and terminal-title changes remains in the byte stream. The control plane does not parse ANSI escape sequences. The browser terminal emulator renders output and owns connected-session scrollback. The agent also feeds PTY output into a `vt100` parser so it can reconstruct the current visible screen and terminal input modes after detachment. Agent PTYs advertise `TERM=xterm-256color` and `COLORTERM=truecolor` because xterm.js is the actual frontend; applications may therefore negotiate xterm mouse reporting without a Wakey-specific mouse protocol.

WebSockets already provide ordered, reliable delivery, so terminal frames do not carry sequence numbers in the initial protocol.

## Lifecycle and Cleanup

Terminal sessions allow only one attached operator. Their lifetime belongs to the agent process, not to a browser route or a particular control-plane connection.

When the operator socket disconnects, the session becomes detached. Navigation, browser closure, and network loss do not terminate the PTY. A protected operator request may discover the live session and obtain a fresh, single-use attachment credential. The control plane does not retain terminal output.

The operator UI lists live sessions as tabs and stores only the active terminal ID in browser-tab-scoped `sessionStorage`. On reload it prefers that ID and briefly waits for the previous WebSocket to detach before requesting a fresh attachment credential. The stored ID is a navigation hint, not an ownership credential; another attached operator still locks the session at the control plane.

After operator attachment, the control plane sends a `snapshot` request. The agent serializes up to 5,000 retained physical rows followed by its parsed current screen and input modes as terminal escape bytes, sends them before subsequent live output, and then signals the PTY's current foreground process group with `SIGWINCH`. Historical rows retain their terminal attributes and are replayed as ordinary terminal output so xterm builds its native scrollback; the final formatted state restores the exact live screen. The signal lets full-screen applications redraw without assuming that the original shell remains in the foreground. Signal failure is logged but never terminates the session.

Wakey carries a small, read-only extension to `vt100` because its public API exposes only the active grid. A snapshot streams bounded history from the primary grid, redraws that grid with its saved cursor and attributes, and then reconstructs the alternate grid when a full-screen application is active. The parser is never resized or switched while taking a snapshot. Consequently, reconnecting during `btop` restores the application while preserving the hidden shell history and state that reappear when it exits.

The agent keeps its terminal manager outside the control-WebSocket reconnect loop. If a dedicated terminal relay disconnects, the PTY continues draining output into the agent-owned terminal parser while waiting for replacement relay credentials. On every authenticated control connection, the agent reports its in-memory live terminal IDs and creation times. The control plane reconciles that inventory, adopts sessions missing from its volatile registry, and issues fresh relay credentials. This allows sessions to survive control-plane restart without persisting terminal state in SQLite.

Closing a session must terminate the entire PTY process group, not only the shell process. Cleanup should send a hangup first, then escalate to termination and forced kill after bounded grace periods. Explicit operator close, absolute session timeout, agent process exit, and machine reboot close the session.

The agent configuration sets `terminal.session_ttl_seconds` for newly created PTYs. The default is 43,200 seconds (12 hours); explicit zero disables automatic expiry. The agent owns and enforces this deadline locally because PTYs survive browser and control-plane disconnection. It advertises the policy to the control plane and includes the creation-time TTL in terminal inventory, allowing CC to mirror expiry and preserve it across CC restart. Missing fields from older agents retain the 12-hour default. Positive values are exact and values that cannot fit the platform timer are rejected rather than clamped.

An agent process restart or machine reboot still ends every terminal session because PTY file descriptors cannot be recovered by a new process. Surviving that boundary would require an external session host such as `tmux` or a separate terminal daemon and is not part of this design.

The agent reports a normal exit status or terminating signal when available. The UI must distinguish an exited shell from a transport failure.

## Backpressure and Resource Limits

Terminal transport must not use unbounded queues.

Every queue between PTY, agent socket, control-plane relay, and browser socket is bounded. The agent keeps draining and parsing the PTY while detached so a noisy child cannot stall the agent. Parsed state is bounded by the configured terminal dimensions and a 5,000-row scrollback limit. The control plane retains only a bounded queue of operator controls while an agent relay is reconnecting.

While a relay is attached, saturation applies lossless backpressure instead of dropping terminal bytes or disconnecting the relay. The agent retains the frame that discovered saturation and pauses further PTY reads until bounded relay capacity returns. Input, resize, close, lifecycle, and reconnect handling remain active while output is backpressured. Reconnect snapshots use the same ordered output path and duplicate snapshot requests are coalesced while one is pending. This is required for terminal protocols such as sixel, where dropping one output frame corrupts the rest of the stream.

When input/control and output are ready simultaneously, relay loops prioritize input and control. This keeps interrupt, resize, close, and snapshot traffic responsive while commands produce sustained output.

The implementation also enforces:

- maximum terminal frame size;
- per-agent concurrent-session limits;
- idle and absolute session timeouts;
- bounded parsed terminal state;
- bounded control-plane relay buffers; and
- cleanup when any relay task exits unexpectedly.

Exact limits are configuration and may be tuned from router testing. The bounded behavior is an architectural requirement.

## Security and Audit

Remote terminal access is root-equivalent on agents that run as root. It is therefore an explicit agent capability and is disabled by default.

Operator terminal endpoints belong to the protected control API surface. Session identifiers are not credentials. Agent and operator attachment credentials are short-lived, scoped to one terminal session, and single-use. The operator WebSocket must enforce the same-origin/protected-control assumptions used when the session was created.

Audit records include terminal request, open, ready, disconnect, close, timeout, and exit metadata. Wakey never stores or audits terminal input, terminal output, command history, or scrollback.

Terminal capability and session state are separate from device inventory, known-device identity, wake routes, and the debug Command Runner.

## Deferred Work

Persistence across agent restart or machine reboot is not supported. A later design would need an external process owner, reconnectable local IPC, and explicit secret handling; persisted metadata alone cannot recover a PTY.

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
- Control-plane terminal state remains ephemeral; agents rebuild it from their in-memory live-session inventory after a control-plane restart.
- The agent session protocol gains terminal-open control and capability advertisement but does not become a terminal-data multiplexer.
- Browser and agent terminal transports can be tested independently before adding xterm.js.
- Browser navigation and control-plane restart do not terminate an agent-owned PTY.
