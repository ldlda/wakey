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

## Snapshots

Device entries in successfully sent snapshots over the last hour:

```promql
sum by (job, instance) (increase(wakey_snapshot_devices_total[1h]))
```

Repeated snapshots count the same devices again. This is collection volume,
**not unique devices, current fleet size, or confirmed CC persistence**.

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
