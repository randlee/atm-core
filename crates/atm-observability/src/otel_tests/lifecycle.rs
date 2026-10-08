#![cfg(test)]

use super::*;
use crate::otel_setup::{EXPORT_BATCH, EXPORT_INTERVAL, EXPORT_QUEUE, EXPORT_TIMEOUT};
use crate::{ExportDiagnostics, TracingBridgeLayer, build_retained_logger};
use atm_core::observability::{
    AtmTelemetryExportFailure, AtmTelemetryExportHealth, AtmTelemetryExportState,
};
use opentelemetry::metrics::MeterProvider;
use std::sync::atomic::Ordering;
use tracing_subscriber::prelude::*;

/// Records every SDK event name so the test can prove which real export
/// failure events the span, log and metric pipelines emit.
#[derive(Clone, Default)]
struct ExportFailureObserved {
    seen: Arc<std::sync::Mutex<Vec<String>>>,
    changed: Arc<tokio::sync::Notify>,
}

impl ExportFailureObserved {
    fn names(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }

    fn first_with_prefix(&self, prefix: &str) -> Option<String> {
        self.names().into_iter().find(|name| {
            name.starts_with(prefix)
                && name.contains("Export")
                && (name.contains("Error") || name.contains("Fail"))
        })
    }

    async fn all_signals_failed(&self) {
        loop {
            let changed = self.changed.notified();
            if ["BatchSpanProcessor", "BatchLogProcessor", "PeriodicReader"]
                .iter()
                .all(|prefix| self.first_with_prefix(prefix).is_some())
            {
                return;
            }
            changed.await;
        }
    }
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for ExportFailureObserved {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        self.seen
            .lock()
            .unwrap()
            .push(event.metadata().name().to_owned());
        self.changed.notify_waiters();
    }
}

fn health() -> AtmTelemetryExportHealth {
    AtmTelemetryExportHealth {
        state: AtmTelemetryExportState::Healthy,
        endpoint: None,
        protocol: None,
        emitted: 0,
        dropped_full: 0,
        dropped_timeout: 0,
        dropped_failure: 0,
        dropped_shutdown: 0,
        last_failure: None,
    }
}

#[test]
fn production_limits_are_distinct_from_test_deadlines_and_terminal_failure_is_retained() {
    assert_eq!(EXPORT_QUEUE, 256);
    assert_eq!(EXPORT_BATCH, 256);
    assert_eq!(EXPORT_TIMEOUT, Duration::from_millis(400));
    assert_eq!(EXPORT_INTERVAL, Duration::from_secs(1));
    let diagnostics = ExportDiagnostics::default();
    diagnostics.shutdown_wait_timed_out();
    diagnostics.observe_result(Ok(()));
    diagnostics.observe_sdk_event("BatchLogProcessor.Export.Error");
    let mut health = health();
    diagnostics.project(&mut health);
    assert_eq!(
        health.last_failure,
        Some(AtmTelemetryExportFailure::ShutdownTimedOut)
    );
    assert_eq!(
        health.dropped_failure, 0,
        "private SDK loss quantity is unknown"
    );
}

#[tokio::test]
async fn real_unreachable_collector_diagnostics_do_not_claim_delivery_or_recovery() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let root = tempfile::tempdir().unwrap();
    let logger = Arc::new(
        build_retained_logger(
            "atm",
            &root.path().join("logs"),
            super::routing::policy(),
            None,
        )
        .unwrap(),
    );
    let diagnostics = Arc::new(ExportDiagnostics::default());
    let bridge = TracingBridgeLayer::new(logger.clone());
    bridge.set_export_diagnostics(diagnostics.clone());
    let observed = ExportFailureObserved::default();
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::registry()
            .with(bridge)
            .with(observed.clone()),
    );
    let _subscriber = tracing::dispatcher::set_default(&dispatch);
    // A one-span batch exports the first span at once through the SDK's
    // full-batch path. The scheduled path first fires at 2x EXPORT_INTERVAL
    // (the SDK interval sleeps before its first tick, then skips it), which
    // a loaded runner can push past the deadline below.
    let setup = setup_with_limits(&config(&endpoint), 1, Duration::from_millis(50)).unwrap();
    setup
        .0
        .sink
        .emit(record("unreachable", TaskTelemetryKind::Assigned, 1, 1))
        .await
        .unwrap();
    setup
        .3
        .logger("unreachable")
        .emit(setup.3.logger("unreachable").create_log_record());
    setup
        .4
        .meter("unreachable")
        .u64_counter("fixture.count")
        .build()
        .add(1, &[]);
    // 30 s is a hang diagnostic only; the periodic metric reader first fires
    // at 2x EXPORT_INTERVAL, so success is far earlier than this ceiling.
    let observed_failure =
        tokio::time::timeout(Duration::from_secs(30), observed.all_signals_failed()).await;
    let mut evidence = health();
    diagnostics.project(&mut evidence);
    // The sink is still nonblocking even though transport failed.
    setup
        .0
        .sink
        .emit(record("unreachable", TaskTelemetryKind::Completed, 2, 2))
        .await
        .unwrap();
    let mut snapshot = health();
    diagnostics.project(&mut snapshot);
    assert_eq!(snapshot.state, AtmTelemetryExportState::Unavailable);
    assert_eq!(snapshot.dropped_failure, 0);
    let (task, workflow, traces, logs, metrics) = setup;
    drop(task);
    drop(workflow);
    let (trace, log, metric) = tokio::join!(
        tokio::task::spawn_blocking(move || traces.shutdown()),
        tokio::task::spawn_blocking(move || logs.shutdown()),
        tokio::task::spawn_blocking(move || metrics.shutdown())
    );
    for result in [trace, log, metric] {
        diagnostics.observe_result(result.unwrap());
    }
    // Assert after explicit shutdown, so fixture failure cannot deadlock the
    // current-thread runtime by dropping live native providers during unwind.
    assert!(
        observed_failure.is_ok(),
        "real span, log and metric export failures expected; SDK emitted {:?}",
        observed.names()
    );
    assert_eq!(
        evidence.last_failure,
        Some(AtmTelemetryExportFailure::Unavailable)
    );
    // Each real SDK event name must independently project as a failure, so a
    // misnamed log or metric mapping cannot hide behind the span events.
    for prefix in ["BatchSpanProcessor", "BatchLogProcessor", "PeriodicReader"] {
        let name = observed.first_with_prefix(prefix).unwrap();
        let alone = ExportDiagnostics::default();
        alone.observe_sdk_event(&name);
        let mut projected = health();
        alone.project(&mut projected);
        assert_eq!(
            projected.last_failure,
            Some(AtmTelemetryExportFailure::Unavailable),
            "{name} is not mapped by ExportDiagnostics"
        );
        assert_eq!(projected.state, AtmTelemetryExportState::Unavailable);
    }
}

#[tokio::test]
async fn configured_trace_log_metric_timeouts_cancel_stalled_nonempty_exports() {
    let receiver = Receiver::start(true).await;
    let setup =
        setup_with_limits(&config(&receiver.endpoint), 64, Duration::from_millis(50)).unwrap();
    setup
        .0
        .sink
        .emit(record("stalled", TaskTelemetryKind::Completed, 1, 1))
        .await
        .unwrap();
    let logger = setup.3.logger("stalled");
    let mut log = logger.create_log_record();
    log.set_body("nonempty".into());
    logger.emit(log);
    setup
        .4
        .meter("stalled")
        .u64_counter("fixture.count")
        .build()
        .add(1, &[]);
    let (task, workflow, traces, logs, metrics) = setup;
    drop(task);
    drop(workflow);
    let trace = tokio::task::spawn_blocking(move || traces.shutdown());
    let log = tokio::task::spawn_blocking(move || logs.shutdown());
    let metric = tokio::task::spawn_blocking(move || metrics.shutdown());
    receiver
        .capture
        .wait(|| receiver.capture.started.load(Ordering::SeqCst) >= 3)
        .await;
    assert!(!receiver.capture.spans.lock().unwrap().is_empty());
    assert!(!receiver.capture.logs.lock().unwrap().is_empty());
    assert!(!receiver.capture.metrics.lock().unwrap().is_empty());
    let (trace, log, metric) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(trace, log, metric)
    })
    .await
    .expect("all configured shutdown timeouts return before the test deadline");
    assert!(trace.unwrap().is_err(), "trace shutdown must time out");
    assert!(log.unwrap().is_err(), "log shutdown must time out");
    assert!(metric.unwrap().is_err(), "metric shutdown must time out");
    receiver
        .capture
        .wait(|| receiver.capture.finished.load(Ordering::SeqCst) >= 3)
        .await;
    receiver.stop().await;
}

#[tokio::test]
async fn full_backlog_and_concurrent_shutdown_keep_terminal_failure_without_fabricated_losses() {
    let receiver = Receiver::start(true).await;
    let setup =
        setup_with_limits(&config(&receiver.endpoint), 64, Duration::from_millis(50)).unwrap();
    let tracer = setup.2.tracer("full-backlog");
    let logger = setup.3.logger("full-backlog");
    // No yield: saturate each SDK bounded queue before its worker can consume.
    for _ in 0..1024 {
        tracer.start("queued").end();
        let mut log = logger.create_log_record();
        log.set_body("queued".into());
        logger.emit(log);
    }
    let diagnostics = Arc::new(ExportDiagnostics::default());
    let (task, workflow, traces, logs, metrics) = setup;
    drop(task);
    drop(workflow);
    let duplicate = traces.clone();
    let started = std::time::Instant::now();
    let results = tokio::join!(
        tokio::task::spawn_blocking(move || traces.shutdown()),
        tokio::task::spawn_blocking(move || duplicate.shutdown()),
        tokio::task::spawn_blocking(move || logs.shutdown()),
        tokio::task::spawn_blocking(move || metrics.shutdown())
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    let results = [
        results.0.unwrap(),
        results.1.unwrap(),
        results.2.unwrap(),
        results.3.unwrap(),
    ];
    assert!(results.iter().any(Result::is_err));
    for result in results {
        diagnostics.observe_result(result);
    }
    diagnostics.observe_result(Ok(()));
    let mut snapshot = health();
    diagnostics.project(&mut snapshot);
    assert!(snapshot.last_failure.is_some());
    assert_eq!(
        (
            snapshot.dropped_full,
            snapshot.dropped_failure,
            snapshot.dropped_shutdown
        ),
        (0, 0, 0),
        "SDK quantities are unknown, never copied from synthetic estimates"
    );
    receiver.stop().await;
}

#[tokio::test]
async fn abandoning_shutdown_wait_does_not_abort_blocking_calls_or_clear_terminal_timeout() {
    let receiver = Receiver::start(true).await;
    // The export timeout outlives the test's hold, so only the receiver's
    // release signal lets the blocking shutdown finish.
    let setup =
        setup_with_limits(&config(&receiver.endpoint), 64, Duration::from_secs(30)).unwrap();
    for _ in 0..64 {
        setup.2.tracer("abandoned-wait").start("nonempty").end();
    }
    let logger = setup.3.logger("abandoned-wait");
    let mut log = logger.create_log_record();
    log.set_body("nonempty".into());
    logger.emit(log);
    setup
        .4
        .meter("abandoned-wait")
        .u64_counter("fixture.count")
        .build()
        .add(1, &[]);
    let (task, workflow, traces, logs, metrics) = setup;
    drop(task);
    drop(workflow);
    let diagnostics = Arc::new(ExportDiagnostics::default());
    let evidence = diagnostics.clone();
    let mut trace_call = tokio::task::spawn_blocking(move || {
        evidence.observe_result(traces.shutdown());
    });
    let mut log_call = tokio::task::spawn_blocking(move || logs.shutdown());
    let mut metric_call = tokio::task::spawn_blocking(move || metrics.shutdown());
    receiver
        .capture
        .wait(|| receiver.capture.started.load(Ordering::SeqCst) >= 3)
        .await;
    // Each export is held by the receiver, so no shutdown can complete yet.
    assert!(
        tokio::time::timeout(Duration::from_millis(200), &mut trace_call)
            .await
            .is_err()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(200), &mut log_call)
            .await
            .is_err()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(200), &mut metric_call)
            .await
            .is_err()
    );
    assert_eq!(receiver.capture.finished.load(Ordering::SeqCst), 0);
    diagnostics.shutdown_wait_timed_out();
    // Cancelling caller waits cannot abort spawn_blocking. Retain and
    // eventually join each handle; BD6 separately proves actual process exit.
    trace_call.abort();
    log_call.abort();
    metric_call.abort();
    for _ in 0..3 {
        receiver.capture.release();
    }
    tokio::time::timeout(Duration::from_secs(10), async {
        let (trace, log, metric) = tokio::join!(trace_call, log_call, metric_call);
        trace.unwrap();
        log.unwrap().unwrap();
        metric.unwrap().unwrap();
    })
    .await
    .expect("all abandoned shutdown calls finish after release");
    receiver
        .capture
        .wait(|| receiver.capture.finished.load(Ordering::SeqCst) >= 3)
        .await;
    let mut snapshot = health();
    diagnostics.project(&mut snapshot);
    assert_eq!(
        snapshot.last_failure,
        Some(AtmTelemetryExportFailure::ShutdownTimedOut)
    );
    receiver.stop().await;
}
