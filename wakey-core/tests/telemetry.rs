#![cfg(feature = "telemetry")]

use opentelemetry_proto::tonic::collector::{
    logs::v1 as logs, metrics::v1 as metrics, trace::v1 as traces,
};
use opentelemetry_proto::tonic::metrics::v1::{metric, number_data_point};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tonic::{Request, Response, Status};
use wakey_core::telemetry::{self, TelemetryConfig};

#[derive(Clone, Default)]
struct Collector {
    traces: Arc<Mutex<Vec<traces::ExportTraceServiceRequest>>>,
    logs: Arc<Mutex<Vec<logs::ExportLogsServiceRequest>>>,
    metrics: Arc<Mutex<Vec<metrics::ExportMetricsServiceRequest>>>,
    fail_until: usize,
    attempts: [Arc<AtomicUsize>; 3],
}

fn check_auth<T>(request: &Request<T>) -> Result<(), Status> {
    if request
        .metadata()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        != Some("Basic dGVzdDp0ZXN0")
    {
        return Err(Status::unauthenticated(
            "test receiver requires authorization",
        ));
    }
    Ok(())
}

/// Use a real receiver and counted RPCs, not sleep/elapsed-time expectations.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exporter_retries_are_bounded_and_bad_credentials_are_not_retried() {
    use opentelemetry_otlp::{RetryPolicy, WithExportConfig, WithTonicConfig};
    use opentelemetry_sdk::trace::SpanExporter as _;
    use std::time::Duration;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let collector = Collector {
        fail_until: usize::MAX,
        ..Default::default()
    };
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(traces::trace_service_server::TraceServiceServer::new(
                collector.clone(),
            ))
            .serve_with_incoming_shutdown(
                tokio_stream::wrappers::TcpListenerStream::new(listener),
                async {
                    let _ = stop_rx.await;
                },
            ),
    );
    let mut metadata = tonic::metadata::MetadataMap::new();
    metadata.insert("authorization", "Basic dGVzdDp0ZXN0".parse().unwrap());
    let policy = RetryPolicy::recommended()
        .with_max_retries(2)
        .with_initial_delay(Duration::from_millis(1))
        .with_max_delay(Duration::from_millis(4))
        .with_max_jitter(Duration::ZERO);
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .with_metadata(metadata)
        .with_timeout(Duration::from_secs(3))
        .with_retry_policy(policy.clone())
        .build()
        .unwrap();
    assert!(exporter.export(vec![]).await.is_err());
    assert_eq!(collector.attempts[0].load(Ordering::SeqCst), 3);

    let unauthorized = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .with_timeout(Duration::from_secs(3))
        .with_retry_policy(policy)
        .build()
        .unwrap();
    assert!(unauthorized.export(vec![]).await.is_err());
    assert_eq!(
        collector.attempts[0].load(Ordering::SeqCst),
        4,
        "authentication failure must add only one attempt"
    );
    let _ = stop_tx.send(());
    server.await.unwrap().unwrap();
}

#[tonic::async_trait]
impl traces::trace_service_server::TraceService for Collector {
    async fn export(
        &self,
        request: Request<traces::ExportTraceServiceRequest>,
    ) -> Result<Response<traces::ExportTraceServiceResponse>, Status> {
        let attempt = self.attempts[0].fetch_add(1, Ordering::SeqCst);
        check_auth(&request)?;
        if attempt < self.fail_until {
            return Err(Status::unavailable("temporary receiver outage"));
        }
        self.traces.lock().unwrap().push(request.into_inner());
        Ok(Response::new(Default::default()))
    }
}
#[tonic::async_trait]
impl logs::logs_service_server::LogsService for Collector {
    async fn export(
        &self,
        request: Request<logs::ExportLogsServiceRequest>,
    ) -> Result<Response<logs::ExportLogsServiceResponse>, Status> {
        let attempt = self.attempts[1].fetch_add(1, Ordering::SeqCst);
        check_auth(&request)?;
        if attempt < self.fail_until {
            return Err(Status::unavailable("temporary receiver outage"));
        }
        self.logs.lock().unwrap().push(request.into_inner());
        Ok(Response::new(Default::default()))
    }
}
#[tonic::async_trait]
impl metrics::metrics_service_server::MetricsService for Collector {
    async fn export(
        &self,
        request: Request<metrics::ExportMetricsServiceRequest>,
    ) -> Result<Response<metrics::ExportMetricsServiceResponse>, Status> {
        let attempt = self.attempts[2].fetch_add(1, Ordering::SeqCst);
        check_auth(&request)?;
        if attempt < self.fail_until {
            return Err(Status::unavailable("temporary receiver outage"));
        }
        self.metrics.lock().unwrap().push(request.into_inner());
        Ok(Response::new(Default::default()))
    }
}

/// Shutdown is the synchronization point: no sleeps or export-interval polling.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exports_all_signals_and_preserves_remote_command_parent() {
    // Exercise the real global subscriber without mutating this process's environment
    // or inheriting a developer's RUST_LOG=warn/off setting.
    if std::env::var_os("WAKEY_OTLP_TEST_CHILD").is_none() {
        for retry in ["0", "1"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "exports_all_signals_and_preserves_remote_command_parent",
                    "--nocapture",
                ])
                .env("WAKEY_OTLP_TEST_CHILD", "1")
                .env("WAKEY_OTLP_TEST_RETRY", retry)
                .env("RUST_LOG", "info")
                .env("WAKEY_OTLP_TEST_AUTH", "Basic dGVzdDp0ZXN0")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            for console in [&output.stdout, &output.stderr] {
                assert!(
                    !String::from_utf8_lossy(console)
                        .contains("debug log exported without console output")
                );
            }
        }
        return;
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let retry = std::env::var("WAKEY_OTLP_TEST_RETRY").as_deref() == Ok("1");
    let collector = Collector {
        fail_until: usize::from(retry),
        ..Default::default()
    };
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let server = tonic::transport::Server::builder()
        .add_service(traces::trace_service_server::TraceServiceServer::new(
            collector.clone(),
        ))
        .add_service(logs::logs_service_server::LogsServiceServer::new(
            collector.clone(),
        ))
        .add_service(metrics::metrics_service_server::MetricsServiceServer::new(
            collector.clone(),
        ))
        .serve_with_incoming_shutdown(
            tokio_stream::wrappers::TcpListenerStream::new(listener),
            async {
                let _ = stop_rx.await;
            },
        );
    let server = tokio::spawn(server);
    // The exporter contract is OS-independent; Linux procfs collection is
    // tested in wakey-linux rather than imported into core.
    struct ProcessSample;
    impl telemetry::ProcessMetricsSource for ProcessSample {
        fn process_uptime_seconds(&self) -> Option<f64> {
            Some(120.5)
        }
        fn resident_memory_bytes(&self) -> Option<u64> {
            Some(32 * 1024 * 1024)
        }
        fn host_uptime_seconds(&self) -> Option<f64> {
            Some(10_000.0)
        }
        fn cpu_time_seconds(&self) -> Option<f64> {
            Some(1.25)
        }
        fn open_file_descriptors(&self) -> Option<u64> {
            Some(12)
        }
        fn host_memory(&self) -> Option<telemetry::HostMemory> {
            Some(telemetry::HostMemory {
                total_bytes: 2 * 1024 * 1024 * 1024,
                available_bytes: Some(512 * 1024 * 1024),
            })
        }
        fn host_load(&self) -> Option<telemetry::HostLoad> {
            Some(telemetry::HostLoad {
                one_minute: 0.5,
                five_minutes: 1.0,
                fifteen_minutes: 1.5,
            })
        }
    }
    let guard = telemetry::init(
        &TelemetryConfig {
            otlp_endpoint: Some(endpoint),
            authorization_env: Some("WAKEY_OTLP_TEST_AUTH".into()),
            service_name: "wakey-test".into(),
            json_logs: false,
            console_filter: Some("info".into()),
            otlp_log_filter: Some("debug".into()),
            trace_filter: Some("info,opentelemetry_sdk=debug,h2=debug,hyper=debug,hyper_util=debug,tonic=debug,tower=debug".into()),
        },
        "unused",
        "info",
        Some("test-agent"),
    )
    .unwrap()
    .with_process_metrics(ProcessSample);

    let fleet_metrics = telemetry::FleetMetrics::new().unwrap();
    fleet_metrics.replace(Some(telemetry::FleetCounts {
        agents: [(
            "removed-agent".into(),
            telemetry::AgentDeviceCounts {
                devices: 9,
                endpoints: 20,
            },
        )]
        .into(),
        saved: 9,
        unsaved: 0,
    }));
    fleet_metrics.replace(None);
    fleet_metrics.replace(Some(telemetry::FleetCounts {
        agents: [
            (
                "router".into(),
                telemetry::AgentDeviceCounts {
                    devices: 2,
                    endpoints: 7,
                },
            ),
            ("empty-agent".into(), Default::default()),
        ]
        .into(),
        saved: 1,
        unsaved: 1,
    }));

    let parent = tracing::info_span!("relay_command");
    let context = parent.in_scope(telemetry::current_trace_context);
    assert!(context.contains_key("traceparent"));
    let child = tracing::info_span!("execute_command");
    telemetry::set_remote_parent(&child, &context);
    child.in_scope(|| {
        tracing::info!(command = "inventory", "command complete");
        let mut operation = telemetry::Operation::new("inventory");
        operation.outcome("ok");
        telemetry::count(telemetry::Counter::SnapshotDevices, 3);
        let _active = telemetry::ActiveTerminal::new();
    });
    drop(child);
    drop(parent);
    tracing::debug!("debug log exported without console output");
    let hidden = tracing::debug_span!("debug_span_not_exported");
    drop(hidden);
    tracing::warn!(target: "opentelemetry_sdk", "synthetic exporter failure remains local");
    macro_rules! transport_activity {
        ($target:literal) => {{
            let span = tracing::debug_span!(target: $target, "exporter_transport_span");
            span.in_scope(|| tracing::debug!(target: $target, "synthetic exporter transport event"));
        }};
    }
    transport_activity!("opentelemetry_sdk");
    transport_activity!("h2::proto");
    transport_activity!("hyper::client");
    transport_activity!("hyper_util::client");
    transport_activity!("tonic::transport");
    transport_activity!("tower::buffer");
    // A similarly named application target is not transport infrastructure.
    tracing::info!(target: "tower_device", "application target remains exported");
    let invalid_parent = tracing::info_span!("invalid_remote_parent");
    telemetry::set_remote_parent(
        &invalid_parent,
        &[("traceparent".into(), "malformed".into())].into(),
    );
    drop(invalid_parent);
    tokio::task::spawn_blocking(move || guard.shutdown())
        .await
        .unwrap();
    let _ = stop_tx.send(());
    server.await.unwrap().unwrap();

    for attempts in &collector.attempts {
        assert!(
            attempts.load(Ordering::SeqCst) > usize::from(retry),
            "each signal must recover from its first failed export"
        );
    }

    let traces = collector.traces.lock().unwrap();
    let spans: Vec<_> = traces
        .iter()
        .flat_map(|r| &r.resource_spans)
        .flat_map(|r| &r.scope_spans)
        .flat_map(|s| &s.spans)
        .collect();
    let parent = spans.iter().find(|s| s.name == "relay_command").unwrap();
    let child = spans.iter().find(|s| s.name == "execute_command").unwrap();
    assert_eq!(parent.trace_id, child.trace_id);
    assert_eq!(parent.span_id, child.parent_span_id);
    let invalid_parent = spans
        .iter()
        .find(|s| s.name == "invalid_remote_parent")
        .unwrap();
    assert!(invalid_parent.parent_span_id.is_empty());
    assert_ne!(parent.trace_id, invalid_parent.trace_id);
    let logs = collector.logs.lock().unwrap();
    assert!(
        !spans
            .iter()
            .any(|span| span.name == "exporter_transport_span"),
        "exporter transport spans must not feed back into OTLP"
    );
    assert!(
        !spans
            .iter()
            .any(|span| span.name == "debug_span_not_exported")
    );
    let records: Vec<_> = logs
        .iter()
        .flat_map(|r| &r.resource_logs)
        .flat_map(|r| &r.scope_logs)
        .flat_map(|s| &s.log_records)
        .collect();
    assert!(
        !records
            .iter()
            .any(|r| format!("{:?}", r.body).contains("synthetic exporter transport event")),
        "exporter transport events must not feed back into OTLP"
    );
    assert!(
        records
            .iter()
            .any(|r| format!("{:?}", r.body).contains("application target remains exported"))
    );
    assert!(
        records
            .iter()
            .any(|r| r.trace_id == child.trace_id && r.span_id == child.span_id)
    );
    assert!(
        !records
            .iter()
            .any(|r| format!("{:?}", r.body).contains("synthetic exporter failure"))
    );
    assert!(
        records
            .iter()
            .any(|r| format!("{:?}", r.body).contains("debug log exported without console output"))
    );
    let metrics = collector.metrics.lock().unwrap();
    let names: Vec<_> = metrics
        .iter()
        .flat_map(|r| &r.resource_metrics)
        .flat_map(|r| &r.scope_metrics)
        .flat_map(|s| &s.metrics)
        .map(|m| m.name.as_str())
        .collect();
    for name in [
        "wakey.operations",
        "wakey.operation.duration",
        "wakey.snapshot.devices",
        "wakey.terminal.active",
        "wakey.stored.devices",
        "wakey.stored.endpoints",
        "wakey.fleet.devices",
    ] {
        assert!(names.contains(&name), "missing {name}");
    }
    let latest = metrics.last().unwrap();
    {
        let duration = latest
            .resource_metrics
            .iter()
            .flat_map(|r| &r.scope_metrics)
            .flat_map(|s| &s.metrics)
            .find(|m| m.name == "wakey.operation.duration")
            .unwrap();
        assert_eq!(duration.unit, "s");
        let Some(metric::Data::Histogram(histogram)) = &duration.data else {
            panic!("operation duration must export a histogram");
        };
        let point = histogram.data_points.first().unwrap();
        assert_eq!(
            point.explicit_bounds,
            vec![
                0.0001, 0.0005, 0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0,
                10.0, 30.0, 60.0,
            ]
        );
        // One inventory operation is recorded above. Derive its expected bucket
        // from the exported sum, avoiding elapsed-time thresholds or sleeps.
        assert_eq!(point.count, 1);
        let sample = point.sum.unwrap();
        let bucket = point
            .explicit_bounds
            .partition_point(|bound| *bound < sample);
        let mut expected = vec![0; point.explicit_bounds.len() + 1];
        expected[bucket] = 1;
        assert_eq!(point.bucket_counts, expected);
    }
    {
        for (name, unit, expected) in [
            (
                "wakey.process.uptime",
                "s",
                number_data_point::Value::AsDouble(120.5),
            ),
            (
                "wakey.process.memory.resident",
                "By",
                number_data_point::Value::AsInt(32 * 1024 * 1024),
            ),
            (
                "wakey.host.uptime",
                "s",
                number_data_point::Value::AsDouble(10_000.0),
            ),
            (
                "wakey.process.open_fds",
                "",
                number_data_point::Value::AsInt(12),
            ),
            (
                "wakey.host.memory.total",
                "By",
                number_data_point::Value::AsInt(2 * 1024 * 1024 * 1024),
            ),
            (
                "wakey.host.memory.available",
                "By",
                number_data_point::Value::AsInt(512 * 1024 * 1024),
            ),
        ] {
            let metric = latest
                .resource_metrics
                .iter()
                .flat_map(|resource| &resource.scope_metrics)
                .flat_map(|scope| &scope.metrics)
                .find(|metric| metric.name == name)
                .unwrap_or_else(|| panic!("missing {name}"));
            assert_eq!(metric.unit, unit);
            let Some(metric::Data::Gauge(gauge)) = &metric.data else {
                panic!("{name} must be a gauge");
            };
            assert_eq!(gauge.data_points.len(), 1);
            let point = &gauge.data_points[0];
            assert!(
                point.attributes.is_empty(),
                "process metrics need no PID/host/address labels"
            );
            assert_eq!(point.value, Some(expected));
        }
        let cpu = latest
            .resource_metrics
            .iter()
            .flat_map(|resource| &resource.scope_metrics)
            .flat_map(|scope| &scope.metrics)
            .find(|metric| metric.name == "wakey.process.cpu.time")
            .expect("CPU time exported");
        assert_eq!(cpu.unit, "s");
        let Some(metric::Data::Sum(sum)) = &cpu.data else {
            panic!("CPU time must be a counter, not a gauge");
        };
        assert!(sum.is_monotonic);
        assert_eq!(sum.data_points.len(), 1);
        assert_eq!(
            sum.data_points[0].value,
            Some(number_data_point::Value::AsDouble(1.25))
        );
        assert!(sum.data_points[0].attributes.is_empty());
        let load = latest
            .resource_metrics
            .iter()
            .flat_map(|resource| &resource.scope_metrics)
            .flat_map(|scope| &scope.metrics)
            .find(|metric| metric.name == "wakey.host.load")
            .expect("host load exported");
        let Some(metric::Data::Gauge(gauge)) = &load.data else {
            panic!("host load must be a gauge");
        };
        assert_eq!(gauge.data_points.len(), 3);
        for (period, expected) in [("1m", 0.5), ("5m", 1.0), ("15m", 1.5)] {
            let point = gauge.data_points.iter().find(|point| point.attributes.iter().any(|attribute| {
                attribute.key == "period" && attribute.value.as_ref().and_then(|value| value.value.as_ref())
                    == Some(&opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue(period.into()))
            })).expect("load period exported");
            assert_eq!(point.attributes.len(), 1, "only fixed period labels");
            assert_eq!(
                point.value,
                Some(number_data_point::Value::AsDouble(expected))
            );
        }
    }
    for (name, label, expected) in [
        (
            "wakey.stored.devices",
            "agent_id",
            vec![("router", 2), ("empty-agent", 0)],
        ),
        (
            "wakey.stored.endpoints",
            "agent_id",
            vec![("router", 7), ("empty-agent", 0)],
        ),
        (
            "wakey.fleet.devices",
            "identity",
            vec![("saved", 1), ("unsaved", 1)],
        ),
    ] {
        let metric = latest
            .resource_metrics
            .iter()
            .flat_map(|r| &r.scope_metrics)
            .flat_map(|s| &s.metrics)
            .find(|m| m.name == name)
            .unwrap();
        let Some(metric::Data::Gauge(gauge)) = &metric.data else {
            panic!("{name} must be a gauge");
        };
        assert_eq!(gauge.data_points.len(), expected.len());
        for (value, count) in expected {
            let point = gauge.data_points.iter().find(|p| p.attributes.iter().any(|a| {
                a.key == label && a.value.as_ref().and_then(|v| v.value.as_ref())
                    == Some(&opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue(value.into()))
            })).unwrap();
            assert_eq!(point.value, Some(number_data_point::Value::AsInt(count)));
            assert_eq!(point.attributes.len(), 1, "no device/address labels");
        }
    }
}
