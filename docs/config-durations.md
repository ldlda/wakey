# Configuration Durations

Agent and control-plane TOML duration settings use strings with explicit units.
Runtime code receives `std::time::Duration`; API and WebSocket numeric fields
such as `timeout_ms`, `session_ttl_seconds`, and `latency_ms` are unchanged.

## Agent

```toml
reconnect_base = "1s"
reconnect_max = "30s"
observation_sync_interval = "60s"
observation_retention = "7d"

[terminal]
session_ttl = "12h"
```

`observation_retention` prunes the agent's local hotplug memory during inventory
collection. It does not delete the control plane's last snapshot for an agent.
`"0d"` disables this local pruning. `terminal.session_ttl = "0s"` disables PTY
expiry. Reconnect and sync timers must be positive and fit the platform timer.
Terminal TTL and hook retention must resolve to whole seconds because their
protocol/storage timestamps use seconds. Reconnect and sync intervals can use
subsecond values.
Reconnect scheduling retains its existing 100ms minimum backoff.

## Control Plane

```toml
command_timeout = "30s"
enroll_token_ttl = "1d"
history_retention = "30d"
device_retention = "60d"
```

`command_timeout`, `enroll_token_ttl`, and `history_retention` must be positive.
Enrollment TTL and history retention must resolve to whole seconds. Command
timeout can use subsecond values.
`history_retention` deletes old audit events and alert transitions, not device
snapshots or saved identities.

`device_retention` expires agent-collected device rows and their child endpoints,
addresses, names and facts. The default is `"60d"`; `"0d"` disables expiry.
It must resolve to whole seconds. Age is measured from the last CC-received
snapshot that included the device, not endpoint timestamps or online presence.
An offline device included in new snapshots therefore stays retained. Saved
devices and their manually attached identifiers are never deleted by this cleanup.
Complete snapshots still remove devices omitted by that agent immediately;
retention handles old rows left behind when an agent stops syncing.
Expired rows are removed by the Unix daemon's existing maintenance loop every
five minutes, including an initial tick at startup. Restart CC to apply changes
to this setting; the state-only reload does not reload configuration.

## Legacy Files

| Old Key | New Key | Numeric Legacy Unit |
| --- | --- | --- |
| Agent `reconnect_base_ms` | `reconnect_base` | milliseconds |
| Agent `reconnect_max_ms` | `reconnect_max` | milliseconds |
| Agent `observation_sync_interval_seconds` | `observation_sync_interval` | seconds |
| Agent `observation_retention_days` | `observation_retention` | days |
| Agent `terminal.session_ttl_seconds` | `terminal.session_ttl` | seconds |
| CC `command_timeout_ms` | `command_timeout` | milliseconds |
| CC `enroll_token_ttl_seconds` | `enroll_token_ttl` | seconds |
| CC `observation_retention_seconds` | `history_retention` | seconds |

Old keys accept non-negative integers in their original units, or explicit
duration strings, and produce a warning on stderr when loaded. New keys require
strings; bare numbers are rejected. Supplying both keys for the same setting is
an error, even when their values agree. Negative, malformed, and overflowing
values are rejected; zero active timers are rejected rather than silently
clamped. No secrets are included in deprecation warnings.

`init-config --from-config ...` reads legacy files and writes only canonical
keys and formatted duration strings. Existing numeric CLI flags are unchanged
in this slice, and their overrides also serialize in the new format.

For example, legacy `reconnect_base_ms = 1500` is read as 1500 milliseconds
and written back as `reconnect_base = "1s 500ms"`. New-key
`reconnect_base = 1500` is rejected: no implicit numeric unit is assigned.
Serialization always writes duration strings, regardless of the accepted input
format, using `humantime-serde`.

The parser is [humantime](https://docs.rs/humantime/latest/humantime/fn.parse_duration.html).
Examples include `"14d"`, `"168h"`, `"250ms"`, and `"1h 30m"`. These are elapsed
durations, not calendar scheduling rules; use days/hours for retention rather
than assuming calendar-month semantics.
