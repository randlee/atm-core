//! Native Tokio SDK construction. Bootstrap owns the returned providers.

use std::sync::Arc;
use std::time::Duration;

use atm_core::TelemetryExportConfig;
use atm_runtime::task_telemetry::{TaskTelemetryConfig, TaskTelemetrySetup};
use atm_runtime::workflow_telemetry::{WorkflowTelemetryConfig, WorkflowTelemetrySetup};
use opentelemetry::metrics::MeterProvider;
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::{Compression, WithExportConfig, WithTonicConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::log_processor_with_async_runtime::BatchLogProcessor;
use opentelemetry_sdk::logs::{BatchConfigBuilder as LogBatchConfigBuilder, SdkLoggerProvider};
use opentelemetry_sdk::metrics::periodic_reader_with_async_runtime::PeriodicReader;
use opentelemetry_sdk::metrics::{SdkMeterProvider, Temporality};
use opentelemetry_sdk::runtime::Tokio;
use opentelemetry_sdk::trace::span_processor_with_async_runtime::BatchSpanProcessor;
use opentelemetry_sdk::trace::{BatchConfigBuilder, Sampler, SdkTracerProvider};
use tonic::metadata::{Ascii, MetadataValue};
use tonic::transport::{ClientTlsConfig, Endpoint};

/// Production per-export bound of every SDK processor.
pub(crate) const EXPORT_TIMEOUT: Duration = Duration::from_millis(400);
/// Production tonic transport bound. It is offset below `EXPORT_TIMEOUT` so a
/// stalled export always ends as a transport timeout (`TimedOut`); equal
/// bounds left the SDK and tonic timers racing, so the recorded failure shape
/// depended on the scheduler. The SDK bound remains the backstop.
///
/// Trade-off: this is also the transport's connect timeout (see
/// `lazy_channel`), so a cold https connection (DNS, TCP and TLS handshake)
/// slower than this fails the first export, visible only as diagnostics. A
/// longer bound would let a stalled collector hold the process shutdown
/// deadline, which bootstrap shares across every provider; ATM chooses bounded
/// shutdown over first-export reliability against a slow collector.
pub(crate) const EXPORT_TRANSPORT_TIMEOUT: Duration = Duration::from_millis(300);
pub(crate) const EXPORT_QUEUE: usize = 256;
pub(crate) const EXPORT_BATCH: usize = 256;
pub(crate) const EXPORT_INTERVAL: Duration = Duration::from_secs(1);

/// Existing runtime setup values plus unwrapped, standard SDK providers.
pub type TelemetrySetup = (
    TaskTelemetrySetup,
    WorkflowTelemetrySetup,
    SdkTracerProvider,
    SdkLoggerProvider,
    SdkMeterProvider,
);

/// Constructs all three native asynchronous signals on the current Tokio
/// runtime. No globals, extra queue or lifecycle handle are installed.
///
/// Quiescent non-full trace/log queues need at most two 400ms exports. Metric
/// export likewise needs at most an in-flight plus final export. Bootstrap
/// runs provider shutdowns concurrently on the blocking pool under one shared
/// remaining deadline (at most 1s). A timeout abandons the wait, not the call;
/// only runtime teardown drops the SDK tasks and releases blocked receivers.
///
/// # Errors
/// Returns the existing safe ATM configuration error. Native source types are
/// retained privately in diagnostics because they may contain credentials.
pub fn setup_telemetry(
    config: &TelemetryExportConfig,
    diagnostics: &crate::ExportDiagnostics,
) -> Result<TelemetrySetup, atm_core::error::AtmError> {
    setup_with_timeouts(
        config,
        EXPORT_BATCH,
        EXPORT_TRANSPORT_TIMEOUT,
        EXPORT_TIMEOUT,
    )
    .map_err(|source| {
        diagnostics.setup_failed(source);
        atm_core::error::AtmError::new(
            atm_core::error::AtmErrorCode::TelemetryExportConfigInvalid,
            "native OpenTelemetry setup failed; check ATM_OTEL_ENDPOINT and ATM_OTEL_AUTH_HEADER",
        )
    })
}

#[cfg(test)]
pub(crate) fn setup_with_limits(
    config: &TelemetryExportConfig,
    batch: usize,
    timeout: Duration,
) -> Result<TelemetrySetup, Box<dyn std::error::Error + Send + Sync>> {
    setup_with_timeouts(config, batch, timeout, timeout)
}

/// Production gives the tonic transport a bound below the SDK processors' so
/// exactly one timer decides a stalled export; tests do the same with their
/// own bounds.
pub(crate) fn setup_with_timeouts(
    config: &TelemetryExportConfig,
    batch: usize,
    transport_timeout: Duration,
    timeout: Duration,
) -> Result<TelemetrySetup, Box<dyn std::error::Error + Send + Sync>> {
    tokio::runtime::Handle::try_current()?;
    let channel = lazy_channel(config, transport_timeout)?;
    let auth: Option<MetadataValue<Ascii>> = config.auth_header().map(str::parse).transpose()?;
    // The upstream builder merges ambient OTEL headers even with metadata
    // supplied. Its standard interceptor runs after that merge: replace the
    // map with ATM's validated metadata so no ambient credential is exported.
    let interceptor = move |mut request: tonic::Request<()>| {
        request.metadata_mut().clear();
        if let Some(auth) = &auth {
            request.metadata_mut().insert("authorization", auth.clone());
        }
        Ok(request)
    };
    // Explicit gzip also prevents the upstream builder consulting ambient
    // compression settings; gRPC remains the only transport.
    macro_rules! exporter {
        ($builder:expr) => {
            $builder
                .with_tonic()
                .with_channel(channel.clone())
                .with_timeout(transport_timeout)
                .with_compression(Compression::Gzip)
                .with_interceptor(interceptor.clone())
                .build()?
        };
    }
    // Build fallible exporters first, before starting any SDK worker.
    let spans = exporter!(opentelemetry_otlp::SpanExporter::builder());
    let logs = exporter!(opentelemetry_otlp::LogExporter::builder());
    let metrics = exporter!(
        opentelemetry_otlp::MetricExporter::builder().with_temporality(Temporality::Cumulative)
    );
    let resource = Resource::builder_empty()
        .with_service_name(config.service_name().to_owned())
        .build();
    let tracer = tracer_provider(spans, resource.clone(), batch, timeout);
    let logger = SdkLoggerProvider::builder()
        .with_resource(resource.clone())
        .with_log_processor(
            BatchLogProcessor::builder(logs, Tokio)
                .with_batch_config(
                    LogBatchConfigBuilder::default()
                        .with_max_queue_size(EXPORT_QUEUE)
                        .with_max_export_batch_size(batch)
                        .with_max_export_timeout(timeout)
                        .with_scheduled_delay(EXPORT_INTERVAL)
                        .build(),
                )
                .build(),
        )
        .build();
    let meter = meter_provider(metrics, resource, timeout);
    let sink = Arc::new(crate::task_exporter::TaskExporter::new(
        tracer.tracer("atm.task"),
        meter.meter("atm.task"),
    ));
    Ok((
        TaskTelemetrySetup {
            config: TaskTelemetryConfig::default(),
            sink: sink.clone(),
        },
        WorkflowTelemetrySetup {
            config: WorkflowTelemetryConfig::default(),
            sink,
        },
        tracer,
        logger,
        meter,
    ))
}

/// Lazily connected gRPC channel with the transport-level request and connect
/// timeouts applied; both use the transport bound (see `EXPORT_TRANSPORT_TIMEOUT`).
fn lazy_channel(
    config: &TelemetryExportConfig,
    transport_timeout: Duration,
) -> Result<tonic::transport::Channel, Box<dyn std::error::Error + Send + Sync>> {
    let mut endpoint = Endpoint::from_shared(config.endpoint().to_owned())?
        .timeout(transport_timeout)
        .connect_timeout(transport_timeout);
    if config.endpoint().starts_with("https://") {
        endpoint = endpoint.tls_config(ClientTlsConfig::new().with_native_roots())?;
    }
    Ok(endpoint.connect_lazy())
}

fn tracer_provider(
    spans: opentelemetry_otlp::SpanExporter,
    resource: Resource,
    batch: usize,
    timeout: Duration,
) -> SdkTracerProvider {
    SdkTracerProvider::builder()
        .with_id_generator(crate::task_exporter::DurableIds)
        .with_resource(resource)
        .with_sampler(Sampler::AlwaysOn)
        .with_max_events_per_span(64)
        .with_max_attributes_per_span(32)
        .with_max_attributes_per_event(24)
        .with_max_links_per_span(0)
        .with_max_attributes_per_link(0)
        .with_span_processor(
            BatchSpanProcessor::builder(spans, Tokio)
                .with_batch_config(
                    BatchConfigBuilder::default()
                        .with_max_queue_size(EXPORT_QUEUE)
                        .with_max_export_batch_size(batch)
                        .with_max_concurrent_exports(1)
                        .with_max_export_timeout(timeout)
                        .with_scheduled_delay(EXPORT_INTERVAL)
                        .build(),
                )
                .build(),
        )
        .build()
}

fn meter_provider(
    metrics: opentelemetry_otlp::MetricExporter,
    resource: Resource,
    timeout: Duration,
) -> SdkMeterProvider {
    SdkMeterProvider::builder()
        .with_resource(resource)
        .with_view(|instrument: &opentelemetry_sdk::metrics::Instrument| {
            instrument.name().starts_with("atm.task.").then(|| {
                opentelemetry_sdk::metrics::Stream::builder()
                    .with_cardinality_limit(32)
                    .build()
                    .expect("fixed positive cardinality")
            })
        })
        .with_reader(
            PeriodicReader::builder(metrics, Tokio)
                .with_interval(EXPORT_INTERVAL)
                .with_timeout(timeout)
                .build(),
        )
        .build()
}
