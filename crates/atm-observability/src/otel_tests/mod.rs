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
use opentelemetry::trace::{Span, Tracer, TracerProvider};
use opentelemetry_proto::tonic::metrics::v1::metric::Data;

use super::otel_setup::{TelemetrySetup, setup_with_limits, setup_with_timeouts};
use receiver::{Receiver, Signal};

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

async fn shutdown(setup: TelemetrySetup) {
    let (task, workflow, traces, logs, metrics) = setup;
    drop(task);
    drop(workflow);
    let results = tokio::join!(
        tokio::task::spawn_blocking(move || traces.shutdown()),
        tokio::task::spawn_blocking(move || logs.shutdown()),
        tokio::task::spawn_blocking(move || metrics.shutdown()),
    );
    results.0.unwrap().unwrap();
    results.1.unwrap().unwrap();
    results.2.unwrap().unwrap();
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
    let setup =
        setup_with_limits(&config(&receiver.endpoint), 64, Duration::from_millis(50)).unwrap();
    for (index, kind) in kinds.into_iter().enumerate() {
        setup
            .0
            .sink
            .emit(record("all-kinds", kind, index as u64 + 1, 1))
            .await
            .unwrap();
    }
    shutdown(setup).await;
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
async fn workflow_uses_durable_native_spans_without_inventing_incomplete_duration() {
    let receiver = Receiver::start(false).await;
    let setup =
        setup_with_limits(&config(&receiver.endpoint), 64, Duration::from_millis(50)).unwrap();
    for completed in [true, false] {
        let record = serde_json::from_value(serde_json::json!({
            "observation": if completed { "Completed" } else { "Incomplete" },
            "scope_kind": "sprint", "scope_id": "bd-fixture", "state": "done",
            "stage": "dev", "transition": "complete", "iteration": null,
            "start_message_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "start_timestamp": "2026-01-01T00:00:01Z",
            "end_message_id": completed.then_some("01ARZ3NDEKTSV4RRFFQ69G5FAW"),
            "end_timestamp": completed.then_some("2026-01-01T00:00:04Z"),
            "duration_millis": completed.then_some(3000)
        }))
        .unwrap();
        setup.1.sink.emit(record).await.unwrap();
    }
    shutdown(setup).await;
    {
        let spans = receiver.capture.spans.lock().unwrap();
        assert_eq!(spans.len(), 2);
        assert!(spans.iter().all(|span| span.name == "atm.workflow"));
        assert_eq!(
            spans[0].end_time_unix_nano - spans[0].start_time_unix_nano,
            3_000_000_000
        );
        assert_eq!(spans[1].end_time_unix_nano, spans[1].start_time_unix_nano);
        assert_eq!(spans[0].trace_id, spans[1].trace_id);
        assert_ne!(spans[0].span_id, spans[1].span_id);
        assert!(
            spans[0]
                .attributes
                .iter()
                .any(|attribute| attribute.key == "atm.workflow.scope_id")
        );
    }
    receiver.stop().await;
}

#[tokio::test]
async fn receiver_observes_live_durable_spans_metrics_and_dedup() {
    let receiver = Receiver::start(false).await;
    let setup =
        setup_with_limits(&config(&receiver.endpoint), 64, Duration::from_millis(50)).unwrap();
    for (kind, seq, second) in [
        (TaskTelemetryKind::Assigned, 1, 1),
        (TaskTelemetryKind::Started, 2, 3),
        (TaskTelemetryKind::Completed, 3, 7),
    ] {
        let record = record("task-normal", kind, seq, second);
        setup.0.sink.emit(record.clone()).await.unwrap();
        setup.0.sink.emit(record).await.unwrap();
    }
    receiver
        .capture
        .wait(|| {
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
    assert!(
        receiver
            .capture
            .metadata
            .lock()
            .unwrap()
            .iter()
            .all(|metadata| metadata.get("authorization").unwrap() == "Bearer fixture-auth")
    );
    shutdown(setup).await;
    receiver.stop().await;
}

#[tokio::test]
async fn native_ids_are_deterministic_isolated_and_context_does_not_leak() {
    let receiver = Receiver::start(false).await;
    for _ in 0..2 {
        let setup =
            setup_with_limits(&config(&receiver.endpoint), 64, Duration::from_millis(50)).unwrap();
        for (task, seq) in [("one", 1), ("two", 1), ("one", 4)] {
            setup
                .0
                .sink
                .emit(record(task, TaskTelemetryKind::Assigned, seq, seq))
                .await
                .unwrap();
            setup
                .0
                .sink
                .emit(record(task, TaskTelemetryKind::Completed, seq + 1, seq + 1))
                .await
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
        setup.2.tracer("unrelated").start("unrelated").end();
        shutdown(setup).await;
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
    let setup =
        setup_with_limits(&config(&receiver.endpoint), 64, Duration::from_millis(50)).unwrap();
    for (kind, seq, second) in [
        (TaskTelemetryKind::Started, 2, 2),
        (TaskTelemetryKind::Assigned, 1, 1),
        (TaskTelemetryKind::Started, 5, 5),
        (TaskTelemetryKind::Reassigned, 4, 4),
        (TaskTelemetryKind::Completed, 3, 3),
        (TaskTelemetryKind::Completed, 6, 6),
    ] {
        setup
            .0
            .sink
            .emit(record("reordered", kind, seq, second))
            .await
            .unwrap();
    }
    setup
        .0
        .sink
        .emit(record("missing", TaskTelemetryKind::Completed, 9, 9))
        .await
        .unwrap();
    shutdown(setup).await;
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
    let setup =
        setup_with_limits(&config(&receiver.endpoint), 64, Duration::from_millis(50)).unwrap();
    for index in 0..40 {
        let task = format!("distinct-task-{index}");
        for (kind, seq, second) in [
            (TaskTelemetryKind::Assigned, 1, 1),
            (TaskTelemetryKind::Started, 2, 2),
            (TaskTelemetryKind::Completed, 3, 3),
        ] {
            setup
                .0
                .sink
                .emit(record(&task, kind, seq, second))
                .await
                .unwrap();
        }
    }
    shutdown(setup).await;
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
