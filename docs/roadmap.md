# Wakey Roadmap

Last updated: 2026-10-10. This records agreed work and follow-ups, not a release
promise. Checked items are implemented; live validation is called out separately.

## Completed Observability

- [x] Shared optional OTLP setup for traces, logs, and metrics in wakey-core.
- [x] Basic/Bearer authorization supplied through a named environment variable.
- [x] OpenWrt UCI configuration and systemd environment-file support, with setup guides.
- [x] Cross-process command trace context; verified live on a CC-to-agent wake.
- [x] Inventory source, merge, device-storage, and fleet-operation spans.
- [x] Agent connection phase spans and retry events, without entered guards across awaits.
- [x] Terminal snapshot generation, transport attachment, resume, and backpressure instrumentation.
- [x] Separate hostname-resolution and neighbor-table spans; a live slow wake spent about four seconds in resolution.
- [x] Metrics recipes for rates, duration averages/percentiles, terminal activity, and snapshots.
- [x] Authenticated live logs, metrics, and traces confirmed through Grafana's read-only API.

See [telemetry setup](telemetry.md), [metrics recipes](telemetry-recipes.md), and
[OpenWrt UCI](openwrt-uci.md). Terminal instrumentation has automated coverage;
live browser recovery and shutdown behavior are not established by those checks.

## Device Management And Presentation

- [x] Make **forget remembered device** reachable in the frontend, with confirmation. The existing backend removes the known identity and its identifiers, not agent observations. An observed device may reappear as unknown.
- [x] Add remembered-device rename support, with backend validation and frontend editing (`PATCH /api/v1/control/devices/{device_id}`). Pinned state and identifiers are preserved.
- [ ] Check and complete editing of remembered-device notes.
- [x] Replace generic unnamed-row labels with useful fallbacks. Offline IP-only rows: `Offline device: <ip>`; unknown-presence IP-only rows: `Device: <ip>`. Use MAC fallback when no IP exists; retain a generic label only when neither exists. Inactive endpoint addresses can label a row without becoming active IPs or wake routes. Unknown-only fleet groups remain unknown.
- [ ] Review frontend visibility defaults. Keep an explicit all-rows option; do not hide evidence just to fix poor labels. The user deliberately enabled all-row querying.

Identifier detachment already exists in the current frontend source; it is **not**
the requested whole-device removal. No automatic identity promotion or deletion
of remembered identifiers based on observation expiry.

## Telemetry Follow-Ups

- [x] Separate console, OTLP log, and trace filters using per-layer filtering. Explicit signal config overrides `RUST_LOG`, then CLI verbosity supplies defaults. Unconfigured defaults remain info-level; exporter-failure logs stay local. Live validation pending.
- [ ] Record hostname-resolution success/failure without exporting hostnames or resolved addresses. Current spans show timing but do not explicitly classify the result.
- [ ] Investigate the slow resolver with a reproducible device-side check. Timing isolated the phase, not the cause; do not treat instrumentation as a fix.
- [ ] Propagate trace context for periodic snapshots so agent collection and CC persistence can be inspected together. Commands already have this linkage.
- [ ] Add current-state device gauges: stored rows per agent and grouped fleet devices split known/unknown. Define scope and update lifecycle first; never derive gauges from the last filtered UI query.
- [ ] Decide whether snapshots need persistence acknowledgements. The existing sent-device counter does not prove CC storage; any confirmed-storage metric needs an authoritative measurement point.

The existing snapshot counter is cumulative collection volume, not unique devices
or current fleet size. Metric labels should remain bounded; no per-device labels.

## Validation And Reliability

- [ ] Reproduce/check Ctrl-C cleanup on the test device: process groups, PTYs, relay tasks, and telemetry shutdown. Leftovers were suspected, not confirmed.
- [ ] Exercise collector unavailability and recovery with running agent/CC instances. Verify normal operations continue and telemetry queues remain bounded; outage losses are expected under the current best-effort policy.
- [ ] Validate terminal recovery instrumentation live: generation timing/size, transport attach/resume, and backpressure episodes ending in drain or disconnect.
- [ ] Validate the revised UCI credential prompt and service environment on the live router.

## Watch, Not Yet Fix

- Fleet listing time is mostly device-store reads in observed traces. Measure
  sustained latency and row counts before changing queries or indexes.
- Snapshot write durations vary. Existing store spans include pool/lock waits,
  transaction work, and result assembly where applicable; they are not pure SQL timings.

## Deferred

- Pinned-state editing: currently not needed for the small remembered fleet.
- Browser-render acknowledgement: attach acceptance and snapshot generation do
  not prove xterm finished restoring. This would require additional protocol work.
- Retaining terminal images across recovery: text-screen snapshots do not provide image replay.
- CPU/heap profiling: use targeted profiling when a measured issue calls for it;
  OTLP traces are not CPU profiles or flame graphs.
- Grafana dashboard construction is the user's exercise; keep improving the recipe book rather than taking over dashboard editing.

## Release Notes

- No version bump is made by this roadmap. Additive optional config fields alone
  do not require 0.4.0; reconsider if compatibility or defaults change materially.
- Recheck live telemetry after deployment. A Grafana volume reset can remove
  history and invalidate service-account tokens or data-source UIDs.
- Never commit Grafana tokens, UCI credentials, or router backups.
