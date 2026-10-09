#![cfg(test)]

use super::*;
use crate::{
    RetainedLogLevel, RetainedLogPolicy, TracingBridgeLayer, build_routed_retained_logger,
};
use atm_core::LogDestination;
use sc_observability_log::{
    AttachmentOptions, BridgeEventDecision, BridgeEventPolicy, BridgeOptions,
};
use tracing_subscriber::prelude::*;

struct Admit;
impl BridgeEventPolicy for Admit {
    fn decide(&self, _: &sc_observability_types::LogEvent) -> BridgeEventDecision {
        BridgeEventDecision::Admit
    }
}

pub(super) fn policy() -> RetainedLogPolicy {
    RetainedLogPolicy {
        rotation_max_bytes: 10 * 1024 * 1024,
        rotation_max_files: 5,
        retention_max_age: Duration::from_secs(7 * 86400),
        maintenance_cadence: Duration::from_secs(60),
        writer_shutdown_timeout: Duration::from_secs(1),
        maintenance_max_work_per_pass: Some(8),
    }
}

#[test]
fn otel_and_both_routes_require_an_sdk_logger() {
    for destination in [LogDestination::Otel, LogDestination::Both] {
        let root = tempfile::tempdir().unwrap();
        let log_dir = root.path().join("logs");
        let result = build_routed_retained_logger(
            "atm",
            &log_dir,
            policy(),
            Some(RetainedLogLevel::Info),
            destination,
            None,
        );
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("{destination:?} must reject a missing SDK logger"),
        };

        assert!(
            error.is_observability_bootstrap(),
            "{destination:?}: {error}"
        );
        assert_eq!(
            error.message().split('\n').next(),
            Some("OTel log destination requires a configured SDK logger"),
            "{destination:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_otel_both_route_once_filter_secrets_and_leave_caller_provider_alive() {
    for destination in [
        LogDestination::File,
        LogDestination::Otel,
        LogDestination::Both,
    ] {
        let receiver = Receiver::start(false).await;
        let setup =
            setup_with_limits(&config(&receiver.endpoint), 64, Duration::from_millis(50)).unwrap();
        let root = tempfile::tempdir().unwrap();
        let log_dir = root.path().join("logs");
        let logger = Arc::new(
            build_routed_retained_logger(
                "atm",
                &log_dir,
                policy(),
                Some(RetainedLogLevel::Info),
                destination,
                Some(setup.3.logger("atm.structured")),
            )
            .unwrap(),
        );
        let mut attachment = sc_observability_log::attach_logger(
            logger.0.clone(),
            AttachmentOptions::new(
                BridgeOptions {
                    default_action: sc_observability_types::ActionName::new("fixture.direct")
                        .unwrap(),
                    parse_bracket_action: true,
                },
                Arc::new(Admit),
            ),
        )
        .unwrap();
        let bridge = TracingBridgeLayer::new(logger.clone());
        let dispatch = tracing::Dispatch::new(tracing_subscriber::registry().with(bridge));
        let subscriber = tracing::dispatcher::set_default(&dispatch);
        sc_observability_log::event!(name: "fixture.direct", target: "fixture.direct", sc_observability_log::Level::INFO, token = "raw-secret", "one direct macro");
        sc_observability_log::debug!(target: "fixture.filtered", "must not be exported");
        sc_observability_log::event!(name: "fixture.secret", target: "fixture.secret", sc_observability_log::Level::DEBUG, token = "raw-secret");
        tracing::warn!(target: "fixture.tracing", code = "FIXTURE_TRACING", token = "raw-secret", "one tracing input");
        tracing::warn!(name: "BatchSpanProcessor.Export.Error", target: "opentelemetry_sdk", error = "Bearer raw-secret");
        let flush = logger.clone();
        tokio::task::spawn_blocking(move || flush.flush())
            .await
            .unwrap()
            .unwrap();
        if destination != LogDestination::File {
            receiver
                .capture
                .wait("two log exports", || {
                    receiver.capture.logs.lock().unwrap().len() >= 2
                })
                .await;
            let logs = receiver.capture.logs.lock().unwrap();
            assert_eq!(
                logs.len(),
                2,
                "one record per selected OTel destination; no SDK recursion"
            );
            let text = format!("{logs:?}");
            assert!(!text.contains("raw-secret"));
            assert!(!text.contains("must not be exported"));
        }
        if destination != LogDestination::Otel {
            let text = std::fs::read_to_string(log_dir.join("atm.log.jsonl")).unwrap();
            let lines: Vec<serde_json::Value> = text
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert_eq!(
                lines.len(),
                3,
                "two inputs plus one local-only SDK diagnostic"
            );
            assert!(!text.contains("raw-secret"));
            assert!(!text.contains("must not be exported"));
        } else {
            assert!(!log_dir.join("atm.log.jsonl").exists());
        }
        drop(subscriber);
        drop(dispatch);
        attachment.detach(Duration::from_secs(1)).unwrap();
        drop(attachment);
        let logger = Arc::try_unwrap(logger)
            .ok()
            .expect("all attachments released");
        tokio::task::spawn_blocking(move || logger.shutdown())
            .await
            .unwrap();
        let sdk_logger = setup.3.logger("caller-still-owns-provider");
        let mut record = sdk_logger.create_log_record();
        record.set_body("after retained logger drop".into());
        sdk_logger.emit(record);
        shutdown(setup).await;
        let expected = if destination == LogDestination::File {
            1
        } else {
            3
        };
        assert_eq!(receiver.capture.logs.lock().unwrap().len(), expected);
        receiver.stop().await;
    }
}
