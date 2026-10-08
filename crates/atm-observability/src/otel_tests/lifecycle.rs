use super::*;
use crate::{ExportDiagnostics, TracingBridgeLayer, build_retained_logger};
use atm_core::observability::{
    AtmTelemetryExportFailure, AtmTelemetryExportHealth, AtmTelemetryExportState,
};
use opentelemetry::metrics::MeterProvider;
use std::sync::atomic::Ordering;
use tracing_subscriber::prelude::*;

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
    assert_eq!(crate::EXPORT_QUEUE, 256);
    assert_eq!(crate::EXPORT_BATCH, 256);
    assert_eq!(crate::EXPORT_TIMEOUT, Duration::from_millis(400));
    assert_eq!(crate::EXPORT_INTERVAL, Duration::from_secs(1));
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
    let dispatch = tracing::Dispatch::new(tracing_subscriber::registry().with(bridge));
    let _subscriber = tracing::dispatcher::set_default(&dispatch);
    let setup = setup_with_limits(&config(&endpoint), 64, Duration::from_millis(50)).unwrap();
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
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let mut health = health();
            diagnostics.project(&mut health);
            if health.last_failure.is_some() {
                assert_eq!(
                    health.last_failure,
                    Some(AtmTelemetryExportFailure::Unavailable)
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
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
    receiver
        .capture
        .wait(|| receiver.capture.started.load(Ordering::SeqCst) >= 3)
        .await;
    receiver
        .capture
        .wait(|| receiver.capture.finished.load(Ordering::SeqCst) >= 3)
        .await;
    assert!(!receiver.capture.spans.lock().unwrap().is_empty());
    assert!(!receiver.capture.logs.lock().unwrap().is_empty());
    assert!(!receiver.capture.metrics.lock().unwrap().is_empty());
    let (task, workflow, traces, logs, metrics) = setup;
    drop(task);
    drop(workflow);
    let started = std::time::Instant::now();
    let (trace, log, metric) = tokio::join!(
        tokio::task::spawn_blocking(move || traces.shutdown()),
        tokio::task::spawn_blocking(move || logs.shutdown()),
        tokio::task::spawn_blocking(move || metrics.shutdown())
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(trace.unwrap().is_err() || log.unwrap().is_err() || metric.unwrap().is_err());
    receiver.stop().await;
}
