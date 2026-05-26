# Wakey

Wakey models devices discovered on a local network and the control-plane view used to wake and manage them across agents.

## Language

**Device**:
An operator-facing aggregate that represents one discovered network identity candidate.
_Avoid_: Host, machine, route

**Device Summary**:
The derived names, MACs, IPs, and presence shown on a device aggregate.
_Avoid_: Source of truth, route data

**Endpoint**:
One observed network contact point for a device, including the agent/source, optional MAC, optional IP, presence, and observation time.
_Avoid_: Raw observation, route, IP row

**Endpoint Source**:
The typed origin of an endpoint, such as neighbor data, DHCP lease data, or a hotplug hook.
_Avoid_: String kind, source label

**Observation Fact**:
Debug evidence copied from local observation inputs before it is interpreted into endpoints.
_Avoid_: Endpoint, identifier, source of truth

**Presence**:
The current reachability interpretation for a device or endpoint.
_Avoid_: Endpoint state, status

**Wake Route**:
An endpoint that has enough information and agent availability to attempt waking a device.
_Avoid_: Device, endpoint when not wakeable

**Known Device**:
A device identity manually curated by the operator.
_Avoid_: Auto-pinned device, saved observation

**Identifier**:
A manually approved MAC or IP value that links an observed device or endpoint to a known device.
_Avoid_: Observation key, route id

**Endpoint Key**:
The typed identity of endpoint evidence, based on source and optional MAC/IP.
_Avoid_: Stringly typed route id

**Agent Endpoint Key**:
An endpoint key scoped to the agent that observed it.
_Avoid_: Concatenated agent/source/MAC/IP string

**Agent**:
A fleet member that observes local network state and sends device snapshots to the control plane.
_Avoid_: Router when referring to the software actor

**Agent Host**:
The machine running an agent, including host identity and runtime metadata.
_Avoid_: Device when referring to the agent machine

**Agent Capability**:
Something an agent host can observe or do on the network.
_Avoid_: Router flag

**Interface Telemetry**:
Current operational measurements for a network interface visible to an agent.
_Avoid_: Endpoint state, device presence

## Relationships

- A **Device** has zero or more **Endpoints**.
- A **Device Summary** is derived from a **Device** and its **Endpoints**.
- Local source rows are grouped into **Devices** by endpoint-derived **DeviceId**, not by exact **Endpoint Key**.
- Within one complete local snapshot, MAC+IP endpoints may absorb IP-only endpoints with the same IP; this merge is not remembered across snapshots unless the operator creates an **Identifier**.
- An **Endpoint** belongs to exactly one **Agent**.
- An **Endpoint** has one **Endpoint Key**.
- An **Agent Endpoint Key** combines an **Agent** with an **Endpoint Key**.
- An **Endpoint** may have both MAC and IP, MAC only, or IP only.
- An **Endpoint** has exactly one **Endpoint Source**.
- An **Endpoint** has one **Presence**.
- An **Endpoint** may name the network interface where it was observed.
- An **Observation Fact** may explain why an **Endpoint** exists, but does not define identity or wakeability.
- A **Wake Route** is derived from exactly one **Endpoint**.
- A **Wake Route** may carry the interface from its source **Endpoint**.
- A **Known Device** has zero or more **Identifiers**.
- An **Identifier** can link observed **Devices** or **Endpoints** to one **Known Device**.
- An **Agent** runs on one **Agent Host**.
- An **Agent Capability** describes what an **Agent Host** can observe or do.
- **Interface Telemetry** describes an **Agent Host** interface, not a **Device Endpoint**.

## Example dialogue

> **Dev:** "This **Device** has three IPs. Which one should the UI wake?"
> **Domain expert:** "Do not infer that from the **Device** summary. Use the **Wake Routes** derived from its **Endpoints**."

## Flagged ambiguities

- "route" was used for both observed network contact points and wakeable paths — resolved: use **Endpoint** for observed contact points and **Wake Route** only when the endpoint can be used for wake.
- "device" was used for both an identity aggregate and each MAC/IP pair — resolved: use **Device** for the aggregate and **Endpoint** for each observed MAC/IP/source/state tuple.
- `Device.ips` and `Device.macs` are true summary values, but they are not authoritative for wake, freshness, or per-IP state decisions — resolved: use **Endpoint** for those decisions.
- MAC-only and IP-only observations are still **Endpoints** — resolved: **Wake Routes** require a MAC and a connected **Agent**, while IP is optional route detail.
- Endpoint-specific states such as "removed" or "stale" are not separate concepts yet — resolved: use **Presence** until a distinction appears that presence cannot express cleanly.
- Endpoint origin is not a free string — resolved: use **Endpoint Source** for domain logic and reserve raw strings for debug facts only.
- Local observation file rows map naturally to **Observation Facts** — resolved: keep them for debugging, but do not use them as the operational model.
- Offline **Endpoints** remain part of the API model — resolved: UI may collapse them from the primary view, but the API should still expose them for details, copy, and debugging.
- **Device Summary** IPs come only from concrete live-ish sources such as neighbor table rows and current DHCP lease rows — resolved: hotplug hook memory creates **Endpoints** and **Observation Facts**, but does not directly inflate summary IPs.
- Router-ness is not a boolean domain concept — resolved: model **Agent Capabilities** and **Agent Host** metadata instead.
- **Presence** ordering should treat **Unknown** as non-voting/no evidence, weaker than **Offline** — resolved: concrete offline evidence should not be overridden by unknown hook or DHCP ambiguity.
- **Device** presence is derived from all **Endpoints**, not only summary-eligible endpoints.
- Current DHCP leases are summary-eligible **Endpoints** with **Unknown** presence; they may support wake when a MAC is present, but they do not prove reachability.
- DHCP hook events, including removal, indicate **Unknown** because they describe lease churn rather than reachability.
- Neighbor table and neighbor hook events may indicate **Offline** when they report failed or removed reachability.
- **Presence** and wakeability are separate: an **Offline** endpoint may still produce a **Wake Route** if it has a MAC and a connected **Agent**.
- **Endpoint** does not store wakeability; wakeability is derived from endpoint data plus **Agent** runtime and capability context.
- Preferred **Wake Routes** are ordered by connected agent, wakeability, having an IP, presence quality, recency, then source quality.
- **Endpoint** evidence preserves source-specific rows; equivalent **Wake Routes** may ignore source when they target the same agent, MAC, and IP.
- The control plane stores **Endpoints** as structured data so fleet search, identity matching, and wake route selection do not depend on parsing opaque device JSON.
- Summary MAC/IP collections are derived from **Endpoints**; separate MAC/IP storage should be removed or represented as views rather than maintained as independent truth.
- API arrays should be domain-sorted by default; UI may sort top-level fleet rows for presentation, but route ranking and recommended route selection are backend-owned.
- The main fleet API embeds **Endpoints** in each **Device** response; a separate endpoint API is unnecessary until payload size or access patterns require it.
- The control plane stores current per-agent endpoint snapshots, not endpoint event history.
- Agent-local hook memory is retained by local last-seen age policy, not by control-plane acknowledgement; default retention is 7 days unless configured otherwise.
- Live-source **Endpoints** use snapshot time as their observed time; hook-derived **Endpoints** use the hook row's recorded last-seen time.
- **DeviceId**, **Endpoint Key**, and **Agent Endpoint Key** are typed concepts; string keys are boundary serialization only.
- **DeviceId** is derived from a device's **Endpoints**: canonical MAC if any endpoint has a MAC, otherwise canonical IP, otherwise absent.
- Hostnames are endpoint evidence/display data, not part of **Endpoint Key** identity.
- Interface names are endpoint metadata, not part of **Endpoint Key** identity; multi-segment routers may require scoped endpoint identity later if the same source/MAC/IP has different meaning on different interfaces.
- An **Endpoint Key** must contain at least one network address value: MAC, IP, or both. Facts without MAC or IP remain **Observation Facts** only.
- **Identifiers** are source-independent ownership claims over MAC or permanent IP values, not claims over endpoint sources.
- IP **Identifiers** are explicit operator claims; Wakey does not infer whether an IP is permanent, static, or reserved.
