# Core Endpoints and Wake Routes

Wakey will model observed network contact points as **Endpoints** in `wakey-core`, and derive **Wake Routes** from those endpoints instead of reconstructing routes from flattened device summary MAC/IP arrays. `Device.ips` and `Device.macs` remain true summary fields for operator display and compatibility, but endpoint data is authoritative for wake, freshness, source, per-IP state, and route selection.

**Status**: accepted

## Context

The previous model stored a `Device` aggregate with summary `macs`, `ips`, `presence`, and raw observation facts. The control plane then built fleet wake routes from combinations like first MAC plus first IP. That lost source, presence, interface, and MAC/IP-pair evidence, especially when hook memory contained removed IPs or when several sources described the same address differently.

## Decision

Add a typed `DeviceEndpoint` concept in `wakey-core`. A device snapshot sent by an agent contains devices with endpoints already attached. The control plane stores structured endpoint rows and derives fleet summaries and wake routes from those rows.

Endpoint shape:

```rust
DeviceEndpoint {
    key: EndpointKey,
    hostname: Option<String>,
    interface: Option<String>,
    presence: Presence,
    first_seen_unix: Option<u64>,
    last_seen_unix: Option<u64>,
}

EndpointKey {
    source: EndpointSource,
    mac: Option<MacAddr>,
    ip: Option<IpAddr>,
}

AgentEndpointKey {
    agent_id: String,
    endpoint: EndpointKey,
}
```

`EndpointKey` must contain at least one of MAC or IP. Hostname and interface are endpoint evidence/metadata, not identity. `AgentEndpointKey` may use `serde(flatten)` for wire shape, but hash values are never durable storage keys. String keys are boundary serialization only.

## Source and Presence Rules

Endpoint sources are typed domain variants, not free strings:

```text
Neighbor
DhcpLease
HookNeighbor
HookDhcp
```

Presence answers “is this reachable?”, not “can this be woken?” The presence order is:

```text
Unknown < Offline < LikelyOnline < Online
```

Source mapping:

```text
Neighbor reachable/permanent -> Online
Neighbor stale -> LikelyOnline
Neighbor failed/remove-like evidence -> Offline
Other neighbor states -> Unknown

Current DHCP lease -> Unknown
Hook DHCP add/update/remove -> Unknown
Hook neighbor add/update/old -> LikelyOnline
Hook neighbor remove -> Offline
```

Current DHCP leases are summary-eligible and may support wake if they include a MAC, but they do not prove reachability. Hook memory creates endpoints and observation facts, but does not directly inflate summary IPs.

## Summary, Presence, and UI

`Device.ips` and `Device.macs` stay as summaries. Summary IPs come from concrete live-ish sources such as current neighbor rows and current DHCP lease rows. Hook-derived and offline IPs remain visible through endpoints but are collapsed out of primary UI summaries by default.

`Device.presence` is derived from all endpoints, not only summary-eligible endpoints. This lets concrete offline evidence matter while allowing stronger live evidence to override it.

The main fleet API embeds endpoints inside each fleet device. API responses expose all endpoints, including offline and unknown endpoints. The UI may collapse offline/unknown endpoints in the primary row, but details must expose them for copy/debug/history.

## Wakeability and Wake Route Ordering

Wakeability is derived, not stored on `DeviceEndpoint`.

An endpoint is wakeable when:

```text
endpoint has a MAC
agent is connected
agent has wake capability
endpoint source is allowed for wake route derivation
```

For now, a connected wakey-agent is assumed to have wake capability. Presence does not gate wakeability: an `Offline` endpoint may still produce a wake route, because waking powered-off devices is the core use case.

Preferred wake route order:

```text
connected agent
wakeable route
has IP
presence quality
newest last_seen
source quality
```

Presence quality for route ranking follows:

```text
Online > LikelyOnline > Offline > Unknown
```

Source quality follows:

```text
Neighbor > DhcpLease > HookNeighbor > HookDhcp
```

Equivalent wake routes may ignore endpoint source when they target the same agent, MAC, and IP. Endpoint evidence preserves source-specific rows; route grouping may deduplicate equivalent targets.

## Device and Identity Rules

`DeviceId` remains the typed observed identity enum:

```rust
DeviceId::Mac(MacAddr)
DeviceId::Ip(IpAddr)
```

`DeviceId` is derived from endpoints: canonical MAC if any endpoint has a MAC, otherwise canonical IP, otherwise absent. Local source rows are grouped into devices by endpoint-derived `DeviceId`, not exact `EndpointKey`.

Within a single complete local snapshot, a MAC+IP endpoint may absorb an IP-only endpoint with the same IP. This merge is not remembered across snapshots unless the operator creates a known-device identifier.

Known-device identifiers are source-independent ownership claims over MAC or permanent IP values. Wakey does not infer whether an IP is permanent, static, or reserved; the UI should warn the operator when attaching an IP identifier.

## Control Plane Storage

The control plane stores current per-agent endpoint snapshots as structured columns, not opaque `Device` JSON and not endpoint event history. Summary MAC/IP collections are derived from endpoints; separate MAC/IP storage should be removed or represented as views rather than maintained as independent truth.

Agent-local hook memory is retained by local last-seen age policy, not by control-plane acknowledgement. The default retention is 7 days unless configured otherwise. Live-source endpoints use snapshot time as their observed time; hook-derived endpoints use the hook row's recorded last-seen time.

## Consequences

- Wake route construction moves away from `macs.first()` / `ips.first()` guessing.
- The agent/core layer owns source interpretation because it still has local context.
- The control plane can search, filter, match known identifiers, and rank wake routes from structured endpoint rows.
- UI can show clean summaries while keeping complete endpoint evidence in details.
- This preserves “wake an offline machine by MAC” without pretending that offline means not wakeable.
