## Plan: Control Plane App v1

Build an end-to-end, ops-ready v1 over 6+ weeks by reusing current wakey and wakey-core logic, keeping wakey-agent as outbound executor, and adding a dedicated control-plane server plus minimal operator UI.

**Steps**
1. Phase 1, contract baseline: finalize relay contract for command, result, error, request correlation, timeout, retry, and forward compatibility behavior.
2. Phase 1, boundary lock: keep execution in wakey service functions and keep domain DTOs in wakey-core while removing legacy HTTP/static compatibility code.
3. Phase 2, server skeleton: implement enrollment endpoint, agent registry, websocket acceptor, and request correlation map. Depends on step 1 and step 2.
4. Phase 2, relay core: implement command submission to connected agents, request_id correlation, timeout paths, and structured relay errors. Depends on step 3.
5. Phase 2, persistence and identity: durable agent records, enroll token lifecycle, and safe credential metadata. Parallel with step 4 after schema is stable.
6. Phase 3, operator surface: add API for agent inventory, health, command execution, and recent outcomes; add minimal UI for core operations. Depends on step 4 and step 5.
7. Phase 3, ops hardening: metrics, logs, audits, heartbeat liveness checks, and alert thresholds. Parallel with step 6.
8. Phase 4, deployment pipeline: add server build and deploy artifacts, environment templates, and rollback workflow. Depends on step 6 and step 7.
9. Phase 4, validation and soak: run enrollment-to-command end-to-end tests and disconnect/failure drills with multi-day soak. Depends on step 8.

**Relevant files to reuse**
- [wakey-agent/src/protocol.rs](wakey-agent/src/protocol.rs)
- [wakey-agent/src/session.rs](wakey-agent/src/session.rs)
- [wakey-agent/src/dispatch.rs](wakey-agent/src/dispatch.rs)
- [src/service/mod.rs](src/service/mod.rs)
- [wakey-core/src/model](wakey-core/src/model)
- [scripts/package_rootfs.ps1](scripts/package_rootfs.ps1)
- [.gitea/workflows/release.yml](.gitea/workflows/release.yml)

**Verification**
1. Contract tests for command and result serialization, request_id stability, and unknown frame tolerance.
2. Relay integration tests for enrollment, websocket auth flow, correlation, and timeout handling.
3. Security tests for token lifecycle and invalid credential rejection.
4. API tests for registry and command execution behavior.
5. Observability tests for heartbeat, reconnect counters, latency, and failure alerts.
6. Deployment tests for build, release, rollback rehearsal, and staging smoke checks.
7. On-device tests for enroll, procd lifecycle, reconnect, and remote command round trips.