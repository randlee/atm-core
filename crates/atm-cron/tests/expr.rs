use std::str::FromStr;
use std::sync::Arc;

use atm_core::boundary::{CronScheduler, CronStartSpec, PhaseId, StopCondition};
use atm_core::test_support::TEST_SENDER;
use atm_cron::{AtmCron, AtmCronDeps, ManualClock, SeqJitter};
use atm_runtime_test_support::{
    DummyHookRunner, InMemoryCronScheduleStore, RecordingHookActionDispatcher,
};

async fn parse(expression: &str) -> Result<(), atm_core::error::AtmError> {
    let now = "2026-10-19T00:00:00Z".parse().unwrap();
    let cron = AtmCron::new(AtmCronDeps {
        store: Arc::new(InMemoryCronScheduleStore::new()),
        runner: Arc::new(DummyHookRunner::default()),
        dispatcher: Arc::new(RecordingHookActionDispatcher::default()),
        clock: Arc::new(ManualClock::new(now)),
        host: None,
        jitter: Arc::new(SeqJitter::default()),
    });
    cron.start(CronStartSpec {
        team: "test-team".parse().unwrap(),
        phase: PhaseId::parse("bk").unwrap(),
        name: "test".into(),
        cron_expr: expression.into(),
        module_path: "/module.py".into(),
        cwd: "/work".into(),
        args: vec![],
        timeout_ms: 1000,
        custom: Default::default(),
        stop: StopCondition::Hook,
        created_by: TEST_SENDER.parse().unwrap(),
    })
    .await
    .map(|_| ())
}

#[tokio::test]
async fn cron_parse_accepts_six_seven_and_macros() {
    for expression in [
        "0 * * * * *",
        "0 * * * * * *",
        "@daily",
        "@hourly",
        "@weekly",
        "@monthly",
        "@yearly",
    ] {
        parse(expression).await.unwrap();
    }
}

#[tokio::test]
async fn cron_parse_refuses_five_fields_with_named_form() {
    assert!(
        parse("* * * * *")
            .await
            .unwrap_err()
            .message()
            .contains("6 or 7 fields")
    );
}

#[tokio::test]
async fn cron_parse_refuses_garbage_and_out_of_range() {
    for expression in ["x y", "0 0 25 * * *"] {
        let error = parse(expression).await.unwrap_err();
        assert_eq!(error.code().as_str(), "ATM_CRON_EXPR_INVALID");
        let parser_error = cron::Schedule::from_str(expression).unwrap_err();
        assert!(error.message().contains(&parser_error.to_string()));
    }
}

#[tokio::test]
async fn cron_parse_refuses_no_future_occurrence() {
    assert!(
        parse("0 0 0 1 1 * 2020")
            .await
            .unwrap_err()
            .message()
            .contains("no future occurrence")
    );
}
