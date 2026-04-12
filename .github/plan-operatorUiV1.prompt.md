## Plan: wakey UI v1 (Device-First)

The Operator UI is not the end goal by itself.
The goal is to make wakey excellent at its original purpose: quickly finding devices and waking them reliably.

Agent, audit, and token features remain important, but they should support the device workflow rather than dominate the navigation and development effort.

## Product North Star

An operator should be able to do this in under 10 seconds:
1. Open the UI.
2. Search for a device by name, IP, or MAC.
3. See whether it looks online/reachable.
4. Trigger wake.
5. See immediate command result and short follow-up status.

## Scope Priorities

1. P0: Device discovery and wake UX.
2. P1: Fast troubleshooting context around wake results.
3. P2: Fleet/agent/admin operations.

This explicitly means pages and components for Agent, Audit, Alerts, and Tokens should be present but secondary in visual hierarchy and effort until P0 is complete.

## IA Direction (v1)

Primary top-level focus:
1. Devices
2. Wake Queue (or Recent Actions)

Secondary top-level focus:
1. Fleet Health
2. Audit
3. Alerts
4. Access/Tokens

If needed, keep current routes during transition, but adjust default landing and navigation emphasis so Devices is the home workflow.

## Phase Plan

### Phase 1: Device-Centric Foundation

1. Add a dedicated Devices page that merges existing status/leases/inventory signal into one operator list.
2. Include searchable columns for name, IP, MAC, interface/dev, and recency indicators.
3. Provide row-level wake action and bulk-safe interaction model (single-click row action, confirm for bulk).
4. Define a compact "device confidence" heuristic from available data (for example: recent lease + reachable neighbor).
5. Set the default route to Devices, with prominent search and wake controls above the fold.

Acceptance criteria:
1. Search by hostname, IP, and MAC all work from one input.
2. Wake action reachable in one click from list row.
3. Response feedback shown immediately with clear success/error text.

### Phase 2: Wake Execution UX

1. Build a focused wake panel with explicit target preview before send.
2. Provide quick presets: "wake by selected device", "wake by MAC", "wake by query".
3. Persist recent wake targets locally for operator speed.
4. Add post-wake verification loop (short timed refresh of status indicators).
5. Surface request correlation id and a copy action for incident sharing.

Acceptance criteria:
1. Operator can retry wake with one click.
2. Operator can see last 20 wake attempts with outcome and timestamp.
3. Error states differentiate validation, timeout, and execution failure.

### Phase 3: Context Without Workflow Drift

1. Keep Alerts and Audit accessible from device rows and wake outcomes.
2. Add contextual deep links: device -> related alerts, device -> recent audit events.
3. Improve filtering for alerts/audit with saved local filter presets.
4. Add gentle live updates, keeping websocket optional with polling fallback.

Acceptance criteria:
1. From any failed wake, operator can jump to relevant audit entries in one step.
2. Alerts page can be filtered by kind/severity/agent and linked back to impacted devices.

### Phase 4: Fleet/Admin Hardening

1. Keep Agents page for connectivity and control routing visibility.
2. Keep Tokens page for enrollment lifecycle operations.
3. Keep Dashboard but reframe metrics around "device availability" and "wake success" first.
4. Add audit-friendly confirmation flows for destructive actions.

Acceptance criteria:
1. Admin flows do not block or slow P0 device workflows.
2. All admin actions produce clear audit-visible outcomes.

### Phase 5: Validation and Production Readiness

1. Add smoke tests for P0 flow: find device -> wake -> observe result.
2. Add contract checks for command payload/response shapes used by device and wake screens.
3. Add scenario drills (offline agent, delayed command, websocket drop, expired token).
4. Verify edge policy still protects control routes while preserving required public endpoints.

Acceptance criteria:
1. P0 flow remains usable during partial degradation.
2. Build/typecheck/test gates stay green in CI.

## Concrete UI Backlog (Ordered)

1. Create DevicesPage with unified searchable table.
2. Wire wake action directly from device row.
3. Add Recent Wake Actions panel with outcomes.
4. Add post-wake short verification refresh.
5. Rework Dashboard cards to device-first metrics.
6. Add deep links from wake result to audit and alerts context.
7. Add empty/loading/error skeleton patterns tuned for device list scale.

## Metrics of Success

1. Time-to-wake median under 10 seconds for known target.
2. Wake success rate visible per time window.
3. Fewer operator clicks for common tasks (device lookup + wake).
4. Reduced navigation to agent-centric pages for everyday usage.

## Design and Interaction Principles

1. Device-first information density over generic admin dashboards.
2. Search and action bar always visible on desktop and mobile.
3. Action feedback must be immediate and explicit.
4. Keep advanced infrastructure details available but visually de-emphasized.

## Architecture Decisions

1. Keep same-domain /ui serving and origin-relative API requests.
2. Keep route and API contracts stable while iterating UX aggressively.
3. Treat websocket as progressive enhancement; polling fallback required.
4. Preserve Cloudflare Access boundary for all control endpoints.

## Relevant Files

1. [ui/src/App.tsx](ui/src/App.tsx)
2. [ui/src/pages/CommandsPage.tsx](ui/src/pages/CommandsPage.tsx)
3. [ui/src/pages/DashboardPage.tsx](ui/src/pages/DashboardPage.tsx)
4. [ui/src/api.ts](ui/src/api.ts)
5. [wakey-control-plane/src/api/commands.rs](wakey-control-plane/src/api/commands.rs)
6. [wakey-control-plane/src/api/audit.rs](wakey-control-plane/src/api/audit.rs)
7. [wakey-control-plane/src/api/alerts.rs](wakey-control-plane/src/api/alerts.rs)
8. [wakey-control-plane/src/api/control.rs](wakey-control-plane/src/api/control.rs)
9. [wakey-control-plane/src/runtime/mod.rs](wakey-control-plane/src/runtime/mod.rs)
10. [README.md](README.md)

## Immediate Next Build Slice

1. Implement DevicesPage and make it default route.
2. Add row-level wake action with in-place result feedback.
3. Add Recent Wake Actions section backed by local state first, then audit correlation.
4. Rebalance navigation labels/order to make device workflows primary.
