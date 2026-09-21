//! Isolated consumer qualification for sc-observability-log's public macros.
//! This integration binary owns one temporary global bridge and never touches
//! ATM's production logger installation.

use std::collections::HashSet;
use std::future::pending;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;
use tokio::sync::oneshot;

use sc_observability_log::{ActionName, BridgeOptions, LoggerConfig, ServiceName};
use serde_json::Value;

#[sc_observability_log::instrument(name = "fixture.sync_ok", skip_all)]
fn sync_ok() -> u32 {
    sc_observability_log::info!(target: "fixture", "sync body");
    7
}

#[sc_observability_log::instrument(name = "fixture.sync_err", err, skip_all)]
fn sync_err() -> Result<(), &'static str> {
    Err("expected")
}

#[sc_observability_log::instrument(name = "fixture.sync_panic", skip_all)]
fn sync_panic() {
    panic!("expected panic");
}

#[sc_observability_log::instrument(name = "fixture.async_ok", ret, err, skip_all)]
async fn async_ok() -> Result<u32, &'static str> {
    tokio::task::yield_now().await;
    Ok(9)
}

#[sc_observability_log::instrument(name = "fixture.async_err", ret, err, skip_all)]
async fn async_err() -> Result<(), &'static str> {
    tokio::task::yield_now().await;
    Err("expected async error")
}

#[sc_observability_log::instrument(name = "fixture.async_panic", skip_all)]
async fn async_panic() {
    panic!("expected async panic");
}

#[sc_observability_log::instrument(name = "fixture.async_cancelled", skip_all)]
async fn async_cancelled(started: oneshot::Sender<()>) {
    let _ = started.send(());
    pending::<()>().await;
}

fn reexported_event_levels() {
    sc_observability_log::debug!(target: "fixture", "[fixture.debug] debug body");
    sc_observability_log::warn!(target: "fixture", "[fixture.warn] warn body");
    sc_observability_log::error!(target: "fixture", "[fixture.error] error body");
    sc_observability_log::trace!(target: "fixture", "[fixture.trace] trace body");
}

fn adversarial_fields() {
    let oversized = "x".repeat(40 * 1024);
    sc_observability_log::event!(
        name: "fixture.adversarial",
        target: "fixture",
        sc_observability_log::Level::INFO,
        {
            token = "sensitive-token",
            oversized = oversized.as_str(),
            excess_00 = 0,
            excess_01 = 1,
            excess_02 = 2,
            excess_03 = 3,
            excess_04 = 4,
            excess_05 = 5,
            excess_06 = 6,
            excess_07 = 7,
            excess_08 = 8,
            excess_09 = 9,
            excess_10 = 10,
            excess_11 = 11,
            excess_12 = 12,
            excess_13 = 13,
            excess_14 = 14,
            excess_15 = 15,
        },
        "adversarial fields"
    );
}

fn read_events(path: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .expect("retained output")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("JSONL event"))
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn macros_and_instrument_preserve_bounded_retained_contracts() {
    let root = tempfile::tempdir().expect("temporary logger root");
    let mut config = LoggerConfig::default_for(
        ServiceName::new("bc3-fixture").expect("service name"),
        root.path().into(),
    );
    config.level = sc_observability_log::LevelFilter::Trace;
    config.redaction.denylist_keys.push("token".to_owned());
    let guard = sc_observability_log::init(
        config,
        BridgeOptions {
            default_action: ActionName::new("fixture.default").expect("action"),
            parse_bracket_action: true,
        },
    )
    .expect("one isolated bridge installation");
    let path = guard.active_log_path().expect("active path").to_path_buf();

    assert_eq!(sync_ok(), 7);
    assert_eq!(sync_err(), Err("expected"));
    assert!(catch_unwind(AssertUnwindSafe(sync_panic)).is_err());
    assert_eq!(async_ok().await, Ok(9));
    assert_eq!(async_err().await, Err("expected async error"));
    reexported_event_levels();
    adversarial_fields();
    let async_panic_join = tokio::spawn(async_panic()).await;
    assert!(
        async_panic_join
            .expect_err("async panic must fail the task")
            .is_panic()
    );
    let (started_tx, started_rx) = oneshot::channel();
    let cancellation = tokio::spawn(async_cancelled(started_tx));
    started_rx
        .await
        .expect("cancellation fixture reached handshake");
    cancellation.abort();
    let _ = cancellation.await;

    guard.flush(Duration::from_secs(5)).expect("bounded flush");
    let events = read_events(&path);
    assert_eq!(events.len(), 13, "fixture event cardinality changed");
    let actions: HashSet<_> = events
        .iter()
        .map(|event| event["action"].as_str().expect("action"))
        .collect();
    assert_eq!(actions.len(), 9, "duplicate action emission");
    assert!(actions.contains("fixture.async_err"));
    assert!(actions.contains("fixture.async_panic"));
    for (message, level) in [
        ("debug body", "Debug"),
        ("warn body", "Warn"),
        ("error body", "Error"),
        ("trace body", "Trace"),
    ] {
        assert!(
            events.iter().any(|event| {
                event["level"] == level
                    && event["message"]
                        .as_str()
                        .is_some_and(|value| value.contains(message))
            }),
            "missing re-exported macro level {level} for {message}"
        );
    }
    assert!(events.iter().all(|event| event["version"] == "v1"));
    assert!(events.iter().all(|event| event["correlation_id"].is_null()));
    assert!(events.iter().all(|event| {
        let target = event["target"].as_str().expect("target");
        target == "fixture" || target == "macro_qualification"
    }));
    let adversarial = events
        .iter()
        .find(|event| event["action"] == "fixture.adversarial")
        .expect("adversarial event");
    let adversarial_fields = adversarial["fields"]
        .as_object()
        .expect("adversarial fields");
    assert_eq!(adversarial_fields["token"], "[REDACTED]");
    assert_eq!(
        adversarial_fields["oversized"]
            .as_str()
            .expect("oversized value")
            .len(),
        40 * 1024
    );
    assert_eq!(adversarial_fields["excess_15"], 15);
    let adversarial_encoded =
        serde_json::to_string(adversarial).expect("encoded adversarial event");
    assert!(adversarial_encoded.len() > 40 * 1024);
    assert!(!adversarial_encoded.contains("sensitive-token"));
    let normal_encoded = events
        .iter()
        .filter(|event| event["action"] != "fixture.adversarial")
        .map(|event| serde_json::to_string(event).expect("encoded normal event"))
        .collect::<String>();
    assert!(
        normal_encoded.len() < 32 * 1024,
        "normal retained output oversized"
    );
    for secret in ["password", "authorization", "secret"] {
        assert!(!normal_encoded.to_ascii_lowercase().contains(secret));
    }
    assert!(
        events
            .iter()
            .any(|event| event["action"] == "fixture.sync_ok")
    );
    assert!(
        events
            .iter()
            .any(|event| event["action"] == "fixture.sync_err" && event["outcome"] == "error")
    );
    assert!(
        events
            .iter()
            .any(|event| event["action"] == "fixture.sync_panic" && event["outcome"] == "panicked")
    );
    assert!(
        events
            .iter()
            .any(|event| event["action"] == "fixture.async_ok" && event["outcome"] == "ok")
    );
    assert!(events.iter().any(
        |event| event["action"] == "fixture.async_cancelled" && event["outcome"] == "cancelled"
    ));
    assert!(events.iter().any(
        |event| event["action"] == "fixture.async_panic" && event["outcome"] == "panicked"
    ));
    assert!(
        events
            .iter()
            .any(|event| event["action"] == "fixture.async_err" && event["outcome"] == "error")
    );
    assert!(
        events
            .iter()
            .filter(|event| event["action"] != "fixture.adversarial")
            .all(|event| {
                event["fields"]
                    .as_object()
                    .is_some_and(|fields| fields.len() < 16)
            })
    );
    // JSONL completion order is intentionally not a wall-clock ordering
    // contract: async completion and writer scheduling may interleave. Verify
    // the timeline field semantically instead of coupling the fixture to list
    // order.
    for event in &events {
        let timestamp =
            serde_json::from_value::<sc_observability_types::Timestamp>(event["timestamp"].clone())
                .expect("canonical retained timestamp");
        assert!(timestamp >= sc_observability_types::Timestamp::UNIX_EPOCH);
    }
    assert_eq!(guard.dropped_events().total(), 0);
    guard
        .shutdown(Duration::from_secs(5))
        .expect("bounded shutdown");
}

#[test]
fn supported_and_rejected_macro_grammar_is_compile_checked() {
    trybuild::TestCases::new().pass("tests/ui/macro_supported.rs");
    trybuild::TestCases::new().compile_fail("tests/ui/macro_rejected.rs");
}
