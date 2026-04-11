# Wakey Checkpoint (2026-04-11)

## Snapshot

This checkpoint captures the current state after control-plane migration, logging hardening, config ergonomics, and state storage upgrades.

## What Is Done

- Legacy router-hosted HTTP/static layer removed from `wakey` crate.
- New `wakey-control-plane` crate is active for:
  - enroll-token issuance
  - agent enrollment
  - connected-agent websocket registry
  - command relay to agent
- `wakey-agent` is active for outbound websocket execution and local command dispatch.

## Logging + Telemetry

- High-signal logs were added across:
  - control-plane API relay path
  - control-plane websocket lifecycle
  - control-plane state lifecycle
  - agent command lifecycle, session lifecycle, and enrollment
- Correlated relay spans include command context (`agent_id`, `request_id`, `command`).
- Control-plane telemetry is config-driven:
  - optional OTLP endpoint
  - optional JSON logs
  - fallback local logs when OTLP endpoint is not set

## Config Ergonomics

### Control-plane

- Config file support is wired (`/etc/wakey-control-plane/config.toml` by default).
- `serve` can read defaults from config file and CLI can override.
- New `init-config` command scaffolds a control-plane config file.

### Agent

- Existing `init-config` command scaffolds agent config.
- Enrollment can optionally signal reload of running daemon.

## State Storage Upgrade

- Control-plane state backend moved from JSON snapshot to embedded `sled` DB.
- Default state path changed to `/var/lib/wakey-control-plane/state.db`.
- Legacy JSON migration support exists:
  - if a `.json` path is configured and DB is empty, tokens/agents are migrated into DB.

## Operator Commands

### Control-plane bootstrap

```sh
wakey-control-plane init-config
wakey-control-plane serve --config-file /etc/wakey-control-plane/config.toml
```

### Issue enroll token (live daemon path)

```sh
wakey-control-plane issue-enroll-token --public-url https://cp.example.com
```

### Agent bootstrap

```sh
wakey-agent enroll --server-url https://cp.example.com --enroll-token <token>
wakey-agent serve --config /etc/wakey-agent/config.toml
```

## Build Health

- Last verified passing:
  - `cargo check --workspace`
  - `cargo clippy --workspace`

## Known Tradeoffs / Follow-ups

- Reload semantics with `sled` are now mostly no-op for in-memory state (data is durable in DB).
- No dedicated state-inspection CLI command yet (suggestion: add `state-stats` command).
- OTLP configuration is currently control-plane focused; agent parity can be added if needed.

## Suggested Next Steps

1. Add a control-plane `state-stats` command to print DB path, agent count, token count.
2. Add symmetric telemetry config support in `wakey-agent` config file.
3. Add integration tests for:
   - enroll + relay over websocket
   - legacy JSON-to-sled migration path
   - init-config command behavior and overrides
