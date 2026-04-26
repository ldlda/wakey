# Plan: Observation Store And Sync V1

## Summary

Build device observation support in layers:

1. Local router observations are captured by `wakey observe ...` from OpenWrt hotplug.
2. `wakey-linux` stores current observed DHCP/neigh state locally.
3. `wakey-agent` forwards compact observations to the control plane.
4. The control plane stores observations per agent and later joins them to known devices through `device_identifiers`.

This is separate from durable identity. Observations are facts an agent saw at a time; known devices are manual/user-approved identity records.

## Network/API Boundary

The control plane has two route classes:

- Public agent routes: intended to be reachable by agents.
- Protected control routes: intended to sit behind Cloudflare Access/admin auth.

Observation upload belongs on the public agent route surface because routers/agents must call it:

```text
POST /api/v1/agents/observations
```

The endpoint must require normal agent authentication, using the same persistent `agent_id`/`agent_token` trust model as WebSocket auth. It must not be a Cloudflare Access-only admin endpoint.

Admin/UI read APIs belong under protected control routes:

```text
GET /api/v1/control/observations
GET /api/v1/control/devices
```

## Step 1: Local Observation Store

Replace the narrow `/tmp/wakey_mac_names.json` cache with a local observation store owned by `wakey-linux`.

Minimum local tables/state:

```text
observed_dhcp_clients:
  mac
  ip
  hostname
  first_seen_unix
  last_seen_unix
  last_action

observed_neighbors:
  key
  mac
  ip
  first_seen_unix
  last_seen_unix
  last_action
```

Behavior:

- Hotplug scripts stay minimal and call `wakey-agent observe ...`.
- `wakey-agent observe ...` delegates to `wakey observe ...`.
- `wakey observe dhcp ...` writes DHCP observations.
- `wakey observe neigh ...` writes neighbor observations.
- Existing `wakey leases` and `wakey inventory` still read live sources first, then enrich from the local observation store.
- Do not make local queries depend exclusively on hotplug events yet.

## Step 2: Control-Plane Observation Tables

Add observation tables early so future API/UI work does not need another major state refactor.

Control-plane schema:

```text
agent_device_observations:
  observation_key TEXT PRIMARY KEY
  agent_id TEXT NOT NULL
  kind TEXT NOT NULL
  mac TEXT
  ip TEXT
  hostname TEXT
  first_seen_unix INTEGER NOT NULL
  last_seen_unix INTEGER NOT NULL
  last_action TEXT NOT NULL

agent_device_observation_events:
  event_id TEXT PRIMARY KEY
  agent_id TEXT NOT NULL
  kind TEXT NOT NULL
  action TEXT NOT NULL
  mac TEXT
  ip TEXT
  hostname TEXT
  ts_unix INTEGER NOT NULL
```

Keep `agent_device_observation_events` optional in behavior if needed, but create it in schema early. Current-state rows are the primary path; event history is for debugging/audit.

Indexes:

```text
agent_device_observations(agent_id)
agent_device_observations(mac)
agent_device_observations(ip)
agent_device_observations(hostname)
agent_device_observations(last_seen_unix)
agent_device_observation_events(agent_id, ts_unix)
agent_device_observation_events(mac)
```

## Step 3: Agent Upload

Add agent upload payload:

```json
{
  "observations": [
    {
      "kind": "dhcp",
      "action": "update",
      "mac": "04:7c:16:79:6d:ee",
      "ip": "192.168.100.94",
      "hostname": "lda",
      "first_seen_unix": 1770000000,
      "last_seen_unix": 1770000123
    }
  ]
}
```

Rules:

- Agent may send snapshots periodically and after local observe events.
- Control plane upserts current-state rows by stable observation key.
- Observation keys should be deterministic from `agent_id`, `kind`, and the best available identifier:
  - DHCP with MAC: `agent:{agent_id}:dhcp:mac:{mac}`
  - Neigh with MAC: `agent:{agent_id}:neigh:mac:{mac}`
  - Neigh without MAC: `agent:{agent_id}:neigh:ip:{ip}`
- Uploads must not create known devices automatically.

## Step 4: Join Observations To Known Devices

Known devices already have durable IDs and manual identifiers:

```text
known_devices
device_identifiers
```

Join rule:

```text
agent_device_observations.mac
  -> device_identifiers(kind = 'mac', value = mac)
  -> known_devices.device_id
```

Unknown observations are rows that do not match any manual `device_identifiers` row.

This enables:

- same known device observed by multiple agents;
- same known device with multiple MACs;
- UI flow to attach an unknown observed MAC to an existing known device;
- wake flows that choose agent-local observed IP/MAC context for a known device.

## Step 5: UI/API Later

After storage and upload exist:

- show known devices with all matching observations grouped by agent;
- show unknown observations;
- add action: attach observation identifier to known device;
- add action: create known device from observation;
- add wake action from known device using a selected agent/observation.

## Defaults

- `wakey` remains the local router/debugging CLI.
- `wakey-agent` is the sync bridge to the control plane.
- `wakey-control-plane` is durable identity and multi-agent view.
- Local observation store is not authoritative; live sources still matter.
- Control-plane observations are not durable identity; only manual known-device identifiers are.
