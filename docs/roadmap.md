# Wakey Roadmap

Last updated: 2026-10-12. This records agreed work and follow-ups, not a release
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

- [x] Endpoint-derived presence on snapshot writes and fleet reads; endpoint-less legacy rows retain their summary. CLI ordering matches the core signal order.

- [x] Make **forget remembered device** reachable in the frontend, with confirmation. The existing backend removes the known identity and its identifiers, not agent observations. An observed device may reappear as unknown.
- [x] Add remembered-device rename support, with backend validation and frontend editing (`PATCH /api/v1/control/devices/{device_id}`). Pinned state and identifiers are preserved.
- [ ] Check and complete editing of remembered-device notes.
- [x] Replace generic unnamed-row labels with useful fallbacks. Prefer saved name, hostname, then `Device: <mac>`, then `Device: <ip>`; retain a generic label only when neither address exists. Presence stays in the badge, not the name. Inactive IPs can label IP-only rows without becoming active IPs or wake routes. Unknown-only fleet groups remain unknown.
- [ ] Review frontend visibility defaults. Keep an explicit all-rows option; do not hide evidence just to fix poor labels. The user deliberately enabled all-row querying.
- [x] Keep device details current after identity mutations change the grouping key or list filters exclude the result. Exact fleet details and full saved-identity reads follow the mutation's explicit ID; no shared-IP matching or stale-control fallback. Remember/Attach/Rename verified with mocked browser flows on desktop, Remember/Rename on mobile; backend tests cover filtering, old-key rejection and lookup beyond the list cap. Forget deliberately closes; detaching stays on the saved identity instead of guessing among split observations.

Identifier detachment already exists in the current frontend source; it is **not**
the requested whole-device removal. No automatic identity promotion or deletion
of remembered identifiers based on observation expiry.

## Telemetry Follow-Ups

- [x] Human-readable agent/CC config durations with unit-aware legacy loading, stderr deprecation warnings, and canonical init-config output. See [configuration durations](config-durations.md).
- [x] CC device retention by last snapshot inclusion (`device_retention = "60d"`, zero disables), independent of audit history and preserving saved identities.
- [x] Lazy grouped fleet-view cache with store/connection invalidation and stale-build rejection. See [fleet cache](fleet-cache.md).

- [x] Separate console, OTLP log, and trace filters using per-layer filtering. Explicit signal config overrides `RUST_LOG`, then CLI verbosity supplies defaults. Unconfigured defaults remain info-level; exporter-failure logs stay local. Live validation pending.
- [x] Upgrade the matched OpenTelemetry stack to 0.33 with exporter-managed backoff/jitter retries. Controlled collector tests cover all-signal recovery, bounded retries, and no retry on authentication rejection; live outage validation remains pending.
- [ ] Record hostname-resolution success/failure without exporting hostnames or resolved addresses. Current spans show timing but do not explicitly classify the result.
- [ ] Investigate the slow resolver with a reproducible device-side check. Timing isolated the phase, not the cause; do not treat instrumentation as a fix.
- [x] Propagate optional W3C trace context for periodic/requested WS snapshots so agent collection and CC persistence can be inspected together. Wire compatibility and context linkage tested; live Grafana inspection confirmed CC storage is a child of agent collection in the same trace. Storage can start after collection ends: this is an asynchronous handoff, not a persistence acknowledgement.
- [x] Add current-state gauges: retained device/endpoint rows per agent and unfiltered grouped fleet identities split saved/unsaved. Sample independently every 30 seconds with bounded revision checks; replace labels and suppress failed samples. Collector and state lifecycle tests included; dashboard recipes ready.
- [ ] Decide whether snapshots need persistence acknowledgements. The existing sent-device counter does not prove CC storage; any confirmed-storage metric needs an authoritative measurement point.
- [x] Break down snapshot persistence timing into pool acquisition, transaction acquisition, row replacement, and commit child spans. Shared write acquisition retains SQLx ownership and `BEGIN IMMEDIATE`; snapshot row work stays in the caller's transaction. Existing rollback and cache-refresh tests cover behavior. Live trace screenshot confirmed all phases: about 83 microseconds acquiring a connection, 154 microseconds beginning the transaction, 12.8 ms replacing rows, and 18.94 ms committing. This sample does not justify changing SQLite durability or prove a persistent bottleneck. See [telemetry span semantics](telemetry.md#signals).

The existing snapshot counter is cumulative collection volume, not unique devices
or current fleet size. Metric labels should remain bounded; no per-device labels.

## Process And Host Visibility

- [x] Add Linux process age/RSS and host uptime gauges for agent and CC through opt-in OTLP, using the existing collection interval. Core owns OS-independent instruments; wakey-linux supplies the optional procfs reader. Missing data omits observations; no added labels, config fields, health routes or polling tasks. Collector coverage verifies units and values; router/live dashboard validation pending.
- [x] Add cumulative process CPU time (counter) and current open-FD count (gauge) through the same Linux reader and OS-independent instruments. Exclude child processes, omit failed samples, and document one-core CPU-rate interpretation and sampling FD overhead. Extend collector/reader coverage rather than adding timing-based tests; live validation pending.
- [ ] Add start-time/version metadata if needed. Keep version as resource metadata rather than repeating it on every sample.
- [x] Add total/available host memory and 1/5/15-minute load gauges through the optional Linux reader, with byte conversion owned by procfs and fixed load-period labels. Omit unavailable MemAvailable instead of substituting free memory; document kernel views versus container limits. Collector/reader coverage included; live validation pending.
- [ ] Complete a small host summary with hostname, OS/kernel and CPU count. Sources must tolerate missing Linux files; not every agent host is a router.
- [ ] Display CC and agent process/host information in Fleet Health and agent details: version/name, process/host uptime, RSS, CPU time/usage, open FDs, available/total host memory and load where supported. OTLP export is not the UI's data source; the UI must work without a collector.
- [ ] Add typed agent status snapshots over the existing authenticated WebSocket, on a modest timer and when requested. Reuse Linux collection rather than introducing separate parsers; CC retains each agent's latest sample with received time and connection status. Show stale/disconnected or unavailable values explicitly, never as zero/current. Keep routine samples in memory initially, not an unbounded DB history.
- [ ] Expose the CC's own sampled status and retained agent summaries through protected control APIs. Keep `/healthz` unchanged and do not expose environment variables, credentials or command lines. CPU percentages require successive cumulative CPU/time samples; do not label raw CPU seconds as utilization. No expensive host reads on every UI request.
- [ ] Keep `/healthz` as cheap liveness; consider a separate bounded `/readyz` database-access check for CC. Collector outages and disconnected fleet agents must not make a functioning CC unready.
- [ ] Add application resource gauges where useful: DB pool usage, connected agents, pending commands, and active terminal sessions. Check existing instruments first to avoid duplicates; distinguish pool pressure from host/process pressure.

## Device Diagnostics

- [ ] Add agent-side ICMP ping through the existing command relay, with an explicit IP and selected agent/network route. Bound probe count, total duration, output and concurrent work; no shell interpolation. Account for missing tools and ICMP permissions, and support IPv4/IPv6 where available.
- [ ] Return probe replies, loss and round-trip times; offer Ping alongside Wake in device details. MAC-only devices cannot be pinged without an IP. A reply means that IP responded from that agent's network, not proof of saved identity; no reply must not automatically mark a device offline or remove an identifier.

## Optional Agent Sources And Capabilities

These are exploration items, not a commitment to a router-only model or a new
message bus. Sparse WSL inventory is legitimate; missing evidence is not failure.

- [ ] Discover and advertise usable observation sources separately from supported commands and configured permissions. Extend the existing hello/capability mechanism where appropriate; CC/UI must tolerate missing sources and older agents. Do not infer router status from Linux alone.
- [ ] Investigate read-only Wi-Fi station data on the actual router: signal in **dBm**, association/interface, rates and traffic counters where available. Evaluate driver-supported `iw`/netlink or local `ubus`/hostapd interfaces before choosing a dependency. Keep measurements scoped to agent/interface/station with source and capture time; they are not permanent identity or globally valid presence. UI may show a signal badge only when supported and sufficiently fresh.
- [ ] Investigate interface lifecycle hooks for WAN/link changes, keeping scripts minimal and logic in the binary. Interface up/down does not prove Internet reachability; any active connectivity probe needs an explicit target and bounded policy. Existing DHCP/neighbor hooks remain independent evidence, not a way to enrich or overwrite other sources.
- [ ] Keep source support, operator authorization and local agent policy distinct. Capability advertisement is not permission: CC checks authorization and the agent enforces configured limits. New diagnostic APIs should be typed, protected operations, not arbitrary shell, UCI or bus calls exposed to the browser.

OpenWrt [hotplug](https://openwrt.org/docs/guide-user/base-system/hotplug) provides
event hooks; [ubus](https://openwrt.org/docs/techref/ubus) is local IPC/RPC, and
[UCI](https://openwrt.org/docs/guide-developer/ubus/uci) manages configuration.
None is a Kafka-style retained observation history. Verify what the vendor
firmware actually exposes before making any of them an agent requirement.

## Origin Authentication

- [x] Replace the nonempty assertion matcher in the Caddy template with `forward_auth`. The initial `lb.ldlda.com` probe reached Cloudflare; the later `mails.ldlda.com` probe reached an origin and confirmed 403 without an assertion versus 200 with a forged assertion on the same IP. This is a reproduced bypass, not just a theoretical concern; production rollout remains required.
- [x] Add CC's `/internal/auth/verify` endpoint with configured issuer/audience/key URL, RS256 signature and required claim validation, bounded cached key retrieval, single-flight refresh and outage cooldown. Return 204/401/503 without exposing tokens or identity data. Configured verification requires loopback binding, and `init-config --from-config` preserves it. See [origin authentication](origin-auth.md).
- [x] Cover forged/expired/wrong-audience/missing-claim tokens, key rotation/removal (including an empty set), duplicate assertions, unavailable/malformed/oversized key responses, cooldown and concurrent fetches. Router coverage verifies the oracle is separate from public/control handlers; actual Caddy enforcement is deployment-owned and still requires validation.
- [x] Use `http-cache-semantics` for issuer-supplied HTTP freshness instead of a hardcoded signing-key TTL. Preserve bounded streaming reads, force revalidation on unknown key IDs, and handle ETag/Last-Modified 304 responses without reparsing a body. Tests cover fresh reuse, Age, no-cache/no-store, unspecified freshness, conditional rotation and invalid 304 rejection; no sleeps required.
- [ ] Deploy the new CC verifier and Caddy policy for root/UI/control HTTP and operator WS handshakes. Keep public agent paths outside Access checks and the verifier unexposed publicly. Validate Caddy configuration, real Access login, agent connectivity, protected WebSocket rejection, and forged-header direct-origin rejection. Authentication failures must never fall back to header-only acceptance.

## Multi-CC Planning

- [ ] Consolidate stable CC/local-agent IDs, forwarded display names, per-CC enrollments, and agent config restructuring. See [multi-CC proposal](multi-cc-plan.md). Identity/name exchange and terminal ownership must be defined before adding connection workers; no CC-to-CC state synchronization is planned.

## Validation And Reliability

- [x] Bound fleet-cache rebuilds to three attempts under sustained mutations; return retryable HTTP 503 `fleet_busy` rather than silently serving stale wake routes. Deterministic limit/recovery tests included.
- [x] Separate fleet wake selection from dispatch; wake lookup uses the complete cached view rather than the presentation list's 1,000-row cap. Automated coverage checks explicit-route rejection and lookup beyond the cap; live checks confirm missing-device and invalid-route errors without dispatching wakes.
- [x] Share device read assembly within a single SQLite read transaction. Query-plan inspection on the test database with Python SQLite confirmed all six per-agent reads use primary-key searches; full-fleet reads scan intentionally, with covering indexes for MAC/IP projections and no temporary sort. No additional indexes justified by this check.
- [x] Separate fleet refresh selection, relay, decoding/storage, and aggregation. Preserve concurrency of eight and per-agent failures; tests cover disconnected agents, sorted partial results, invalid payload preservation, and valid empty snapshot replacement.
- [x] Add a refresh integration regression: a simulated agent answers through the production command relay; its inventory reaches SQLite, invalidates a previously populated fleet cache, and appears in the next fleet read. A disconnected agent does not cancel success; duplicate selection sends only one command. This covers the CC application path, not a real agent process or WebSocket transport.
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
