#![cfg(test)]

mod ambient;
mod lifecycle;
mod receiver;
mod routing;

use std::sync::Arc;
use std::time::Duration;

use atm_core::test_support::FakeEnvSource;
use atm_core::{TaskTelemetryKind, TaskTelemetryRecord, TelemetryExportConfig};
use opentelemetry::logs::{LogRecord, Logger, LoggerProvider};
use opentelemetry::metrics::MeterProvider;
use opentelemetry::trace::{Span, Tracer, TracerProvider};
use opentelemetry_proto::tonic::metrics::v1::metric::Data;

use super::otel_setup::{setup_with_limits, setup_with_timeouts, task_stream};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::{SdkLoggerProvider, SimpleLogProcessor};
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::trace::SpanExporter as _;
use opentelemetry_sdk::trace::{Sampler, SdkTracerProvider, SimpleSpanProcessor};
use receiver::{Capture, Receiver, Signal, TestExporters};

const TASK_METRIC_CARDINALITY: usize = 32;

struct SuccessSetup {
    task: atm_runtime::task_telemetry::TaskTelemetrySetup,
    tracer: SdkTracerProvider,
    logger: SdkLoggerProvider,
    meter: SdkMeterProvider,
}

fn setup_success(
    config: &TelemetryExportConfig,
    _batch: usize,
    capture: Capture,
) -> Result<SuccessSetup, Box<dyn std::error::Error + Send + Sync>> {
    tokio::runtime::Handle::try_current()?;
    let resource = Resource::builder_empty()
        .with_service_name(config.service_name().to_owned())
        .build();
    let test_exporters = TestExporters(capture.clone());
    let tracer = SdkTracerProvider::builder()
        .with_id_generator(crate::task_exporter::DurableIds)
        .with_resource(resource.clone())
        .with_sampler(Sampler::AlwaysOn)
        .with_max_events_per_span(64)
        .with_max_attributes_per_span(32)
        .with_max_attributes_per_event(24)
        .with_max_links_per_span(0)
        .with_max_attributes_per_link(0)
        .with_span_processor(SimpleSpanProcessor::new(test_exporters.clone()))
        .build();
    let logger = SdkLoggerProvider::builder()
        .with_resource(resource.clone())
        .with_log_processor(SimpleLogProcessor::new(test_exporters.clone()))
        .build();
    task_stream(TASK_METRIC_CARDINALITY)?;
    let metric_exporter = test_exporters.clone();
    let meter = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_view(|instrument: &opentelemetry_sdk::metrics::Instrument| {
            instrument
                .name()
                .starts_with("atm.task.")
                .then(|| task_stream(TASK_METRIC_CARDINALITY).ok())
                .flatten()
        })
        // The test exporter returns synchronously after recording its event.
        // The long interval prevents an unrelated periodic tick; shutdown
        // explicitly collects the final metrics, with no timeout or transport.
        .with_reader(
            PeriodicReader::builder(metric_exporter)
                .with_interval(Duration::from_secs(3600))
                .build(),
        )
        .build();
    let task = atm_runtime::task_telemetry::TaskTelemetrySetup {
        sink: Arc::new(crate::task_exporter::TaskExporter::new(
            tracer.tracer("atm.task"),
            meter.meter("atm.task"),
        )),
    };
    Ok(SuccessSetup {
        task,
        tracer,
        logger,
        meter,
    })
}

fn config(endpoint: &str) -> TelemetryExportConfig {
    TelemetryExportConfig::from_env(&FakeEnvSource::new([
        ("ATM_OTEL_ENDPOINT", Some(endpoint)),
        ("ATM_OTEL_SERVICE_NAME", Some("atm-fixture")),
        ("ATM_OTEL_AUTH_HEADER", Some("Bearer fixture-auth")),
    ]))
    .unwrap()
    .unwrap()
}

#[test]
fn a_rejected_metric_stream_is_a_typed_error_not_a_panic() {
    let error = crate::otel_setup::task_stream(0).expect_err("zero cardinality is invalid");
    assert!(
        error.to_string().starts_with("invalid task metric stream:"),
        "{error}"
    );
    crate::otel_setup::task_stream(32).expect("the production limit builds");
}

#[test]
fn setup_without_tokio_returns_safe_error_instead_of_panicking() {
    let diagnostics = crate::ExportDiagnostics::default();
    let result = crate::setup_telemetry(&config("http://127.0.0.1:4317"), &diagnostics);
    assert!(result.is_err());
    assert!(!result.err().unwrap().to_string().contains("fixture-auth"));
}

pub(crate) fn record(
    task: &str,
    kind: TaskTelemetryKind,
    seq: u64,
    second: u64,
) -> TaskTelemetryRecord {
    serde_json::from_value(serde_json::json!({
        "kind": kind, "team": "telemetry-fixture", "task_id": task,
        "assignee": "worker", "actor": "daemon", "seq": seq,
        "at": format!("2026-01-01T00:00:{second:02}Z"), "from_state": null,
        "to_state": null, "close_outcome": null, "message_id": null,
        "reminder_outcome": null, "marker": null, "handoff": null,
    }))
    .unwrap()
}

async fn shutdown(setup: SuccessSetup) {
    let SuccessSetup {
        task,
        tracer,
        logger,
        meter,
    } = setup;
    drop(task);
    let results = tokio::join!(
        tokio::task::spawn_blocking(move || tracer.shutdown()),
        tokio::task::spawn_blocking(move || logger.shutdown()),
        tokio::task::spawn_blocking(move || meter.shutdown()),
    );
    results.0.unwrap().unwrap();
    results.1.unwrap().unwrap();
    results.2.unwrap().unwrap();
}

async fn shutdown_production(setup: super::otel_setup::TelemetrySetup) {
    let (task, tracer, logger, meter) = setup;
    drop(task);
    let results = tokio::join!(
        tokio::task::spawn_blocking(move || tracer.shutdown()),
        tokio::task::spawn_blocking(move || logger.shutdown()),
        tokio::task::spawn_blocking(move || meter.shutdown()),
    );
    results.0.unwrap().unwrap();
    results.1.unwrap().unwrap();
    results.2.unwrap().unwrap();
}

fn setup_production(config: &TelemetryExportConfig) -> super::otel_setup::TelemetrySetup {
    crate::setup_telemetry(config, &crate::ExportDiagnostics::default()).unwrap()
}

fn assert_transport_metadata(capture: &Capture) {
    for (signal, requests) in [
        ("trace", &capture.trace_metadata),
        ("metric", &capture.metric_metadata),
    ] {
        let requests = requests.lock().unwrap();
        assert!(
            !requests.is_empty(),
            "no production {signal} request received"
        );
        for metadata in requests.iter() {
            assert_eq!(
                metadata
                    .get("authorization")
                    .and_then(|value| value.to_str().ok()),
                Some("Bearer fixture-auth"),
                "production {signal} request must use ATM auth"
            );
            assert!(
                metadata.get("x-ambient").is_none(),
                "production {signal} request must omit ambient headers"
            );
        }
    }
}

#[tokio::test]
async fn event_wait_returns_exporter_error_and_receiver_exit_evidence() {
    let capture = Capture::default();
    capture.fail_exports("test exporter rejected the batch");
    let exporter = TestExporters(capture.clone());
    let error = exporter
        .export(Vec::new())
        .await
        .expect_err("injected export failure");
    assert!(
        error
            .to_string()
            .contains("test exporter rejected the batch")
    );
    let wait_error = capture
        .wait_result("trace export completion", || false)
        .await
        .expect_err("failed exporter must release the event wait");
    assert!(wait_error.contains("test exporter rejected the batch"));

    let receiver = Receiver::start(false).await;
    let capture = receiver.capture.clone();
    receiver.stop().await;
    let wait_error = capture
        .wait_result("receiver export completion", || false)
        .await
        .expect_err("receiver exit must release the event wait");
    assert!(wait_error.contains("receiver exited"));
}

async fn wait_for_task_exports(receiver: &Receiver, trace_before: usize, metric_before: usize) {
    receiver
        .capture
        .wait("the successful task span export to finish", || {
            receiver.capture.finished(Signal::Trace) > trace_before
        })
        .await;
    receiver
        .capture
        .wait("the successful task metric export to finish", || {
            receiver.capture.finished(Signal::Metric) > metric_before
        })
        .await;
}

#[tokio::test]
async fn every_task_kind_exports_its_typed_name_and_facts() {
    use TaskTelemetryKind::*;
    let kinds = [
        Assigned,
        Acked,
        Started,
        Reassigned,
        Reopened,
        Completed,
        Refused,
        Cancelled,
        Rejected,
        Reminded,
        LeadNotified,
        Moved,
        Migrated,
        RemindersReset,
        PromptHandoff,
    ];
    let receiver = Receiver::start(false).await;
    let trace_before = receiver.capture.finished(Signal::Trace);
    let metric_before = receiver.capture.finished(Signal::Metric);
    let setup = setup_success(&config(&receiver.endpoint), 64, receiver.capture.clone()).unwrap();
    for (index, kind) in kinds.into_iter().enumerate() {
        setup
            .task
            .sink
            .emit(record("all-kinds", kind, index as u64 + 1, 1))
            .unwrap();
    }
    shutdown(setup).await;
    wait_for_task_exports(&receiver, trace_before, metric_before).await;
    {
        let spans = receiver.capture.spans.lock().unwrap();
        let events: Vec<_> = spans
            .iter()
            .filter(|span| span.name == "atm.task.event")
            .collect();
        assert_eq!(events.len(), kinds.len());
        for (event, kind) in events.iter().zip(kinds) {
            let fact = event
                .attributes
                .iter()
                .find(|attribute| attribute.key == "atm.task.kind")
                .unwrap();
            assert_eq!(
                fact.value.as_ref().unwrap().value,
                Some(
                    opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue(
                        kind.as_str().to_owned()
                    )
                )
            );
            assert_eq!(event.start_time_unix_nano, event.end_time_unix_nano);
        }
    }
    receiver.stop().await;
}

#[tokio::test]
async fn receiver_observes_live_durable_spans_metrics_and_dedup() {
    let receiver = Receiver::start(false).await;
    let trace_before = receiver.capture.finished(Signal::Trace);
    let metric_before = receiver.capture.finished(Signal::Metric);
    let setup = setup_production(&config(&receiver.endpoint));
    for (kind, seq, second) in [
        (TaskTelemetryKind::Assigned, 1, 1),
        (TaskTelemetryKind::Started, 2, 3),
        (TaskTelemetryKind::Completed, 3, 7),
    ] {
        let record = record("task-normal", kind, seq, second);
        setup.0.sink.emit(record.clone()).unwrap();
        setup.0.sink.emit(record).unwrap();
    }
    shutdown_production(setup).await;
    receiver
        .capture
        .wait("a span and a metric export", || {
            !receiver.capture.spans.lock().unwrap().is_empty()
                && !receiver.capture.metrics.lock().unwrap().is_empty()
        })
        .await;
    {
        let spans = receiver.capture.spans.lock().unwrap();
        assert_eq!(
            spans
                .iter()
                .filter(|span| span.name == "atm.task.event")
                .count(),
            3
        );
        let span = spans.iter().find(|span| span.name == "atm.task").unwrap();
        assert_eq!(
            span.end_time_unix_nano - span.start_time_unix_nano,
            6_000_000_000
        );
        assert_eq!(span.events.len(), 3);
        assert!(span.start_time_unix_nano < 1_800_000_000_000_000_000);
    }
    {
        let metrics = receiver.capture.metrics.lock().unwrap();
        let counter = metrics
            .iter()
            .find(|metric| metric.name == "atm.task.events")
            .unwrap();
        if let Some(Data::Sum(sum)) = &counter.data {
            assert_eq!(sum.data_points.len(), 3);
            for point in &sum.data_points {
                assert_eq!(point.attributes.len(), 1);
                assert_eq!(point.attributes[0].key, "kind");
                assert_eq!(
                    point.value,
                    Some(
                        opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsInt(1)
                    )
                );
            }
        } else {
            panic!("native counter sum");
        }
        for (name, sum) in [
            ("atm.task.time_to_start_ms", 2000.),
            ("atm.task.time_to_close_ms", 6000.),
        ] {
            let histogram = metrics.iter().find(|metric| metric.name == name).unwrap();
            if let Some(Data::Histogram(histogram)) = &histogram.data {
                assert_eq!(histogram.data_points[0].count, 1);
                assert_eq!(histogram.data_points[0].sum, Some(sum));
                assert!(histogram.data_points[0].attributes.is_empty());
                assert!(!histogram.data_points[0].explicit_bounds.is_empty());
            } else {
                panic!("explicit-bucket histogram");
            }
        }
    }
    wait_for_task_exports(&receiver, trace_before, metric_before).await;
    assert_transport_metadata(&receiver.capture);
    receiver.stop().await;
}

#[tokio::test]
async fn native_ids_are_deterministic_isolated_and_context_does_not_leak() {
    let receiver = Receiver::start(false).await;
    for _ in 0..2 {
        let trace_before = receiver.capture.finished(Signal::Trace);
        let metric_before = receiver.capture.finished(Signal::Metric);
        let setup =
            setup_success(&config(&receiver.endpoint), 64, receiver.capture.clone()).unwrap();
        for (task, seq) in [("one", 1), ("two", 1), ("one", 4)] {
            setup
                .task
                .sink
                .emit(record(task, TaskTelemetryKind::Assigned, seq, seq))
                .unwrap();
            setup
                .task
                .sink
                .emit(record(task, TaskTelemetryKind::Completed, seq + 1, seq + 1))
                .unwrap();
        }
        assert!(
            opentelemetry::Context::current()
                .get::<opentelemetry::trace::TraceId>()
                .is_none()
        );
        assert!(
            opentelemetry::Context::current()
                .get::<opentelemetry::trace::SpanId>()
                .is_none()
        );
        setup.tracer.tracer("unrelated").start("unrelated").end();
        shutdown(setup).await;
        wait_for_task_exports(&receiver, trace_before, metric_before).await;
    }
    {
        let spans = receiver.capture.spans.lock().unwrap();
        let tasks: Vec<_> = spans
            .iter()
            .filter(|span| span.name == "atm.task")
            .collect();
        assert_eq!(tasks.len(), 6);
        for i in 0..3 {
            assert_eq!(tasks[i].trace_id, tasks[i + 3].trace_id);
            assert_eq!(tasks[i].span_id, tasks[i + 3].span_id);
        }
        assert_eq!(tasks[0].trace_id, tasks[2].trace_id);
        assert_ne!(tasks[0].span_id, tasks[2].span_id);
        assert_ne!(tasks[0].trace_id, tasks[1].trace_id);
        let unrelated: Vec<_> = spans
            .iter()
            .filter(|span| span.name == "unrelated")
            .collect();
        assert_eq!(unrelated.len(), 2);
        assert_ne!(unrelated[0].trace_id, unrelated[1].trace_id);
        assert_ne!(unrelated[0].trace_id, tasks[0].trace_id);
    }
    receiver.stop().await;
}

#[tokio::test]
async fn late_old_close_never_closes_new_assignment_and_missing_start_is_partial() {
    let receiver = Receiver::start(false).await;
    let trace_before = receiver.capture.finished(Signal::Trace);
    let metric_before = receiver.capture.finished(Signal::Metric);
    let setup = setup_success(&config(&receiver.endpoint), 64, receiver.capture.clone()).unwrap();
    for (kind, seq, second) in [
        (TaskTelemetryKind::Started, 2, 2),
        (TaskTelemetryKind::Assigned, 1, 1),
        (TaskTelemetryKind::Started, 5, 5),
        (TaskTelemetryKind::Reassigned, 4, 4),
        (TaskTelemetryKind::Completed, 3, 3),
        (TaskTelemetryKind::Completed, 6, 6),
    ] {
        setup
            .task
            .sink
            .emit(record("reordered", kind, seq, second))
            .unwrap();
    }
    setup
        .task
        .sink
        .emit(record("missing", TaskTelemetryKind::Completed, 9, 9))
        .unwrap();
    shutdown(setup).await;
    wait_for_task_exports(&receiver, trace_before, metric_before).await;
    {
        let spans = receiver.capture.spans.lock().unwrap();
        let tasks: Vec<_> = spans
            .iter()
            .filter(|span| span.name == "atm.task")
            .collect();
        assert_eq!(tasks.len(), 3);
        assert_eq!(
            tasks[1].end_time_unix_nano - tasks[1].start_time_unix_nano,
            2_000_000_000
        );
        assert_eq!(tasks[2].start_time_unix_nano, tasks[2].end_time_unix_nano);
        assert!(
            tasks[1]
                .events
                .iter()
                .all(|event| event.time_unix_nano >= tasks[1].start_time_unix_nano)
        );
    }
    {
        let metrics = receiver.capture.metrics.lock().unwrap();
        for (name, count, sum) in [
            ("atm.task.time_to_start_ms", 1, 1_000.),
            ("atm.task.time_to_close_ms", 1, 2_000.),
        ] {
            let metric = metrics.iter().find(|metric| metric.name == name).unwrap();
            let Some(Data::Histogram(histogram)) = &metric.data else {
                panic!("{name} must be a histogram");
            };
            assert_eq!(histogram.data_points.len(), 1);
            assert_eq!(histogram.data_points[0].count, count);
            assert_eq!(histogram.data_points[0].sum, Some(sum));
            assert!(histogram.data_points[0].attributes.is_empty());
        }
    }
    receiver.stop().await;
}

#[tokio::test]
async fn receiver_metrics_cap_series_without_identity_labels() {
    let receiver = Receiver::start(false).await;
    let trace_before = receiver.capture.finished(Signal::Trace);
    let metric_before = receiver.capture.finished(Signal::Metric);
    let setup = setup_success(&config(&receiver.endpoint), 64, receiver.capture.clone()).unwrap();
    for index in 0..40 {
        let task = format!("distinct-task-{index}");
        for (kind, seq, second) in [
            (TaskTelemetryKind::Assigned, 1, 1),
            (TaskTelemetryKind::Started, 2, 2),
            (TaskTelemetryKind::Completed, 3, 3),
        ] {
            setup
                .task
                .sink
                .emit(record(&task, kind, seq, second))
                .unwrap();
        }
    }
    shutdown(setup).await;
    receiver
        .capture
        .wait("the task metric export", || {
            receiver
                .capture
                .metrics
                .lock()
                .unwrap()
                .iter()
                .any(|metric| metric.name == "atm.task.events")
        })
        .await;
    wait_for_task_exports(&receiver, trace_before, metric_before).await;
    {
        let metrics = receiver.capture.metrics.lock().unwrap();
        let (mut sums, mut histograms) = (0, 0);
        for metric in metrics
            .iter()
            .filter(|metric| metric.name.starts_with("atm.task."))
        {
            match &metric.data {
                Some(Data::Sum(sum)) => {
                    sums += 1;
                    assert!(
                        sum.data_points.len() <= 15,
                        "{} exceeded series cap",
                        metric.name
                    );
                    assert_eq!(
                        sum.data_points.len(),
                        3,
                        "{} must contain one series for each emitted kind",
                        metric.name
                    );
                    assert!(
                        sum.data_points
                            .iter()
                            .all(|point| point.attributes.len() == 1
                                && point.attributes[0].key.as_str() == "kind")
                    );
                }
                Some(Data::Histogram(histogram)) => {
                    histograms += 1;
                    assert_eq!(
                        histogram.data_points.len(),
                        1,
                        "{} must be one unlabeled series",
                        metric.name
                    );
                    assert!(
                        histogram
                            .data_points
                            .iter()
                            .all(|point| point.attributes.is_empty())
                    );
                }
                _ => {}
            }
        }
        assert!(sums > 0, "the receiver captured the per-kind task counter");
        assert!(histograms > 0, "the receiver captured the task histograms");
    }
    receiver.stop().await;
}

/// Positive: an `https` endpoint enables TLS in any letter case, matching the
/// scheme validation. Negative: `http` and look-alike prefixes do not.
#[test]
fn tls_is_enabled_for_an_https_scheme_in_any_case() {
    for endpoint in ["https://c:4317", "HTTPS://c:4317", "HttpS://c:4317"] {
        assert!(crate::otel_setup::uses_tls(endpoint), "{endpoint}");
    }
    for endpoint in [
        "http://c:4317",
        "HTTP://c:4317",
        "https:/c",
        "xhttps://c",
        "",
    ] {
        assert!(!crate::otel_setup::uses_tls(endpoint), "{endpoint}");
    }
}

/// Positive: a reassign stamped with the same instant as the close of a
/// closed assignment of unknown generation starts a new assignment, decided by
/// its higher seq. Negative: the new assignment's own close yields a second
/// task span and is not dropped as an old terminal row.
#[tokio::test]
async fn same_instant_reassign_after_a_close_starts_a_new_assignment_by_seq() {
    let receiver = Receiver::start(false).await;
    let trace_before = receiver.capture.finished(Signal::Trace);
    let metric_before = receiver.capture.finished(Signal::Metric);
    let setup = setup_success(&config(&receiver.endpoint), 64, receiver.capture.clone()).unwrap();
    for (kind, seq, second) in [
        (TaskTelemetryKind::Completed, 3, 3),
        (TaskTelemetryKind::Reassigned, 4, 3),
        (TaskTelemetryKind::Completed, 5, 4),
    ] {
        setup
            .task
            .sink
            .emit(record("same-instant", kind, seq, second))
            .unwrap();
    }
    shutdown(setup).await;
    wait_for_task_exports(&receiver, trace_before, metric_before).await;
    {
        let spans = receiver.capture.spans.lock().unwrap();
        let tasks: Vec<_> = spans
            .iter()
            .filter(|span| span.name == "atm.task")
            .collect();
        assert_eq!(tasks.len(), 2, "the reassign opened a second assignment");
        assert!(
            tasks
                .iter()
                .any(|task| task.end_time_unix_nano - task.start_time_unix_nano == 1_000_000_000),
            "the new assignment closed one second after it began"
        );
    }
    receiver.stop().await;
}
