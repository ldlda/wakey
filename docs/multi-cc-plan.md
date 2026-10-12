# Multi-CC Identity And Configuration

Status: proposal, 2026-10-12. This is not an implemented config format.

## Goal

One agent process can serve production and test CCs simultaneously. Collect
locally once, forward to independent connections, and show human names alongside
IDs. A failed CC must not block another or kill local terminal processes.

## Identity And Names

- **CC ID:** stable identity of a logical control plane, independent of URL,
  hostname, display name and WebSocket connection. Renaming or moving it must
  not require agents to enroll again.
- **CC name:** configured on CC and advertised to authenticated agents. Start
  with config-based rename plus reload/restart; no rename API required initially.
- **Agent instance ID/name:** local daemon identity and reported human name,
  shared across its connections. Not the enrollment identity of every CC.
- **Enrollment:** each CC issues its own `agent_id` and token. Never send one
  CC's credentials to another. Existing per-CC agent nicknames override the
  reported name in that CC's UI; names never become routing/authentication keys.
- **Connection alias:** local config key such as `cloud` or `test`, useful for
  CLI selection and pre-auth logs. It is not the remote CC ID or authoritative
  remote name.

CC ID is generated once and retained in SQLite metadata when not configured;
an explicit config ID must agree with an existing stored identity, not silently
replace it. New init-config output can include a generated ID. Agent instance
ID belongs in its local config. Copying either deployment for an independent
test instance requires a distinct identity; do not mint a new ID each startup.

## Proposed Config Shape

CC keeps existing operational settings and adds:

```toml
[identity]
id = "cc-<uuid>"
name = "Cloud"
```

Agent separates local identity/settings from per-CC enrollment:

```toml
[identity]
id = "node-<uuid>"
name = "Router"

[local]
pid_file = "/var/run/wakey-agent.pid"
dhcp_leases_path = "/tmp/dhcp.leases"
mac_name_cache_path = "/tmp/wakey_mac_names.json"
observation_store_path = "/tmp/wakey_observations.json"

[sync]
interval = "60s"
hook_retention = "7d"

[reconnect]
base = "1s"
max = "30s"

[control_planes.cloud]
server_url = "https://wakey.example.com"
agent_id = "agent-<cloud-issued-id>"
agent_token = "<cloud-token>"
# expected_cc_id = "cc-<uuid>"

[control_planes.test]
server_url = "http://127.0.0.1:8080"
agent_id = "agent-<test-issued-id>"
agent_token = "<test-token>"
```

Existing `[terminal]` and `[telemetry]` remain daemon-wide. Reconnect defaults
apply independently to each connection; avoid per-CC copies of every setting.
The field names above are proposed, not a reason to rename unrelated CC settings.

Keep accepting the legacy single-CC root fields as one `default` connection,
with a deprecation warning. Reject mixed legacy/new connection declarations
rather than guessing. Init-config emits the new shape; `--from-config` preserves
all connections and their credentials. Enrollment targets a connection alias:
one connection can be implicit, multiple connections require explicit selection.

## Connection And Forwarding Rules

- Add negotiated, post-auth CC identity/name metadata. Current protocol has
  Hello/Auth but no explicit successful-auth response; sending credentials is
  not proof of authentication. Old clients must not receive unsupported enum
  variants unconditionally. Keep legacy single-CC compatibility explicit.
- Forward the agent's configured name during hello; CC sends its own name/ID
  after auth. Show `name (id)` or a short ID in operator/debug views. Retain the
  local alias and last reported name for disconnected status, marking it offline.
- Each CC gets its own worker, retry state, bounded outbound queue and request
  replies. Reject duplicate authenticated connections to the same CC ID; changed
  expected IDs require an explicit operator decision, not silent rebinding.
- CC ID is identification, not a substitute for TLS/server authentication.
- Share local inventory/hook storage and periodic collection. Fan out complete
  snapshots under each CC's enrollment ID; a slow connection cannot hold the
  collector hostage. This does not turn snapshots into a durable event journal.
- Requests/replies and their trace contexts stay scoped to their connection.
  Never broadcast command results or terminal bytes to every CC.
- Saved devices, nicknames, permissions and history remain independent on each
  CC. No CC-to-CC synchronization or automatic identity promotion.

## Terminal And Telemetry Scope

The agent owns PTYs and one daemon-wide session registry. Session limits apply
globally, not once per connection. Initially, sessions belong to their creating
CC: another CC cannot attach, resume or close them. Attachment ownership is
enforced on the agent, not just inferred from one CC's registry. Disconnecting
one CC preserves its PTYs until existing TTL/explicit-close/agent-exit policy acts.
Reconnecting the same CC ID can reconcile its own sessions; removing a configured
CC must have an explicit terminal cleanup policy before implementation ships.

Use local agent instance ID and CC ID for `service.instance.id`, addressing the
current unspecified CC instance. Add configured display names as resource
metadata, not a replacement for stable IDs. Grafana presentation/label mapping
needs validation; do not promise names will automatically become `instance`.
CPU/RSS/host metrics describe one local process and must not be duplicated per CC.
Document whether snapshot counters count collections or per-CC sends before fan-out.

## Implementation Order

1. Stable CC/local-agent identities, names, telemetry metadata and config adapters.
2. Negotiated authenticated metadata exchange; keep the one-CC path working.
3. Shared local managers plus isolated connection workers and snapshot fan-out.
4. Global terminal ownership checks and multi-CC operator/debug status.

Acceptance tests must cover config/enrollment isolation, names changing without
IDs changing, one CC failing while another proceeds, shared collection without
cross-CC replies, duplicate CC identity rejection, and cross-CC terminal access
rejection. Use two test CCs and one agent before deploying this on the router.
