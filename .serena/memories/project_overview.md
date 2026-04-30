# Wakey Project Overview

Wakey is a Wake-on-LAN and LAN observability workspace for Linux/OpenWrt routers. It includes:
- `wakey-core`: shared model and parsing types for devices, neighbors, DHCP, interfaces, wake results.
- `wakey-linux`: Linux/OpenWrt adapter layer for DHCP lease files, neighbor lookup, interface data, WoL sending, and local observation storage.
- root `wakey` crate: service layer and operator CLI for inventory, leases, devs, wake, and observe.
- `wakey-agent`: outbound router daemon. Enrolls with control plane, maintains authenticated websocket, executes relayed commands, syncs observations.
- `wakey-control-plane`: Axum control plane, SQLx SQLite state, agent registry, command relay, known devices, observations, audit/alerts, fleet APIs, static UI hosting.
- `ipjs`: typed wrappers around Linux `ip -j` data.
- `ui`: React/Vite operator UI using shadcn/base-ui style components.

Current persistence direction: control plane uses SQLx SQLite with migrations in `wakey-control-plane/migrations` and offline query metadata in `.sqlx` plus `wakey-control-plane/.sqlx`. Legacy sled import exists as explicit command.

Current product direction: fleet-first device UI and backend. Known device identity is manual. Observations/inventory provide suggestions/current state but should not automatically promote or delete durable identifiers.