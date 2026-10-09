#![cfg(feature = "telemetry")]

use opentelemetry_proto::tonic::collector::{
    logs::v1 as logs, metrics::v1 as metrics, trace::v1 as traces,
};
use std::sync::{Arc, Mutex};
use tonic::{Request, Response, Status};
use wakey_core::telemetry::{self, TelemetryConfig};

#[derive(Clone, Default)]
struct Collector {
    traces: Arc<Mutex<Vec<traces::ExportTraceServiceRequest>>>,
    logs: Arc<Mutex<Vec<logs::ExportLogsServiceRequest>>>,
    metrics: Arc<Mutex<Vec<metrics::ExportMetricsServiceRequest>>>,
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

#[tonic::async_trait]
impl traces::trace_service_server::TraceService for Collector {
    async fn export(
        &self,
        request: Request<traces::ExportTraceServiceRequest>,
    ) -> Result<Response<traces::ExportTraceServiceResponse>, Status> {
        check_auth(&request)?;
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
        check_auth(&request)?;
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
        check_auth(&request)?;
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
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "exports_all_signals_and_preserves_remote_command_parent",
                "--nocapture",
            ])
            .env("WAKEY_OTLP_TEST_CHILD", "1")
            .env("RUST_LOG", "info")
            .env("WAKEY_OTLP_TEST_AUTH", "Basic dGVzdDp0ZXN0")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let collector = Collector::default();
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
    let guard = telemetry::init(
        &TelemetryConfig {
            otlp_endpoint: Some(endpoint),
            authorization_env: Some("WAKEY_OTLP_TEST_AUTH".into()),
            service_name: "wakey-test".into(),
            json_logs: false,
        },
        "unused",
        "info",
        Some("test-agent"),
    )
    .unwrap();

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
    tracing::warn!(target: "opentelemetry_sdk", "synthetic exporter failure remains local");
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
    let records: Vec<_> = logs
        .iter()
        .flat_map(|r| &r.resource_logs)
        .flat_map(|r| &r.scope_logs)
        .flat_map(|s| &s.log_records)
        .collect();
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
    ] {
        assert!(names.contains(&name), "missing {name}");
    }
}
