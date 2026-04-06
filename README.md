# wakey

`wakey` is a Wake-on-LAN and LAN-observability tool for a Linux router.

It started as a small web UI running on-device. It is now being reshaped into a
service-first project with:

- a reusable core model
- a Linux/OpenWrt adapter layer
- a CLI for operators
- a temporary legacy HTTP/static adapter during migration

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
wakey http --host :: --port 12012
```

## Workspace layout

- `wakey-core`
  - shared types and parsing
  - device, neighbor, DHCP, interface, and wake models
- `wakey-linux`
  - Linux/OpenWrt adapter
  - DHCP lease loading, interface summaries, neighbor lookup, WoL sending
- `wakey`
  - service layer, CLI, and temporary HTTP/static adapter
- `ipjs`
  - typed wrappers around Linux `ip -j ...` data
  - JSON-first, with optional experimental netlink backends

## Current architecture

Inside the `wakey` crate:

- `src/service`
  - the real use-case layer
  - status, leases, inventory, interfaces, wake, and query resolution
- `src/http`
  - temporary legacy HTTP/static adapter
  - compatibility mapping for the current `/static` client
- `src/legacy`
  - transitional compatibility wrappers kept during the migration

The long-term direction is:

- keep the service layer stable
- keep HTTP as an adapter, not the architecture
- eventually move toward an agent + control-plane model

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

### Temporary HTTP adapter

The old web/static app can still be served during migration:

```sh
wakey http --host :: --port 12012
```

This should be treated as a compatibility surface, not the long-term product
shape.

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
