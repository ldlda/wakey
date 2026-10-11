//! Shared, opt-in OTLP setup. Local logging remains available without a collector.

use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use opentelemetry::{
    KeyValue, global,
    trace::{TraceContextExt as _, TracerProvider as _},
};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_otlp::WithTonicConfig;
use opentelemetry_sdk::{
    Resource, logs::SdkLoggerProvider, metrics::SdkMeterProvider, trace::SdkTracerProvider,
};
use serde::{Deserialize, Serialize};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::{EnvFilter, Layer, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// A single gRPC receiver accepts all three signals. No endpoint means local-only.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TelemetryConfig {
    pub otlp_endpoint: Option<String>,
    /// Name of the environment variable holding the complete Authorization header.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_env: Option<String>,
    pub service_name: String,
    pub json_logs: bool,
    /// Explicit per-signal filters override RUST_LOG and CLI verbosity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub console_filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub otlp_log_filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_filter: Option<String>,
}

/// Owns providers until all instrumented work has stopped; flush off the async workers.
#[derive(Default)]
pub struct TelemetryGuard {
    traces: Option<SdkTracerProvider>,
    logs: Option<SdkLoggerProvider>,
    metrics: Option<SdkMeterProvider>,
}

impl TelemetryGuard {
    pub fn shutdown(self) {
        // Export failures must not turn an otherwise successful daemon exit into a failure.
        if let Some(provider) = self.metrics {
            let _ = provider.shutdown();
        }
        if let Some(provider) = self.logs {
            let _ = provider.shutdown();
        }
        if let Some(provider) = self.traces {
            let _ = provider.shutdown();
        }
    }
}

pub fn init(
    config: &TelemetryConfig,
    service: &str,
    default_filter: &str,
    instance: Option<&str>,
) -> Result<TelemetryGuard> {
    let rust_log = std::env::var("RUST_LOG").ok();
    let console_filter = signal_filter(
        config.console_filter.as_deref(),
        rust_log.as_deref(),
        default_filter,
    )?;
    let service = if config.service_name.is_empty() {
        service
    } else {
        &config.service_name
    };
    let mut guard = TelemetryGuard::default();
    if let Some(endpoint) = config
        .otlp_endpoint
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        // Filters belong to individual layers: a quiet console must not globally
        // disable debug events needed by the OTLP log exporter.
        let trace_filter = signal_filter(
            config.trace_filter.as_deref(),
            rust_log.as_deref(),
            default_filter,
        )?;
        let log_filter = signal_filter(
            config.otlp_log_filter.as_deref(),
            rust_log.as_deref(),
            default_filter,
        )?;
        let metadata = authorization_metadata(config.authorization_env.as_deref(), |name| {
            std::env::var(name).ok()
        })?;
        let mut resource = Resource::builder_empty().with_service_name(service.to_owned());
        if let Some(instance) = instance {
            resource =
                resource.with_attribute(KeyValue::new("service.instance.id", instance.to_owned()));
        }
        let resource = resource.build();
        // Small fixed queues are intentional: a disconnected collector cannot grow router RAM.
        // Exporters own transient retries/backoff; authentication failures are
        // not retried. The timeout remains their budget, not a durable outbox.
        let trace_exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_metadata(metadata.clone())
            .with_endpoint(endpoint)
            .with_timeout(Duration::from_secs(3))
            .with_retry_policy(opentelemetry_otlp::RetryPolicy::recommended())
            .build()
            .context("OTLP trace exporter")?;
        let trace_processor = opentelemetry_sdk::trace::BatchSpanProcessor::builder(trace_exporter)
            .with_batch_config(
                opentelemetry_sdk::trace::BatchConfigBuilder::default()
                    .with_max_queue_size(256)
                    .with_max_export_batch_size(64)
                    .build(),
            )
            .build();
        let traces = SdkTracerProvider::builder()
            .with_span_processor(trace_processor)
            .with_resource(resource.clone())
            .build();
        let log_exporter = opentelemetry_otlp::LogExporter::builder()
            .with_tonic()
            .with_metadata(metadata.clone())
            .with_endpoint(endpoint)
            .with_timeout(Duration::from_secs(3))
            .with_retry_policy(opentelemetry_otlp::RetryPolicy::recommended())
            .build()
            .context("OTLP log exporter")?;
        let log_processor = opentelemetry_sdk::logs::BatchLogProcessor::builder(log_exporter)
            .with_batch_config(
                opentelemetry_sdk::logs::BatchConfigBuilder::default()
                    .with_max_queue_size(256)
                    .with_max_export_batch_size(64)
                    .build(),
            )
            .build();
        let logs = SdkLoggerProvider::builder()
            .with_log_processor(log_processor)
            .with_resource(resource.clone())
            .build();
        let metric_exporter = opentelemetry_otlp::MetricExporter::builder()
            .with_tonic()
            .with_metadata(metadata)
            .with_endpoint(endpoint)
            .with_timeout(Duration::from_secs(3))
            .with_retry_policy(opentelemetry_otlp::RetryPolicy::recommended())
            .build()
            .context("OTLP metric exporter")?;
        let reader = opentelemetry_sdk::metrics::PeriodicReader::builder(metric_exporter)
            .with_interval(Duration::from_secs(30))
            .build();
        let metrics = SdkMeterProvider::builder()
            .with_reader(reader)
            .with_resource(resource)
            .build();
        global::set_tracer_provider(traces.clone());
        global::set_meter_provider(metrics.clone());
        let _ = METRICS.set(Instruments::new());
        let otel = tracing_opentelemetry::layer()
            .with_tracer(traces.tracer(service.to_owned()))
            .with_filter(trace_filter);
        // Exporter failures must remain local, never feed back into the failing exporter.
        let logs_layer =
            opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(&logs)
                .with_filter(log_filter)
                .with_filter(tracing_subscriber::filter::filter_fn(|metadata| {
                    !metadata.target().starts_with("opentelemetry")
                }));
        let subscriber = tracing_subscriber::registry().with(otel).with(logs_layer);
        if config.json_logs {
            subscriber
                .with(
                    fmt::layer()
                        .json()
                        .with_timer(local_timer())
                        .with_filter(console_filter),
                )
                .try_init()?;
        } else {
            subscriber
                .with(
                    fmt::layer()
                        .with_timer(local_timer())
                        .with_filter(console_filter),
                )
                .try_init()?;
        }
        guard = TelemetryGuard {
            traces: Some(traces),
            logs: Some(logs),
            metrics: Some(metrics),
        };
    } else if config.json_logs {
        tracing_subscriber::registry()
            .with(
                fmt::layer()
                    .json()
                    .with_timer(local_timer())
                    .with_filter(console_filter),
            )
            .try_init()?;
    } else {
        tracing_subscriber::registry()
            .with(
                fmt::layer()
                    .with_timer(local_timer())
                    .with_filter(console_filter),
            )
            .try_init()?;
    }
    Ok(guard)
}

fn signal_filter(
    explicit: Option<&str>,
    rust_log: Option<&str>,
    default_filter: &str,
) -> Result<EnvFilter> {
    if let Some(value) = explicit {
        anyhow::ensure!(
            !value.trim().is_empty(),
            "telemetry filter must not be empty"
        );
        return EnvFilter::try_new(value).context("invalid telemetry filter");
    }
    // Preserve the existing fallback for an absent or invalid RUST_LOG.
    rust_log
        .and_then(|value| EnvFilter::try_new(value).ok())
        .map(Ok)
        .unwrap_or_else(|| {
            EnvFilter::try_new(default_filter).context("invalid default telemetry filter")
        })
}

fn authorization_metadata(
    name: Option<&str>,
    lookup: impl FnOnce(&str) -> Option<String>,
) -> Result<tonic::metadata::MetadataMap> {
    let mut metadata = tonic::metadata::MetadataMap::new();
    let Some(name) = name else {
        return Ok(metadata);
    };
    anyhow::ensure!(
        !name.is_empty()
            && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
            && !name.as_bytes()[0].is_ascii_digit(),
        "telemetry authorization_env must be a valid environment variable name"
    );
    let raw = lookup(name).with_context(|| {
        format!("telemetry authorization environment variable {name} is missing or not UTF-8")
    })?;
    let header = raw.trim();
    anyhow::ensure!(
        !header.is_empty(),
        "telemetry authorization environment variable {name} is empty"
    );
    let valid_scheme = header.split_once(' ').is_some_and(|(scheme, credentials)| {
        (scheme.eq_ignore_ascii_case("basic") || scheme.eq_ignore_ascii_case("bearer"))
            && !credentials.trim().is_empty()
    });
    anyhow::ensure!(
        valid_scheme && !raw.contains(['\r', '\n']),
        "telemetry authorization environment variable {name} must contain a single Basic or Bearer header"
    );
    // Never include the supplied value/parser error in diagnostics. Sensitive
    // metadata also hides it if a dependency Debug-prints an outgoing request.
    let mut value = tonic::metadata::MetadataValue::try_from(header).map_err(|_| {
        anyhow::anyhow!("invalid authorization metadata in environment variable {name}")
    })?;
    value.set_sensitive(true);
    metadata.insert("authorization", value);
    Ok(metadata)
}

fn local_timer() -> fmt::time::OffsetTime<time::format_description::well_known::Rfc3339> {
    fmt::time::OffsetTime::new(
        time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC),
        time::format_description::well_known::Rfc3339,
    )
}

// Construct instruments once, after the real provider exists. Disabled telemetry
// therefore does no per-PTY-read allocations or instrument registration.
static METRICS: OnceLock<Instruments> = OnceLock::new();
mod fleet;
pub use fleet::{AgentDeviceCounts, FleetCounts, FleetMetrics};
struct Instruments {
    operations: opentelemetry::metrics::Counter<u64>,
    duration: opentelemetry::metrics::Histogram<f64>,
    snapshot_devices: opentelemetry::metrics::Counter<u64>,
    terminal_active: opentelemetry::metrics::UpDownCounter<i64>,
    terminal_input: opentelemetry::metrics::Counter<u64>,
    terminal_output: opentelemetry::metrics::Counter<u64>,
    terminal_backpressure: opentelemetry::metrics::Counter<u64>,
}
impl Instruments {
    fn new() -> Self {
        let meter = global::meter("wakey");
        Self {
            operations: meter.u64_counter("wakey.operations").build(),
            duration: meter
                .f64_histogram("wakey.operation.duration")
                .with_unit("s")
                .build(),
            snapshot_devices: meter.u64_counter("wakey.snapshot.devices").build(),
            terminal_active: meter.i64_up_down_counter("wakey.terminal.active").build(),
            terminal_input: meter
                .u64_counter("wakey.terminal.input.bytes")
                .with_unit("By")
                .build(),
            terminal_output: meter
                .u64_counter("wakey.terminal.output.bytes")
                .with_unit("By")
                .build(),
            terminal_backpressure: meter.u64_counter("wakey.terminal.backpressure").build(),
        }
    }
}

/// W3C context only, never arbitrary application fields or credentials.
pub fn current_trace_context() -> BTreeMap<String, String> {
    use opentelemetry::propagation::TextMapPropagator;
    let mut carrier = std::collections::HashMap::new();
    opentelemetry_sdk::propagation::TraceContextPropagator::new()
        .inject_context(&tracing::Span::current().context(), &mut carrier);
    carrier.into_iter().collect()
}

pub fn set_remote_parent(span: &tracing::Span, carrier: &BTreeMap<String, String>) {
    use opentelemetry::propagation::TextMapPropagator;
    let carrier: std::collections::HashMap<String, String> = carrier
        .iter()
        .filter(|(key, value)| {
            matches!(key.as_str(), "traceparent" | "tracestate") && value.len() <= 512
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let context = opentelemetry_sdk::propagation::TraceContextPropagator::new().extract(&carrier);
    if context.has_active_span() {
        let _ = span.set_parent(context);
    }
}

/// Records completion on every return path, without high-cardinality identifiers.
pub struct Operation {
    started: Instant,
    name: &'static str,
    outcome: &'static str,
}

impl Operation {
    pub fn new(name: &'static str) -> Self {
        Self {
            started: Instant::now(),
            name,
            outcome: "error",
        }
    }
    pub fn outcome(&mut self, outcome: &'static str) {
        self.outcome = outcome;
    }
}

impl Drop for Operation {
    fn drop(&mut self) {
        let Some(metrics) = METRICS.get() else {
            return;
        };
        let labels = [
            KeyValue::new("operation", self.name),
            KeyValue::new("outcome", self.outcome),
        ];
        metrics.operations.add(1, &labels);
        metrics
            .duration
            .record(self.started.elapsed().as_secs_f64(), &labels);
    }
}

pub enum Counter {
    SnapshotDevices,
    TerminalInputBytes,
    TerminalOutputBytes,
    TerminalBackpressure,
}

pub fn count(counter: Counter, value: u64) {
    let Some(metrics) = METRICS.get() else {
        return;
    };
    let counter = match counter {
        Counter::SnapshotDevices => &metrics.snapshot_devices,
        Counter::TerminalInputBytes => &metrics.terminal_input,
        Counter::TerminalOutputBytes => &metrics.terminal_output,
        Counter::TerminalBackpressure => &metrics.terminal_backpressure,
    };
    counter.add(value, &[]);
}

/// An active session contributes until its worker finishes or is cancelled.
pub struct ActiveTerminal;
impl ActiveTerminal {
    pub fn new() -> Self {
        if let Some(metrics) = METRICS.get() {
            metrics.terminal_active.add(1, &[]);
        }
        Self
    }
}
impl Default for ActiveTerminal {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for ActiveTerminal {
    fn drop(&mut self) {
        if let Some(metrics) = METRICS.get() {
            metrics.terminal_active.add(-1, &[]);
        }
    }
}

#[cfg(test)]
mod auth_tests {
    #[test]
    fn explicit_signal_filters_override_environment_and_validate() {
        assert_eq!(
            super::signal_filter(Some("debug"), Some("warn"), "info")
                .unwrap()
                .to_string(),
            "debug"
        );
        assert_eq!(
            super::signal_filter(None, Some("warn"), "info")
                .unwrap()
                .to_string(),
            "warn"
        );
        assert_eq!(
            super::signal_filter(None, None, "info")
                .unwrap()
                .to_string(),
            "info"
        );
        assert!(super::signal_filter(Some(""), None, "info").is_err());
        assert!(super::signal_filter(Some("["), None, "info").is_err());
    }

    use super::*;

    #[test]
    fn trace_context_connects_collection_to_remote_persistence() {
        let provider = SdkTracerProvider::builder().build();
        let subscriber = tracing_subscriber::registry()
            .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("snapshot-test")));
        // Scoped subscriber and synchronous entered guards keep this isolated
        // from other tests without installing a global exporter or collector.
        tracing::subscriber::with_default(subscriber, || {
            let collect = tracing::info_span!("collect_device_snapshot");
            let carrier = {
                let _entered = collect.enter();
                current_trace_context()
            };
            let collection = carrier["traceparent"].split('-').collect::<Vec<_>>();
            let store = tracing::info_span!("store_device_snapshot");
            set_remote_parent(&store, &carrier);
            let persisted = {
                let _entered = store.enter();
                current_trace_context()
            };
            let persistence = persisted["traceparent"].split('-').collect::<Vec<_>>();
            assert_eq!(collection[1], persistence[1], "trace IDs must match");
            assert_ne!(
                collection[2], persistence[2],
                "spans must have distinct IDs"
            );

            let unrelated = tracing::info_span!("unrelated");
            set_remote_parent(&unrelated, &BTreeMap::new());
            set_remote_parent(
                &unrelated,
                &[("traceparent".into(), "invalid".into())]
                    .into_iter()
                    .collect(),
            );
            let _entered = unrelated.enter();
            let local = current_trace_context();
            assert_ne!(
                collection[1],
                local["traceparent"].split('-').nth(1).unwrap()
            );
        });
    }

    #[test]
    fn authorization_supports_both_schemes_and_redacts_debug() {
        for header in ["Basic dGVzdDp0ZXN0", "Bearer test-token"] {
            let metadata =
                authorization_metadata(Some("OTLP_AUTH"), |_| Some(header.into())).unwrap();
            assert_eq!(
                metadata.get("authorization").unwrap().to_str().unwrap(),
                header
            );
            assert!(!format!("{metadata:?}").contains(header));
        }
        assert!(
            authorization_metadata(None, |_| panic!("no credential lookup"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn missing_empty_and_invalid_auth_fail_without_echoing_secrets() {
        for value in [
            None,
            Some(""),
            Some("   "),
            Some("Basic "),
            Some("Digest private-secret"),
            Some("Basic private-secret\r\n"),
            Some("Bearer private-secret\u{7f}"),
        ] {
            let error = authorization_metadata(Some("OTLP_AUTH"), |_| value.map(str::to_owned))
                .unwrap_err();
            assert!(!format!("{error:#}").contains("private-secret"));
        }
        assert!(
            authorization_metadata(Some("BAD=NAME"), |_| panic!(
                "invalid name must not be looked up"
            ))
            .is_err()
        );
    }
}
