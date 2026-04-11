## Plan: Audit, Alerts, UI, and Safe Edge Exposure

Recommended approach: ship in this order audit first, alerts second, UI third, then edge hardening and soak. This gives you observability truth before you build subscriptions and screens, and keeps risky control APIs private behind Cloudflare Access at Caddy.

**Steps**
1. Phase A: Lock trust boundaries and endpoint classes.
2. Public Agent API stays exposed: /api/v1/agents/enroll, /api/v1/agent/ws, /healthz.
3. Private Control API stays protected: /api/v1/control/* and /ui/*.
4. Phase A: Define AuditEvent schema and retention defaults.
5. Phase B: Add audit persistence in sled and emit at key points.
6. Emit audit on token issue/list/revoke, ws auth/disconnect, command dispatch/result/timeout/error, and reload/config operations.
7. Phase B: Add audit query API with filters and pagination.
8. Phase C: Implement deterministic alert rules and evaluator loop.
9. Start with offline agent threshold, timeout-rate threshold, auth-failure spike, token misuse attempts.
10. Add dedupe and cooldown so alerts do not flap.
11. Phase C: Add alert delivery APIs.
12. Poll-first endpoint for active alerts and recent transitions, websocket stream optional after rules stabilize.
13. Phase D: Build same-domain UI app shell at /ui with origin-relative API client.
14. Phase D: Build pages: Agent Health, Command Runner, Audit Timeline, Alerts Panel.
15. Phase E: Add Caddy deployment template with Cloudflare Access policy boundaries and websocket support.
16. Phase E: Run end-to-end drills and 48-72h soak.

**Relevant files**
- [wakey-control-plane/src/runtime/mod.rs](wakey-control-plane/src/runtime/mod.rs)
- [wakey-control-plane/src/api/commands.rs](wakey-control-plane/src/api/commands.rs)
- [wakey-control-plane/src/api/control.rs](wakey-control-plane/src/api/control.rs)
- [wakey-control-plane/src/ws.rs](wakey-control-plane/src/ws.rs)
- [wakey-control-plane/src/state/store.rs](wakey-control-plane/src/state/store.rs)
- [wakey-control-plane/src/state/types.rs](wakey-control-plane/src/state/types.rs)
- [wakey-control-plane/src/config/types.rs](wakey-control-plane/src/config/types.rs)
- [wakey-control-plane/src/config/resolve.rs](wakey-control-plane/src/config/resolve.rs)
- [wakey-control-plane/src/cli.rs](wakey-control-plane/src/cli.rs)
- [README.md](README.md)
- [scripts/init/openwrt/wakey](scripts/init/openwrt/wakey)
- [.github/plan-controlPlaneAppV1.prompt.md](.github/plan-controlPlaneAppV1.prompt.md)

**Verification**
1. Unit tests for audit append/query, pagination, retention pruning.
2. Unit tests for alert rule evaluation, dedupe, cooldown.
3. Contract tests for request_id correlation across command result and timeout/error audit records.
4. Integration tests for ws auth/disconnect audit events and command timeout event emission.
5. API tests for audit and alert endpoints.
6. Edge security tests that unauthenticated /api/v1/control/* and /ui/* are denied.
7. Soak tests for reconnect churn and audit growth stability.

**Decisions captured**
- Admin auth default: Cloudflare Access only, enforced at Caddy.
- UI host: same domain path deployment.
- Shell bridge: excluded from v1 due high risk and low break-glass value during hard router failures.
- Alert delivery: poll-first in v1, websocket stream optional.

**Caddy policy shape for this plan**
1. Route /api/v1/agents/enroll and /api/v1/agent/ws to control-plane upstream without Cloudflare Access gate.
2. Route /api/v1/control/* and /ui/* only when Cloudflare Access authentication is valid.
3. Preserve websocket upgrade headers on /api/v1/agent/ws.
4. Keep control-plane process bound to private interface or localhost behind Caddy.
5. Deny direct exposure of /api/v1/control/* from origin network paths.
