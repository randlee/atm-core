use atm_core::error::AtmError;
use atm_core::test_support::FakeEnvSource;
use tempfile::TempDir;

use super::{
    ATM_LOG_LEVEL_ENV, AtmObservabilityHealthState, CANONICAL_LOG_FILE_NAME, RetainedCommandEvent,
    RetainedLogLevel, RetainedLogOffer, RetainedLogPolicy, RetainedLogger, build_retained_logger,
    logger_level_override_from, parse_logger_level, prepare_retained_log,
};
use sc_observability::LoggerConfig;
use sc_observability_types::{LogQuery, ServiceName};
use std::time::Duration;

#[test]
fn prepares_the_active_log_file() {
    let tempdir = TempDir::new().expect("tempdir");
    let log_dir = tempdir.path().join("logs");

    let active_log_path = prepare_retained_log(&log_dir).expect("prepare retained log");

    assert_eq!(active_log_path, log_dir.join("atm.log.jsonl"));
    assert!(active_log_path.is_file());
}

#[test]
fn retained_logger_bootstrap_error_preserves_historical_json_without_cause() {
    let daemon_error = super::map_retained_logger_error("synthetic initialization failure");
    let historical = AtmError::observability_bootstrap(
        "failed to initialize shared daemon observability logger",
    );

    assert_eq!(daemon_error.cause(), None);
    assert_eq!(
        serde_json::to_string(&daemon_error).expect("daemon error JSON"),
        serde_json::to_string(&historical).expect("historical error JSON")
    );
}

#[test]
fn rejects_invalid_log_levels_with_the_exact_override_name() {
    let error = parse_logger_level("loud").expect_err("invalid level");

    assert!(error.is_observability_bootstrap());
    assert!(error.message().contains("invalid ATM_LOG value `loud`"));
}

#[test]
fn parses_every_supported_level_case_insensitively() {
    let cases = [
        ("TRACE", RetainedLogLevel::Trace),
        ("debug", RetainedLogLevel::Debug),
        (" info ", RetainedLogLevel::Info),
        ("Warn", RetainedLogLevel::Warn),
        ("error", RetainedLogLevel::Error),
        ("off", RetainedLogLevel::Off),
    ];

    for (value, expected) in cases {
        assert_eq!(
            parse_logger_level(value).expect("supported level"),
            Some(expected),
            "value={value}"
        );
    }
}

#[test]
fn treats_blank_overrides_as_absent() {
    assert_eq!(parse_logger_level("   ").expect("blank override"), None);
}

#[test]
fn reads_the_override_from_the_supplied_environment() {
    let env = FakeEnvSource::new([(ATM_LOG_LEVEL_ENV, Some("debug"))]);

    assert_eq!(
        logger_level_override_from(&env).expect("override"),
        Some(RetainedLogLevel::Debug)
    );
    assert_eq!(
        logger_level_override_from(&FakeEnvSource::empty()).expect("no override"),
        None
    );
}

#[test]
fn atm_adapter_logger_contract_includes_query_records() {
    let tempdir = TempDir::new().expect("tempdir");
    let log_dir = tempdir.path().join("logs");
    let policy = RetainedLogPolicy {
        rotation_max_bytes: 4096,
        rotation_max_files: 2,
        retention_max_age: Duration::from_secs(60),
        maintenance_cadence: Duration::from_secs(60),
        writer_shutdown_timeout: Duration::from_secs(2),
        maintenance_max_work_per_pass: Some(4),
    };
    let logger = build_retained_logger("atm", &log_dir, policy, None)
        .expect("published 1.4.0 logger configuration");

    let health = logger
        .health_at(log_dir.join(CANONICAL_LOG_FILE_NAME))
        .expect("health projection");
    assert!(matches!(
        health.logging_state,
        AtmObservabilityHealthState::Healthy
    ));
    assert!(
        health
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("writer_state=running"))
    );

    let accepted = logger
        .try_log_command(RetainedCommandEvent {
            target: "atm.test",
            action: "qualification",
            outcome: "ok",
            code: None,
        })
        .expect("log admission");
    assert_eq!(accepted, RetainedLogOffer::Accepted);
    logger.flush().expect("flush");

    let queue_full = RetainedLogger::force_queue_full_for_test(|| {
        logger.try_log_command(RetainedCommandEvent {
            target: "atm.test",
            action: "qualification",
            outcome: "ok",
            code: None,
        })
    })
    .expect("queue-full admission");
    assert_eq!(queue_full, RetainedLogOffer::QueueFull);
    let query_logger = RetainedLogger(
        sc_observability::v2::Logger::builder(LoggerConfig::default_for(
            ServiceName::new("atm").expect("service"),
            tempdir.path().join("query"),
        ))
        .expect("published canonical builder")
        .build()
        .expect("typed query logger")
        .into(),
    );
    query_logger
        .try_log_command(RetainedCommandEvent {
            target: "atm.query",
            action: "qualification",
            outcome: "ok",
            code: Some("ATM_QUERY_CONTRACT"),
        })
        .expect("query record admission");
    query_logger.flush().expect("query record flush");
    let snapshot = (0..20)
        .find_map(|_| {
            let snapshot = query_logger.query(&LogQuery::default()).ok()?;
            if snapshot.events.is_empty() {
                std::thread::yield_now();
                None
            } else {
                Some(snapshot)
            }
        })
        .expect("query record");
    assert_eq!(snapshot.events.len(), 1);
    assert_eq!(
        snapshot.events[0]
            .fields
            .get("code")
            .and_then(|value| value.as_str()),
        Some("ATM_QUERY_CONTRACT")
    );
    let _ = query_logger.shutdown();
}

#[test]
fn typed_facade_rejects_invalid_admission_and_keeps_queue_full_distinct() {
    let tempdir = TempDir::new().expect("tempdir");
    let policy = RetainedLogPolicy {
        rotation_max_bytes: 4096,
        rotation_max_files: 2,
        retention_max_age: Duration::from_secs(60),
        maintenance_cadence: Duration::from_secs(60),
        writer_shutdown_timeout: Duration::from_secs(2),
        maintenance_max_work_per_pass: Some(4),
    };
    let logger = build_retained_logger("atm", tempdir.path(), policy, None).expect("logger");
    let invalid = logger
        .try_log_command(RetainedCommandEvent {
            target: "atm.test",
            action: "",
            outcome: "ok",
            code: None,
        })
        .expect_err("invalid action must be rejected");
    assert_eq!(
        invalid.code(),
        atm_core::error::AtmErrorCode::ObservabilityEmitFailed
    );
    let queue_full = RetainedLogger::force_queue_full_for_test(|| {
        logger.try_log_command(RetainedCommandEvent {
            target: "atm.test",
            action: "qualification",
            outcome: "ok",
            code: None,
        })
    })
    .expect("queue-full admission");
    assert_eq!(queue_full, RetainedLogOffer::QueueFull);
    let _ = logger.shutdown();
}

#[test]
fn rejected_offer_surfaces_a_validated_diagnostic_code() {
    let diagnostic_code =
        atm_core::observability::diagnostic_code("SC_TEST_REJECTED").expect("valid code");
    let offer = RetainedLogOffer::Rejected { diagnostic_code };
    let RetainedLogOffer::Rejected { diagnostic_code } = offer else {
        panic!("expected rejected offer");
    };
    assert_eq!(diagnostic_code.as_str(), "SC_TEST_REJECTED");
}
