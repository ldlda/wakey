# Wakey Checkpoint (2026-04-12)

## Snapshot

This checkpoint captures progress after adding audit persistence, alert evaluation and transitions, control-plane alert APIs, and websocket timing diagnostics across agent and control-plane.

## Major Changes Landed

- Control-plane audit system implemented with persistent sled-backed events.
- Audit emission wired into:
  - enroll accept/reject
  - token issue/list/revoke
  - command dispatch/result/error/timeout
  - websocket auth accept/reject and disconnect
- Audit query API added:
  - `GET /api/v1/control/audit/events`
- Active alert engine added with deterministic rules over audit + live session state.
- Alert APIs added:
  - `GET /api/v1/control/alerts`
  - `GET /api/v1/control/alerts/history`
  - `GET /api/v1/control/alerts/ws`
- Alert transition persistence added (open/resolve transitions tracked across evaluations).
- Route classes split explicitly in runtime:
  - public routes (enroll/ws/health)
  - control routes (`/api/v1/control/*`)
- Caddy template added for edge policy and Cloudflare Access boundary:
  - `deploy/Caddyfile.control-plane.example`

## Reliability and Diagnostics Improvements

- Agent websocket connect diagnostics now include:
  - DNS resolution timing (`dns_resolve_ms`)
  - websocket connect timing (`ws_connect_ms`)
- Control-plane websocket lifecycle logs include:
  - connect-to-hello timing
  - connect-to-auth timing
  - hello-to-auth timing
- Slow connect warnings are now emitted when timing thresholds are exceeded.

## Root-Cause Findings Captured

- Long agent websocket connect delays were reproduced and traced to hostname resolution path.
- Switching agent `server_url` hostname to direct IP made connect immediate.
- This confirms app-level relay logic was not the source of the startup delay.

## Verification Status

- `cargo check --workspace` passing after all changes.
- Added and passing tests include:
  - audit event append/filter in state store
  - alert transition open/resolve persistence
  - alert evaluator rule checks (offline + timeout, auth/enroll rejection spikes)

## Current API Surface for UI Start

- Agents and command execution:
  - `GET /api/v1/control/agents`
  - `POST /api/v1/control/agents/{agent_id}/command`
- Audits:
  - `GET /api/v1/control/audit/events`
- Alerts:
  - `GET /api/v1/control/alerts`
  - `GET /api/v1/control/alerts/history`
  - websocket subscribe: `GET /api/v1/control/alerts/ws`

## Remaining Plan Items (Most Significant)

1. UI implementation (`/ui` app shell and pages) is still open.
2. Alert dedupe/cooldown persistence and tuning are still basic and need hardening.
3. Audit retention pruning policy and long-run storage controls are not finalized.
4. Edge auth enforcement tests and deployment rehearsals remain to be added.
5. Multi-day soak drills and failure-injection validation remain open.

## Suggested Next Actions

1. Build minimal UI shell with three views:
   - agents/commands
   - audit timeline
   - alerts panel (active + history + websocket stream)
2. Add periodic retention task for audit and alert transition trees.
3. Add proxy-level integration tests that assert private endpoints are blocked without Access headers.
4. Run a 48-72h soak with hostname vs IP connect-path metrics collected.
