# Wakey Metrics Recipes

Use Grafana Explore with your Prometheus-compatible data source and the Code
query editor. These recipes assume standard OTLP-to-Prometheus name conversion.
Discover your actual series first:

```promql
count by (__name__, job) ({__name__=~"wakey[._].*"})
```

With Alloy's Prometheus exporter, service names normally become `job`, and agent
instance IDs become `instance`. Inspect the returned labels before adding filters.
For example, append `{job="wakey-agent"}` to a metric selector only if that is
the job value you actually see. Metrics export every 30 seconds; use a time range
that includes recent activity and allow at least two exports for rate queries.

## The Three Types

| Type | What it records | How to read it |
| --- | --- | --- |
| Counter | Cumulative events or bytes since process startup | `rate` for per-second throughput; `increase` for a window's estimated total |
| UpDownCounter | Adds and subtracts from a current quantity | Read the current value directly; do not use `rate` for active sessions |
| Gauge | Measures a current value, such as stored device rows | Read directly; do not use `rate` or `increase` |
| Histogram | Distribution of individual measurements | `_sum / _count` for an average; `_bucket` for estimated percentiles |

Counter resets on restart are handled by `rate` and `increase`. Apply those
functions **before** summing across instances so individual resets remain visible.

## Operations

Completed attempts per second, including errors and timeouts:

```promql
sum by (job, operation, outcome) (rate(wakey_operations_total[5m]))
```

Estimated attempts over the last hour:

```promql
sum by (job, operation, outcome) (increase(wakey_operations_total[1h]))
```

Average duration over the last five minutes:

```promql
sum by (job, operation) (rate(wakey_operation_duration_seconds_sum[5m]))
/
sum by (job, operation) (rate(wakey_operation_duration_seconds_count[5m]))
```

Set Grafana's unit to **seconds**. This is a weighted average, not an average of
instance averages. Both successful and unsuccessful attempts are included;
add `outcome="ok"` to both selectors for successful attempts only.

Estimated p95 duration:

```promql
histogram_quantile(
  0.95,
  sum by (le, job, operation) (
    rate(wakey_operation_duration_seconds_bucket[5m])
  )
)
```

This needs classic histogram bucket series. Keep `le` in the grouping: it is
the bucket boundary, in seconds. p95 is an estimate from those boundaries,
not an exact sort of recorded durations. Few operations make percentiles noisy.

CC relay duration includes waiting for the agent; agent execution duration
measures its dispatch separately. Do not sum both and call that user latency.

## Terminals

Active PTY workers by agent, including detached sessions:

```promql
sum by (job, instance) (wakey_terminal_active)
```

Read this directly as a count. It is not a count of browser tabs or attachments.

PTY output bytes per second:

```promql
sum by (job, instance) (rate(wakey_terminal_output_bytes_total[5m]))
```

Input bytes per second:

```promql
sum by (job, instance) (rate(wakey_terminal_input_bytes_total[5m]))
```

Set the unit to **bytes/sec**. Output is measured at the PTY read, even while
detached; it is not WebSocket wire traffic or confirmed browser delivery.

Relay queue-full encounters over the last fifteen minutes:

```promql
sum by (job, instance) (increase(wakey_terminal_backpressure_total[15m]))
```

This counts encounters, not their duration and not dropped chunks. A burst of
image output can raise it while output still arrives intact through backpressure.

Average duration of completed backpressure episodes, in seconds:

```promql
sum by (job, instance, outcome) (
  rate(wakey_operation_duration_seconds_sum{operation="terminal_backpressure_wait"}[5m])
)
/
sum by (job, instance, outcome) (
  rate(wakey_operation_duration_seconds_count{operation="terminal_backpressure_wait"}[5m])
)
```

An episode starts when the output channel is full and ends when the pending
backlog drains or the relay disconnects. It is not per-frame network latency;
unfinished episodes do not appear until they end. Keep the outcome grouping so
disconnected episodes are not confused with successfully drained ones.

## Snapshots

Device entries in successfully sent snapshots over the last hour:

```promql
sum by (job, instance) (increase(wakey_snapshot_devices_total[1h]))
```

Repeated snapshots count the same devices again. This is collection volume,
**not unique devices, current fleet size, or confirmed CC persistence**.

## Current Devices And Endpoints

These gauges are emitted by CC. Counts are sampled at startup and every 30 seconds,
independently of UI requests, and exported on the normal metrics schedule. Allow
up to two intervals for a change to appear. The snapshot counter above cannot
substitute for these gauges.

A **device** is a grouped device identity. An **endpoint** is one observed address
entry with an IP, optional MAC/interface, source, and presence. One PC can have
multiple IPv4/IPv6 endpoints, and different sources or agents can observe the
same address. An endpoint here is not an HTTP API route.

| Panel | Scope | Suggested visualization |
| --- | --- | --- |
| Stored devices by agent | Device rows currently retained in CC for each agent; not globally unique identities | Bar gauge, one value per agent |
| Stored endpoints by agent | Endpoint rows currently retained in CC for each agent, including inactive entries; not unique IPs | Bar gauge, one value per agent |
| Fleet devices | Grouped fleet identities, split into saved and unsaved | Stat, one value per category |

Stored device rows by agent:

```promql
sum by (job, agent_id) (wakey_stored_devices{job="wakey-control-plane"})
```

Stored endpoint rows by agent:

```promql
sum by (job, agent_id) (wakey_stored_endpoints{job="wakey-control-plane"})
```

Grouped fleet devices by manual identity status:

```promql
sum by (job, identity) (wakey_fleet_devices{job="wakey-control-plane"})
```

Adjust the `job` selector to your actual service label. These queries assume one
CC owns the fleet; do not sum replicas of the same database as distinct devices.
Enrolled agents with no stored rows report zero. After revocation, an agent can
remain represented while its device rows are retained. When neither enrollment
nor stored rows exist, the sampler stops reporting that agent label.

Use Grafana's **short** unit with zero decimal places. Read the gauges directly;
do not apply `rate` or `increase`. Counts must reflect unfiltered CC state, not
the last UI search or visibility filter. An agent going offline does not by
itself delete its stored rows, so these counts are not online-device counts.

Do not sum per-agent device counts and label that value "unique fleet devices":
the same device can be observed by more than one agent. Likewise, an endpoint
count is not a count of distinct IP addresses. Saved/unsaved describes manual
identity management, not online/offline/unknown presence.

Failed refreshes suppress observations until sampling recovers; they do not report
zero or keep exporting an old sample. Prometheus may still show its last value
until that series becomes stale. Export callbacks only read the in-memory sample;
they never wait on database work. Sampling is disabled when OTLP is disabled.

## Empty or Strange Results

- No series: generate the relevant activity, wait for export, and check the metrics pipeline.
- Different names: use the discovery query; collector translation can change suffixes and labels.
- `NaN` average: no operations occurred in the window, so the denominator is zero. Show a gap, not a fake zero latency.
- Empty `rate`: it needs multiple samples. Try a longer window and check the time range.
- Fractional `increase`: Prometheus extrapolates to the window boundaries; event counts can therefore appear fractional.
- Stale active count: the last exported value is not an immediate liveness check after a crash or network loss.
- No `instance` label: CC does not currently set a service instance ID; agent telemetry does.

## References

- [Wakey telemetry setup and limitations](telemetry.md)
- [Prometheus rate, increase, and histogram_quantile](https://prometheus.io/docs/prometheus/latest/querying/functions/)
- [Alloy OTLP-to-Prometheus conversion](https://grafana.com/docs/alloy/latest/reference/components/otelcol/otelcol.exporter.prometheus/)
