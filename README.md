# wakey

`wakey` is a Wake-on-LAN and LAN-observability tool for a Linux router.

It started as a small web UI running on-device. It is now being reshaped into a
service-first project with:

- a reusable core model
- a Linux/OpenWrt adapter layer
- a CLI for operators
- an outbound agent plus control-plane model

## What it does

`wakey` currently focuses on a small set of router-side jobs:

- inspect LAN neighbor state
- read DHCP lease data
- merge those facts into a higher-level device inventory
- list useful interface/broadcast information
- send Wake-on-LAN packets

In practice that means commands like:

```sh
wakey status bedroom-pc
wakey leases --include-state
wakey devs
wakey wake bedroom-pc
wakey wake --mac aa:bb:cc:dd:ee:ff
```

## Workspace layout

- `wakey-core`
  - shared types and parsing
  - device, neighbor, DHCP, interface, and wake models
- `wakey-linux`
  - Linux/OpenWrt adapter
  - DHCP lease loading, interface summaries, neighbor lookup, WoL sending
- `wakey`
  - service layer and operator CLI
- `wakey-agent`
  - outbound router daemon, enrollment, websocket command execution
- `wakey-control-plane`
  - enrollment endpoint, connected-agent registry, command relay API
- `ipjs`
  - typed wrappers around Linux `ip -j ...` data
  - JSON-first, with optional experimental netlink backends

## Current architecture

Inside the `wakey` crate:

- `src/service`
  - the real use-case layer
  - status, leases, inventory, interfaces, wake, and query resolution

The long-term direction is:

- keep the service layer stable
- keep the local CLI stable for operators
- run remote control via outbound agent + control-plane relay

## Future direction

The project now uses:

- `wakey` for local operator workflows and shared service behavior
- `wakey-agent` for outbound enrollment and websocket execution
- `wakey-control-plane` for enrollment, registry, and command relay

## Logging and troubleshooting

Both daemons emit structured tracing logs to stderr. If you are not seeing
enrollment/command activity, run with increased verbosity.

Control-plane also supports a config file at
`/etc/wakey-control-plane/config.toml` (override with `--config-file`) so you
can persist telemetry settings instead of passing flags.

You can scaffold this file with:

```sh
wakey-control-plane init-config
```

Or bootstrap once during serve (writes only if config is missing):

```sh
wakey-control-plane serve --bootstrap-config
```

Inspect current persisted state:

```sh
wakey-control-plane state-stats
```

List/revoke enroll tokens from CLI:

```sh
wakey-control-plane list-enroll-tokens --include-expired
wakey-control-plane revoke-enroll-token --token enr-...
```

Machine-readable output is available:

```sh
wakey-control-plane list-enroll-tokens --include-expired --json
wakey-control-plane state-stats --json
```

Example:

```toml
data_dir = "/var/lib/wakey-control-plane"
bind = "0.0.0.0:8080"
public_url = "https://cp.example.com"
state_file = "state.db"
pid_file = "wakey-control-plane.pid"
command_timeout_ms = 30000
enroll_token_ttl_seconds = 86400

[telemetry]
otlp_endpoint = "http://127.0.0.1:4317"
service_name = "wakey-control-plane"
json_logs = false
```

If `telemetry.otlp_endpoint` is omitted, logs still work normally and only local
structured logs are emitted.

State is persisted in an embedded `sled` database (default
`/var/lib/wakey-control-plane/state.db`).
Relative paths in config are resolved under `data_dir`.

Enroll tokens are now expiring and revocable. Issuance returns `expires_at_unix`.
Expired tokens are rejected on enroll and can be garbage-collected periodically
or on demand.

### Quick start

Control-plane:

```sh
wakey-control-plane -v serve --bind 0.0.0.0:8787 --public-url https://cp.example.com
```

Agent:

```sh
wakey-agent -v serve --config /etc/wakey-agent/config.toml
```

### Fine-grained log filters

Use `RUST_LOG` when you want to focus on websocket/API internals.

```sh
RUST_LOG=wakey_control_plane=debug,wakey_agent=debug wakey-control-plane serve
RUST_LOG=wakey_agent=debug wakey-agent serve
```

### What you should see

During registration/enroll:

- control-plane: `issued enroll token`, then `agent enrollment accepted`
- agent: `starting agent enrollment`, then `agent enrollment succeeded and config was written`

During live connectivity:

- control-plane: `agent websocket upgraded`, `agent authenticated`, `agent disconnected`
- agent: `connecting agent websocket`, `agent websocket session authenticated`, `heartbeat sent` (debug)

During command relay:

- control-plane: `dispatching command to agent`
- agent: `received command from control-plane`, then `command execution completed` (or `command dispatch failed`)
- control-plane: `agent command completed` (or timeout/error warnings)

During daemon control/state operations:

- control-plane: `wrote control-plane pid file`, `saved control-plane store`, `reloaded control-plane store from disk`
- agent: `wrote wakey-agent pid file`, `sending wakey-agent reload signal`

Control-plane admin API includes token management endpoints:

- `POST /api/v1/control/enroll-token?ttl_seconds=<n>`
- `GET /api/v1/control/enroll-tokens?include_expired=true|false`
- `DELETE /api/v1/control/enroll-tokens/{token}`

If commands still appear silent, verify both processes are running with `-v`
and that `RUST_LOG` is not overriding to a stricter level.

## CLI

`wakey` is usable as a local/operator CLI.

### Status

Show device status rows using a free-form selector:

```sh
wakey status bedroom-pc
```

Or use explicit filters:

```sh
wakey status --dev br-lan --nud reachable
wakey status --mac aa:bb:cc:dd:ee:ff
wakey status --json
```

### Leases

Show DHCP leases:

```sh
wakey leases
wakey leases --include-state
wakey leases --include-state --json
```

### Wake

Query mode:

```sh
wakey wake bedroom-pc
```

Explicit/manual mode:

```sh
wakey wake --mac aa:bb:cc:dd:ee:ff
wakey wake --mac aa:bb:cc:dd:ee:ff --ip 192.168.1.255
wakey wake --mac aa:bb:cc:dd:ee:ff --json
```

Rules:

- query mode and explicit `--mac/--ip` mode are mutually exclusive
- `--ip` requires `--mac`
- `--mac` without `--ip` fans out to interface broadcast targets

### Interfaces

Show condensed interface summaries:

```sh
wakey devs
wakey devs br-lan
wakey devs --up
wakey devs --json
```

## Tests

This repo has two useful testing modes:

- local compile checks
- live on-device tests against the real router/runtime environment

### Local

```sh
cargo check
cargo test --no-run
cargo clippy --all-targets --all-features -- -D warnings
```

Focused control-plane state tests:

```sh
cargo test -p wakey-control-plane state::store::tests::gc_removes_expired_tokens
cargo test -p wakey-control-plane state::store::tests::enroll_rejects_expired_token
cargo test -p wakey-control-plane state::store::tests::stats_counts_agents_and_expired_tokens
```

### On-device

Some integration tests are intentionally `#[ignore]` because they use real
router state. Use the PowerShell helper:

```powershell
./scripts/test_remote.ps1 -Package wakey -BinaryFilter integration_live_services -Ignored -NoCapture
./scripts/test_remote.ps1 -Package wakey -BinaryFilter integration_inventory -Ignored -NoCapture
```

You can further narrow execution with `-Filter` to select individual Rust test
functions inside a test binary.

## Build target

This project is primarily aimed at an OpenWrt/Linux ARM router target. The
workspace is commonly built for:

```text
armv7-unknown-linux-musleabihf
```

Some crates and tests are Linux-specific by design.

## Notes

- `ipjs` is JSON-first by default; experimental netlink paths exist where they
  are worth keeping.
- the current web client is temporary and kept alive through explicit
  compatibility mapping
- the service layer is the part intended to survive the migration
