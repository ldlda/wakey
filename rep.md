# Report: Device State Rewrite

## What changed

The control plane now stores **complete per-agent Device snapshots** instead of flat observation rows. The agent sends `Vec<Device>` (merged from DHCP, neighbors, hooks). The control plane upserts per agent, deletes stale, fleet view merges across agents at read time.

## Architecture (before → after)

**Before**: Agent → raw hook observations → control plane flattens into rows → re-merges at fleet read → presence recomputed from flat actions

**After**: Agent → inventory → `Vec<Device>` with correct presence → control plane stores as-is → fleet merge at read time from typed rows

## Schema

Old tables dropped: `agent_device_observations`, `agent_device_observation_events`, `agent_observation_snapshots`, `agent_observation_snapshot_keys`

New tables:
- `agent_devices` — per-agent device row (keyed by `DeviceId` serialization)
- `agent_device_macs` — MACs per device, indexed for cross-agent merge
- `agent_device_ips` — IPs per device, indexed for cross-agent merge
- `agent_device_hostnames` — hostnames per device
- `agent_device_facts` — debug/source material (JSON, not identity)

## Concrete types throughout

- `FleetDevice.macs`: `Vec<MacAddr>`, `ips`: `Vec<IpAddr>`, `presence`: `Presence`
- `FleetWakeRoute.mac`: `Option<MacAddr>`, `ip`: `Option<IpAddr>`
- `AgentDeviceWithChildren.macs`: `Vec<MacAddr>`, `ips`: `Vec<IpAddr>`
- Row types (`AgentDeviceMacRow`, `AgentDeviceIpRow`) stay `String` from SQLite, convert via `TryFrom` impls
- `AgentDeviceRow` has `.presence()`, `.first_seen()`, `.last_seen()` methods — no string matching in business code
- `Presence` from wakey-core: `Ord`, `PartialOrd`, `From<&str>`, `.as_str()`

## Rust features used

- `TryFrom<&AgentDeviceMacRow> for MacAddr`, `TryFrom<&AgentDeviceIpRow> for IpAddr`
- `From<&str> for Presence`
- `#[allow(dead_code)]` where appropriate (test-only methods, debug fields)
- Workspace dependencies for shared crates (`macaddr`, `serde`, `tokio`, etc.)
- Shared `TestStore` with `Drop` guard for clean test teardown

## What was removed

- `AgentObservation` struct (replaced by `Device`)
- `AgentDeviceObservation`, `AgentDeviceObservationView`, `AgentDeviceObservationEvent`, `AgentDeviceObservationInput` types
- `inventory_result_to_observations()` — the flatten-then-rebuild function
- `upload_agent_observations`, `list_agent_observations`, `list_agent_observation_history` API endpoints
- `gc_stale_observations()` — no longer needed
- `SyncObservations` CLI command from agent
- `prune_removed_observations_from_path` calls from agent session
- Observations page from UI

## What stayed

- Agent observation store (`/tmp/wakey_observations.json`) — agent-local memory, not forwarded
- Hotplug hooks — still update agent-local store, feed next inventory
- Known devices + identifiers — manual lifecycle, never auto-deleted
- Audit events, alerts — unchanged
- `observation_retention` config field — `#[allow(dead_code)]`, kept for config file compat

## Verification

```
cargo fmt         — clean
cargo clippy-all  — clean (0 warnings)
cargo test-all    — 89 tests pass, 6 ignored (on-device only)
pnpm typecheck    — clean
```

## Remaining

- UI cleanup: remove observations page nav entry, observation API types from `api.ts`
- Known device identifier UI controls (API exists, UI needs affordance)
- The `observation_retention` config field could be removed in a future cleanup
- `list_agent_device_rows_for_agent` is `#[allow(dead_code)]` — used in tests, could be `#[cfg(test)]`
