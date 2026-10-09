#![cfg(test)]

use super::*;
use crate::otel_setup::{
    EXPORT_BATCH, EXPORT_INTERVAL, EXPORT_QUEUE, EXPORT_TIMEOUT, EXPORT_TRANSPORT_TIMEOUT,
};
use crate::{ExportDiagnostics, TracingBridgeLayer, build_retained_logger};
use atm_core::observability::{
    AtmTelemetryExportFailure, AtmTelemetryExportHealth, AtmTelemetryExportState,
};
use opentelemetry::metrics::MeterProvider;
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
    assert_eq!(EXPORT_TRANSPORT_TIMEOUT, Duration::from_millis(300));
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

const UNREACHABLE_CHILD: &str = "ATM_OTEL_UNREACHABLE_COLLECTOR_CHILD";
const UNREACHABLE_TEST: &str = "otel_tests::lifecycle::real_unreachable_collector_diagnostics_do_not_claim_delivery_or_recovery";

/// The scenario captures SDK failure events through a tracing subscriber. A
/// thread-scoped dispatcher races sibling tests in tracing's per-callsite
/// interest cache and can miss the event, so the scenario runs in a child
/// process of this test binary that owns the global subscriber, as the
/// production bridge does.
#[test]
fn real_unreachable_collector_diagnostics_do_not_claim_delivery_or_recovery() {
    if std::env::var_os(UNREACHABLE_CHILD).is_some() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(unreachable_collector_scenario());
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            UNREACHABLE_TEST,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(UNREACHABLE_CHILD, "1")
        .output()
        .expect("spawn the unreachable-collector child");
    assert!(
        output.status.success(),
        "child scenario failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A collector that accepts each connection and closes it at once.
async fn start_closing_collector() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let collector = tokio::spawn(async move {
        while let Ok((connection, _)) = listener.accept().await {
            drop(connection);
        }
    });
    (endpoint, collector)
}

fn assert_each_sdk_event_projects(observed: &ExportFailureObserved) {
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

/// Installs the child process's global subscriber: the production tracing
/// bridge feeding `ExportDiagnostics`, plus a recorder of SDK event names.
fn install_global_subscriber() -> (
    tempfile::TempDir,
    Arc<ExportDiagnostics>,
    ExportFailureObserved,
) {
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
    let bridge = TracingBridgeLayer::new(logger);
    bridge.set_export_diagnostics(diagnostics.clone());
    let observed = ExportFailureObserved::default();
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::registry()
            .with(bridge)
            .with(observed.clone()),
    );
    tracing::dispatcher::set_global_default(dispatch).expect("child owns the global subscriber");
    (root, diagnostics, observed)
}

async fn unreachable_collector_scenario() {
    // The collector accepts each connection and closes it at once, so every
    // export fails immediately on every OS. A closed port is not equivalent:
    // a refused connect is OS-timed, and on Windows CI no span or log export
    // failure surfaced within 30 s.
    let (endpoint, collector) = start_closing_collector().await;
    let (_root, diagnostics, observed) = install_global_subscriber();
    // A one-span batch exports the first span at once through the SDK's
    // full-batch path. The scheduled path first fires at 2x EXPORT_INTERVAL
    // (the SDK interval sleeps before its first tick, then skips it), which
    // a loaded runner can push past the deadline below.
    let setup = setup_with_limits(&config(&endpoint), 1, Duration::from_millis(50)).unwrap();
    setup
        .0
        .sink
        .emit(record("unreachable", TaskTelemetryKind::Assigned, 1, 1))
        .unwrap();
    setup
        .2
        .logger("unreachable")
        .emit(setup.2.logger("unreachable").create_log_record());
    setup
        .3
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
        .unwrap();
    let mut snapshot = health();
    diagnostics.project(&mut snapshot);
    assert_eq!(snapshot.state, AtmTelemetryExportState::Unavailable);
    assert_eq!(snapshot.dropped_failure, 0);
    let (task, traces, logs, metrics) = setup;
    drop(task);
    let (trace, log, metric) = tokio::join!(
        tokio::task::spawn_blocking(move || traces.shutdown()),
        tokio::task::spawn_blocking(move || logs.shutdown()),
        tokio::task::spawn_blocking(move || metrics.shutdown())
    );
    for result in [trace, log, metric] {
        diagnostics.observe_result(result.unwrap());
    }
    collector.abort();
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
    assert_each_sdk_event_projects(&observed);
}

#[tokio::test]
async fn configured_trace_log_metric_timeouts_cancel_stalled_nonempty_exports() {
    let receiver = Receiver::start(true).await;
    // Deterministic by construction: the tonic transport bound is 50 ms while
    // the SDK processor bounds are 30 s, so only the transport timeout can end
    // a stalled export inside this test. Equal bounds would race.
    let setup = setup_with_timeouts(
        &config(&receiver.endpoint),
        64,
        Duration::from_millis(50),
        Duration::from_secs(30),
    )
    .unwrap();
    setup
        .0
        .sink
        .emit(record("stalled", TaskTelemetryKind::Completed, 1, 1))
        .unwrap();
    let logger = setup.2.logger("stalled");
    let mut log = logger.create_log_record();
    log.set_body("nonempty".into());
    logger.emit(log);
    setup
        .3
        .meter("stalled")
        .u64_counter("fixture.count")
        .build()
        .add(1, &[]);
    let (task, traces, logs, metrics) = setup;
    drop(task);
    let trace = tokio::task::spawn_blocking(move || traces.shutdown());
    let log = tokio::task::spawn_blocking(move || logs.shutdown());
    let metric = tokio::task::spawn_blocking(move || metrics.shutdown());
    let capture = &receiver.capture;
    capture
        .wait("an export started for every signal", || {
            Signal::ALL
                .iter()
                .all(|signal| capture.started(*signal) >= 1)
        })
        .await;
    assert!(!receiver.capture.spans.lock().unwrap().is_empty());
    assert!(!receiver.capture.logs.lock().unwrap().is_empty());
    assert!(!receiver.capture.metrics.lock().unwrap().is_empty());
    let (trace, log, metric) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(trace, log, metric)
    })
    .await
    .expect("all configured shutdown timeouts return before the test deadline");
    // The receiver never releases these exports, so each one ends only when
    // the transport timeout cancels the request (the SDK bound is 30 s and
    // cannot fire). The pinned SDK wraps the tonic failure as InternalFailure
    // text naming the signal's client, which is then the only possible shape.
    capture
        .wait("an export finished for every signal", || {
            Signal::ALL
                .iter()
                .all(|signal| capture.finished(*signal) >= 1)
        })
        .await;
    for (signal, result) in Signal::ALL.into_iter().zip([trace, log, metric]) {
        let error = result
            .unwrap()
            .expect_err("each shutdown must report its export timeout")
            .to_string();
        assert!(
            error.contains(&format!("{} export failed", signal.client()))
                && error.contains("Timeout expired"),
            "{signal:?} shutdown returned {error}"
        );
        assert_eq!(capture.started(signal), 1, "{signal:?} exports started");
        assert_eq!(capture.finished(signal), 1, "{signal:?} export cancelled");
    }
    receiver.stop().await;
}

#[tokio::test]
async fn full_backlog_and_concurrent_shutdown_keep_terminal_failure_without_fabricated_losses() {
    let receiver = Receiver::start(true).await;
    let setup =
        setup_with_limits(&config(&receiver.endpoint), 64, Duration::from_millis(50)).unwrap();
    let tracer = setup.1.tracer("full-backlog");
    let logger = setup.2.logger("full-backlog");
    // No yield: saturate each SDK bounded queue before its worker can consume.
    for _ in 0..1024 {
        tracer.start("queued").end();
        let mut log = logger.create_log_record();
        log.set_body("queued".into());
        logger.emit(log);
    }
    let diagnostics = Arc::new(ExportDiagnostics::default());
    let (task, traces, logs, metrics) = setup;
    drop(task);
    let duplicate = traces.clone();
    // The outer bound only names a hang; the pass criterion is the results.
    let results = tokio::time::timeout(Duration::from_secs(120), async {
        tokio::join!(
            tokio::task::spawn_blocking(move || traces.shutdown()),
            tokio::task::spawn_blocking(move || duplicate.shutdown()),
            tokio::task::spawn_blocking(move || logs.shutdown()),
            tokio::task::spawn_blocking(move || metrics.shutdown())
        )
    })
    .await
    .expect("the four concurrent SDK shutdowns never returned");
    let results = [
        results.0.unwrap(),
        results.1.unwrap(),
        results.2.unwrap(),
        results.3.unwrap(),
    ];
    // A saturated queue leaves no room for the shutdown message (channel full)
    // or, when the worker drains first, the flush meets the stalled receiver
    // (transport timeout). Which one is scheduler-dependent, so only the
    // failure is asserted for traces and logs; metrics has nothing to flush
    // and may succeed.
    assert!(
        results[..3].iter().all(Result::is_err),
        "traces and logs shutdowns must fail with a stalled receiver: {results:?}"
    );
    // The two traces handles share one provider: exactly one call performs
    // the shutdown and the other reports it was already invoked.
    let already_invoked = results[..2]
        .iter()
        .filter(|result| {
            result
                .as_ref()
                .is_err_and(|error| error.to_string().contains("already invoked"))
        })
        .count();
    assert_eq!(already_invoked, 1, "{results:?}");
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
            snapshot.dropped_timeout,
            snapshot.dropped_failure,
            snapshot.dropped_shutdown
        ),
        (0, 0, 0, 0),
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
        setup.1.tracer("abandoned-wait").start("nonempty").end();
    }
    let logger = setup.2.logger("abandoned-wait");
    let mut log = logger.create_log_record();
    log.set_body("nonempty".into());
    logger.emit(log);
    setup
        .3
        .meter("abandoned-wait")
        .u64_counter("fixture.count")
        .build()
        .add(1, &[]);
    let (task, traces, logs, metrics) = setup;
    drop(task);
    let diagnostics = Arc::new(ExportDiagnostics::default());
    let mut trace_call = tokio::task::spawn_blocking(move || traces.shutdown());
    let mut log_call = tokio::task::spawn_blocking(move || logs.shutdown());
    let mut metric_call = tokio::task::spawn_blocking(move || metrics.shutdown());
    let capture = &receiver.capture;
    capture
        .wait("an export started for every signal", || {
            Signal::ALL
                .iter()
                .all(|signal| capture.started(*signal) >= 1)
        })
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
    for signal in Signal::ALL {
        assert_eq!(capture.finished(signal), 0, "{signal:?} export still held");
    }
    diagnostics.shutdown_wait_timed_out();
    // Cancelling caller waits cannot abort spawn_blocking. Retain and
    // eventually join each handle; BD6 separately proves actual process exit.
    trace_call.abort();
    log_call.abort();
    metric_call.abort();
    for _ in 0..3 {
        receiver.capture.release();
    }
    let (trace, log, metric) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(trace_call, log_call, metric_call)
    })
    .await
    .expect("all abandoned shutdown calls finish after release");
    // Each signal's abandoned call still completes its own released export,
    // and each terminal Ok is recorded without clearing the shutdown timeout.
    for (signal, result) in Signal::ALL.into_iter().zip([trace, log, metric]) {
        let result = result.unwrap();
        assert!(result.is_ok(), "{signal:?} shutdown returned {result:?}");
        diagnostics.observe_result(result);
        assert_eq!(capture.finished(signal), capture.started(signal));
    }
    let mut snapshot = health();
    diagnostics.project(&mut snapshot);
    assert_eq!(
        snapshot.last_failure,
        Some(AtmTelemetryExportFailure::ShutdownTimedOut)
    );
    receiver.stop().await;
}
