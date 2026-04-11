use anyhow::{Context, Result};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::global;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::{SdkTracerProvider, Tracer};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::TelemetryConfig;

pub fn init(verbose: u8, telemetry: &TelemetryConfig) -> Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(default_filter(verbose)))
        .expect("static tracing filter should parse");

    let otel = build_otel_layer(telemetry)?;

    if telemetry.json_logs {
        if let Some(otel_layer) = otel {
            tracing_subscriber::registry()
                .with(otel_layer)
                .with(filter)
                .with(fmt::layer().json())
                .init();
            tracing::info!(endpoint = ?telemetry.otlp_endpoint, json_logs = telemetry.json_logs, "tracing initialized with otlp exporter");
        } else {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().json())
                .init();
            tracing::info!(json_logs = telemetry.json_logs, "tracing initialized without otlp exporter");
        }
    } else if let Some(otel_layer) = otel {
        tracing_subscriber::registry()
            .with(otel_layer)
            .with(filter)
            .with(fmt::layer())
            .init();
        tracing::info!(endpoint = ?telemetry.otlp_endpoint, json_logs = telemetry.json_logs, "tracing initialized with otlp exporter");
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer())
            .init();
        tracing::info!(json_logs = telemetry.json_logs, "tracing initialized without otlp exporter");
    }

    Ok(())
}

fn build_otel_layer(
    telemetry: &TelemetryConfig,
) -> Result<
    Option<
        tracing_opentelemetry::OpenTelemetryLayer<
            tracing_subscriber::Registry,
            Tracer,
        >,
    >,
> {
    let Some(endpoint) = telemetry.otlp_endpoint.as_deref() else {
        return Ok(None);
    };

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()
        .context("failed to build OTLP span exporter")?;

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            Resource::builder_empty()
                .with_service_name(telemetry.service_name.clone())
                .build(),
        )
        .build();

    let tracer = provider.tracer(telemetry.service_name.clone());
    global::set_tracer_provider(provider);
    Ok(Some(tracing_opentelemetry::layer().with_tracer(tracer)))
}

fn default_filter(verbose: u8) -> &'static str {
    match verbose {
        0 => "wakey_control_plane=info",
        1 => "wakey_control_plane=debug",
        _ => "wakey_control_plane=trace",
    }
}
