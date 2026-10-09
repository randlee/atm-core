//! Native Tokio SDK construction. Bootstrap owns the returned providers.

use std::sync::Arc;
use std::time::Duration;

use atm_core::TelemetryExportConfig;
use atm_runtime::task_telemetry::TaskTelemetrySetup;
use opentelemetry::logs::LoggerProvider;
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
/// stalled export always ends as a tonic transport timeout, which the SDK reports
/// as an internal failure and the diagnostics record as `Unavailable`; equal
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
    .map_err(|_source| {
        diagnostics.setup_failed();
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
    let (channel, interceptor) = grpc_transport(config, transport_timeout)?;
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
    let logger = logger_provider(logs, resource.clone(), batch, timeout);
    let meter = meter_provider(metrics, resource, timeout)?;
    let sink = Arc::new(crate::task_exporter::TaskExporter::new(
        tracer.tracer("atm.task"),
        meter.meter("atm.task"),
    ));
    Ok((TaskTelemetrySetup { sink }, tracer, logger, meter))
}

/// The validated gRPC channel plus the interceptor that replaces the request
/// metadata with ATM's own. The upstream builder merges ambient OTEL headers
/// even with metadata supplied; its standard interceptor runs after that
/// merge, so no ambient credential is exported.
#[allow(
    clippy::type_complexity,
    reason = "the interceptor closure type is unnameable"
)]
fn grpc_transport(
    config: &TelemetryExportConfig,
    transport_timeout: Duration,
) -> Result<
    (
        tonic::transport::Channel,
        impl FnMut(tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status>
        + Clone
        + Send
        + Sync
        + 'static,
    ),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let channel = lazy_channel(config, transport_timeout)?;
    let auth: Option<MetadataValue<Ascii>> = config.auth_header().map(str::parse).transpose()?;
    let interceptor = move |mut request: tonic::Request<()>| {
        request.metadata_mut().clear();
        if let Some(auth) = &auth {
            request.metadata_mut().insert("authorization", auth.clone());
        }
        Ok(request)
    };
    Ok((channel, interceptor))
}

fn logger_provider(
    logs: opentelemetry_otlp::LogExporter,
    resource: Resource,
    batch: usize,
    timeout: Duration,
) -> SdkLoggerProvider {
    SdkLoggerProvider::builder()
        .with_resource(resource)
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
        .build()
}

/// The routed log destination of a short-lived process (the `atm` CLI) and,
/// for `otel` or `both`, its one native SDK log provider on the current Tokio
/// runtime. It shares [`atm_core::LogDestination::from_env`],
/// [`TelemetryExportConfig::from_env`] and the daemon's transport and batch
/// bounds; it builds no trace or metric provider.
#[derive(Debug)]
pub struct LogExport {
    destination: atm_core::LogDestination,
    provider: Option<SdkLoggerProvider>,
    config_invalid: bool,
}

impl LogExport {
    /// Resolves the destination the daemon would use from `env`. Invalid
    /// configuration or failed SDK setup selects file logging, as the daemon
    /// does, and is reported by [`Self::config_invalid`].
    #[must_use]
    pub fn from_env(env: &dyn atm_core::atm_temp::EnvSource) -> Self {
        let Ok(destination) = atm_core::LogDestination::from_env(env) else {
            return Self::invalid();
        };
        if destination == atm_core::LogDestination::File {
            return Self {
                destination,
                provider: None,
                config_invalid: false,
            };
        }
        // `otel` and `both` already required an endpoint, so a configuration
        // is present unless it is invalid.
        let Ok(Some(config)) = TelemetryExportConfig::from_env(env) else {
            return Self::invalid();
        };
        match log_provider(&config) {
            Ok(provider) => Self {
                destination,
                provider: Some(provider),
                config_invalid: false,
            },
            Err(_) => Self::invalid(),
        }
    }

    fn invalid() -> Self {
        Self {
            destination: atm_core::LogDestination::File,
            provider: None,
            config_invalid: true,
        }
    }

    /// Whether invalid configuration or failed SDK setup selected file
    /// logging in place of the configured destination.
    #[must_use]
    pub fn config_invalid(&self) -> bool {
        self.config_invalid
    }

    /// The destination records are routed to.
    #[must_use]
    pub fn destination(&self) -> atm_core::LogDestination {
        self.destination
    }

    /// Registers the OTel log sink on `builder` when the destination includes
    /// OTel; a file-only export registers nothing.
    pub fn register_sink(&self, builder: &mut sc_observability::v2::LoggerBuilder) {
        if let Some(provider) = &self.provider {
            builder.register_sink(sc_observability::SinkRegistration::typed(Arc::new(
                crate::otel_logs::OtelLogSink::new(provider.logger("atm")),
            )));
        }
    }

    /// Exports every admitted record and stops the provider, waiting at most
    /// `bound`. The blocking SDK shutdown runs on the blocking pool so a
    /// current-thread runtime keeps driving the batch worker it waits for.
    pub async fn shutdown(self, bound: Duration) {
        if let Some(provider) = self.provider {
            let shutdown = tokio::task::spawn_blocking(move || provider.shutdown());
            drop(tokio::time::timeout(bound, shutdown).await);
        }
    }
}

fn log_provider(
    config: &TelemetryExportConfig,
) -> Result<SdkLoggerProvider, Box<dyn std::error::Error + Send + Sync>> {
    tokio::runtime::Handle::try_current()?;
    let (channel, interceptor) = grpc_transport(config, EXPORT_TRANSPORT_TIMEOUT)?;
    let logs = opentelemetry_otlp::LogExporter::builder()
        .with_tonic()
        .with_channel(channel)
        .with_timeout(EXPORT_TRANSPORT_TIMEOUT)
        .with_compression(Compression::Gzip)
        .with_interceptor(interceptor)
        .build()?;
    let resource = Resource::builder_empty()
        .with_service_name(config.service_name().to_owned())
        .build();
    Ok(logger_provider(
        logs,
        resource,
        EXPORT_BATCH,
        EXPORT_TIMEOUT,
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
    if uses_tls(config.endpoint()) {
        endpoint = endpoint.tls_config(ClientTlsConfig::new().with_native_roots())?;
    }
    Ok(endpoint.connect_lazy())
}

/// Whether the endpoint scheme is `https`, in any letter case, as
/// `parse_endpoint` accepts it.
pub(crate) fn uses_tls(endpoint: &str) -> bool {
    endpoint
        .get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
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

/// Per-instrument series cap for the `atm.task.*` metrics.
const TASK_METRIC_CARDINALITY: usize = 32;

/// The SDK refused to build the cardinality-capped metric stream.
#[derive(Debug)]
pub(crate) struct MetricStreamError(String);

impl std::fmt::Display for MetricStreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid task metric stream: {}", self.0)
    }
}

impl std::error::Error for MetricStreamError {}

pub(crate) fn task_stream(
    cardinality_limit: usize,
) -> Result<opentelemetry_sdk::metrics::Stream, MetricStreamError> {
    opentelemetry_sdk::metrics::Stream::builder()
        .with_cardinality_limit(cardinality_limit)
        .build()
        .map_err(|source| MetricStreamError(source.to_string()))
}

fn meter_provider(
    metrics: opentelemetry_otlp::MetricExporter,
    resource: Resource,
    timeout: Duration,
) -> Result<SdkMeterProvider, MetricStreamError> {
    // Validate once so a builder failure propagates through setup; the view
    // below rebuilds the same stream per instrument and cannot then fail.
    task_stream(TASK_METRIC_CARDINALITY)?;
    Ok(SdkMeterProvider::builder()
        .with_resource(resource)
        .with_view(|instrument: &opentelemetry_sdk::metrics::Instrument| {
            instrument
                .name()
                .starts_with("atm.task.")
                .then(|| task_stream(TASK_METRIC_CARDINALITY).ok())
                .flatten()
        })
        .with_reader(
            PeriodicReader::builder(metrics, Tokio)
                .with_interval(EXPORT_INTERVAL)
                .with_timeout(timeout)
                .build(),
        )
        .build())
}
