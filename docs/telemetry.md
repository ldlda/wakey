# Telemetry

CC and the agent daemon share the optional `wakey-core/telemetry` module.
Local logs remain enabled. Configure an OTLP **gRPC** receiver to additionally
export traces, structured logs, and metrics; HTTP/4318 is not supported in this slice.

CC configuration:

```toml
[telemetry]
otlp_endpoint = "http://127.0.0.1:4317"
authorization_env = "WAKEY_OTLP_AUTHORIZATION"
service_name = "wakey-control-plane"
json_logs = true
```

Agent configuration (replace the example with the collector's WireGuard address):

```toml
[telemetry]
otlp_endpoint = "http://10.5.0.1:4317"
authorization_env = "WAKEY_OTLP_AUTHORIZATION"
service_name = "wakey-agent"
json_logs = false
```

Omit `otlp_endpoint` to disable exporting. Omit `authorization_env` for a receiver
that requires no authentication. Existing configs need no changes.
An empty agent service name uses `wakey-agent`; its enrolled agent ID becomes
the resource's `service.instance.id`, not a metric label. Telemetry config changes
require a daemon restart, not SIGHUP. Short-lived enroll/reload/observe commands
only log locally and do not initialize exporters.

## Receiver and filtering

The collector must enable OTLP receivers in **traces, logs, and metrics** pipelines.
Loopback is sufficient for CC on the same host/network namespace. The router
needs a collector listener reachable over WireGuard; do not expose an unauthenticated
receiver on the public Internet. This initial setup assumes plaintext inside that
tunnel. `json_logs` changes local output,
not whether OTLP logs are exported.

## Credentials and services

The OpenWrt agent uses `/etc/config/wakey`; see the
[UCI setup guide](openwrt-uci.md) for commands and permissions.
The CC uses a systemd environment file as described below.

`authorization_env` names an environment variable containing the complete header:
`Basic <base64(username:password)>` or `Bearer <token>`. With exporting enabled,
missing/empty/invalid values fail startup; credentials are never written to TOML
or included in error messages. Metadata is marked sensitive for Debug output.
Base64 is not encryption: use the WireGuard/SSH tunnel or another protected transport.

The systemd CC reads the optional `/etc/wakey-control-plane/environment` file.

Use this portable literal format, **without quotes or `export`**:

```text
# Replace this placeholder with the actual encoded credential locally.
WAKEY_OTLP_AUTHORIZATION=Basic BASE64_OF_USERNAME_COLON_PASSWORD
RUST_LOG=info
```

Protect the file with owner `root:root` and mode `0600`. systemd's manager reads it
before switching to the service user. Use the simple format above, with LF endings
and no inline comments or multiline values.
An absent file is allowed; configuring auth without providing its variable is not.

After changing the file, **restart** the service. On systemd, run `daemon-reload`
once after installing the updated unit. OpenWrt's HUP reload does not reload the
process environment. For a foreground WSL test, export the variable in that shell
and use `http://127.0.0.1:4317` with the SSH tunnel; do not put credentials in CLI args.

`RUST_LOG` overrides the verbosity defaults for both local output and exports.
For example, `RUST_LOG=warn` suppresses informational command spans as well as logs.
Use `wakey_agent=info,wakey=info,wakey_core=info,opentelemetry=warn` for the agent and
`wakey_control_plane=info,wakey_core=info,opentelemetry=warn` for CC. Exporter warnings
remain local to prevent export-failure feedback loops. Increasing verbosity exports the
existing debug events too; inspect them before enabling it broadly.

## Signals

- `relay_command` on CC propagates W3C context in an optional command field.
  `execute_command` on the agent adopts it; older messages/agents remain compatible.
- `collect_device_snapshot` and `store_device_snapshot` measure collection and
  persistence separately. Snapshot traces are not yet linked across the wire.
- Terminal create/attach/close/resume spans describe lifecycle operations, not
  output chunks. Trace-linked OTLP logs use the SDK's tracing-context bridge.
- Inventory source spans separate neighbor, DHCP lease, and hook-store reads
  from merging. `db_*` spans cover snapshot replacement, fleet reads, and manual
  identity changes, including transaction waits and result assembly where applicable.
- Fleet spans cover list/load/grouping, refresh fan-out with per-agent outcomes,
  and wake dispatch. Counts distinguish grouped, filtered, and returned rows;
  wake events identify the chosen agent and whether the route was explicit,
  without recording device identifiers or addresses.
- Connectivity uses short `agent_dns`, `agent_connect`, `agent_handshake_send`,
  and CC `agent_auth_verify` spans, rather than connection-long spans. Connect
  attempts also contribute to operation metrics. Lifecycle events report retries
  with backoff and CC disconnect duration. Sending credentials is not an auth ACK;
  CC's authentication event is the authoritative result. DNS diagnostics are a
  separate lookup, not a measurement of the resolver inside WebSocket connect.
- Terminal recovery uses `terminal_snapshot_generate` (encoded byte count),
  transport-attachment spans, and `terminal_snapshot_request`. These measure
  local work, not browser restoration or end-to-end image replay. Resume operation
  success means credentials were queued for the worker. `terminal_backpressure_wait`
  operation durations cover one full-queue backlog episode until it drains (`ok`)
  or disconnects (`disconnected`); cancellation can retain the default `error` outcome.

Instrument operation boundaries rather than individual rows or terminal chunks.
Use `skip_all`, explicitly allow count fields, and instrument async futures rather
than holding entered span guards across awaits. Do not record query arguments,
SQL parameters, device payloads, or credentials. Span names are fixed across requests.

Metrics are exported every 30 seconds and at orderly shutdown:

The table uses OpenTelemetry instrument names. Prometheus conversion typically
replaces dots with underscores and adds type/unit suffixes; histograms produce
multiple series. See the [metrics recipe book](telemetry-recipes.md) for discovery
queries, rates, averages, percentiles, and Grafana units.

| Metric | Meaning |
| --- | --- |
| `wakey.operations` | Completed command/snapshot attempts, labeled by operation and outcome |
| `wakey.operation.duration` | Corresponding duration histogram in seconds |
| `wakey.snapshot.devices` | Cumulative devices in snapshots successfully sent by the agent |
| `wakey.terminal.active` | Agent PTY workers currently alive |
| `wakey.terminal.input.bytes` | Input bytes received by the terminal relay |
| `wakey.terminal.output.bytes` | Output bytes read from the PTY, including while detached |
| `wakey.terminal.backpressure` | Relay queue-full encounters, not dropped output |

Metric labels do not include request/session IDs, MACs, or IPs. New spans use
`skip_all` and explicitly select safe fields; terminal bytes and credentials are
not attached. Profiles, host CPU/RSS metrics, connected-agent gauges, and export-loss
dashboards are not part of this initial slice.

## Failure policy

Trace and log queues are capped at 256 records each, with export batches of 64
and a 3-second request timeout. Export happens off the application path. On an
outage, telemetry can be dropped rather than accumulating unbounded router RAM;
terminal output follows its existing lossless backpressure policy independently.
The bound is a record count, not an absolute byte limit on arbitrary log messages.
Providers flush on orderly daemon exit (Ctrl-C/SIGTERM), on a blocking worker while
Tokio remains alive. Only completed spans are exported; this is not graceful draining
of every in-flight request. SIGKILL/power loss cannot flush. This is not a persistent
telemetry spool.

## Validation

```sh
cargo test -p wakey-core --features telemetry --test telemetry
```

The test starts a temporary gRPC collector and verifies all three signals,
command parent/child IDs, log correlation, and shutdown delivery without sleeps.
