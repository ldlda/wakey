# Style And Conventions

Rust:
- Edition 2024.
- Prefer explicit, small service/API functions over large abstractions.
- Use `anyhow::Context` for fallible operations in application layers.
- Use `tracing` for logs.
- Shared domain types live in `wakey-core`; Linux/OpenWrt I/O in `wakey-linux`; root crate owns service/CLI behavior; agent/control-plane should reuse service/domain types where possible.
- SQLx query macros are used in control-plane state. Regenerate `.sqlx` metadata after query string changes.
- Device identity is manual: observations and inventory may suggest, but should not silently pin, merge, promote, or delete known identifiers.
- Hotplug observation actions should stay event-shaped (`add`, `update`, `remove`). Do not store NUD state as the action. NUD belongs to live neighbor/inventory data.

Frontend:
- React + Vite + TypeScript.
- Use existing component/style conventions; shadcn/base-ui/lucide are available.
- Main operator UI is fleet-first: common path is find device, inspect current IP/MAC/agent, wake, copy, remember/merge.
- Admin/debug pages stay available but should not dominate the primary nav.

Editing:
- Keep changes scoped.
- Preserve user edits in dirty worktrees.
- Use `apply_patch` for manual file edits.
- Default to ASCII unless files already use non-ASCII for a reason.