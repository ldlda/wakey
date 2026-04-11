## Plan: Operator UI v1 (Flexible Delivery)

Ship a practical, operator-first UI quickly using existing control-plane APIs, while keeping implementation choices flexible where they do not affect UX outcomes. Prioritize clear workflows, reliable live state, and low-friction deployment at /ui.

**Steps**
1. Phase 1: Experience goals and page IA. Define primary workflows and success criteria before coding: monitor fleet health, run commands safely, inspect audit history, manage alerts, and handle enrollment tokens.
2. Phase 1: Information architecture. Create a minimal nav with Dashboard, Agents, Commands, Audit, Alerts, and Tokens; keep room to merge/split pages later based on usage.
3. Phase 1: Data contracts and client boundaries. Build a typed API client around existing endpoints with origin-relative URLs only; centralize request/retry/error normalization and leave room to swap transport helpers.
4. Phase 2: Core shell and states. Implement app shell, loading/empty/error states, top-level notifications, and shared primitives (table/list/cards/filter panel) without prematurely freezing visual details.
5. Phase 2: Dashboard and Agents first. Surface connected/offline agent state and active alert counts, with quick navigation into command and investigation workflows.
6. Phase 2: Command Runner. Implement safe command form (status/devs/leases/inventory/wake), result rendering, request_id visibility, and copy/share affordances for incident collaboration.
7. Phase 3: Audit timeline. Implement filterable audit feed (agent_id, event_type, outcome, time window) with metadata drill-down and links back to related command outcomes.
8. Phase 3: Alerts center. Implement active alerts plus transition history, with live subscription via alerts websocket and automatic polling fallback if stream drops.
9. Phase 3: Token operations. Implement issue/list/revoke flows with expiry visibility and clear destructive-action confirmation UX.
10. Phase 4: UX polish and operator ergonomics. Add keyboard-friendly flow, persisted local filters, robust reconnect indicators, and compact/high-density views for on-call usage.
11. Phase 4: Deployment integration. Build static UI artifact into release pipeline and serve at /ui behind Cloudflare Access with the existing edge policy.
12. Phase 5: Validation and soak. Run realistic operator drills and prolonged soak to tune alert noise, UI refresh cadence, and failure handling.

**Relevant files**
- [wakey-control-plane/src/runtime/mod.rs](wakey-control-plane/src/runtime/mod.rs) — route integration point for serving /ui and preserving public/control boundary.
- [wakey-control-plane/src/api/commands.rs](wakey-control-plane/src/api/commands.rs) — command runner response contract.
- [wakey-control-plane/src/api/audit.rs](wakey-control-plane/src/api/audit.rs) — audit query/filter contract.
- [wakey-control-plane/src/api/alerts.rs](wakey-control-plane/src/api/alerts.rs) — active alerts, transition history, and websocket stream contracts.
- [wakey-control-plane/src/api/control.rs](wakey-control-plane/src/api/control.rs) — token-management contracts.
- [deploy/Caddyfile.control-plane.example](deploy/Caddyfile.control-plane.example) — edge gating and /ui exposure model.
- [scripts/package_rootfs.ps1](scripts/package_rootfs.ps1) — package integration for UI artifact.
- [.gitea/workflows/release.yml](.gitea/workflows/release.yml) — CI build and release integration.
- [README.md](README.md) — operator-facing UI and API usage docs.

**Verification**
1. UI build verification in CI (typecheck, lint, production build) and artifact presence checks.
2. Page-level smoke tests: Dashboard, Agents, Commands, Audit, Alerts, Tokens all load and complete primary actions.
3. Contract checks between typed client models and control-plane payloads for commands/audit/alerts.
4. Live-state checks: websocket stream updates alerts; fallback polling activates on disconnect and recovers gracefully.
5. Security checks: /ui and /api/v1/control/* remain blocked without Access policy; public agent endpoints remain reachable.
6. Soak checks: multi-hour sessions preserve responsiveness and do not lose transitions during reconnect churn.

**Decisions**
- Keep major architecture fixed: same-domain /ui, origin-relative API client, edge-enforced admin access.
- Keep minor implementation details flexible: exact component library, state library, and styling primitives can change if DX improves.
- Optimize for operator speed over visual novelty: dense information, low click depth, and fast command feedback.
- Treat websocket as enhancement, not dependency: polling fallback is mandatory.

**Further Considerations**
1. Short-term backend enhancements likely to improve UX quickly: agent metadata labels, audit cursor pagination, persisted alert-rule defaults.
2. If team preference changes, frontend framework can be swapped as long as route/contracts/deploy shape stays the same.
3. Add a lightweight investigation-mode preset in UI that pivots from alert to related audits to related command request_id in one flow.
