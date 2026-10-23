//! External consumer proof for the frozen cron-only runner port.
//!
//! This deliberately lives in `atm-cron`, the BK-13 consumer crate, rather
//! than beside the trait: a task-hook `HookRunner` cannot satisfy this test.

use std::sync::Arc;
use std::time::Duration;

use atm_core::api::RequestDeadline;
use atm_core::boundary::{CronHookRunner, HookName, HookTarget, PhaseId};
use atm_core::hook::{CronHookOutcome, CronStopReply, CronTickReply, HookCaller, HookRequest};
use atm_core::types::{AgentName, TeamName};
use atm_runtime_test_support::ScriptedCronHookRunner;

fn target() -> HookTarget {
    HookTarget {
        team: TeamName::from_validated("team"),
        phase: PhaseId::parse("bk").expect("fixed phase is valid"),
        module_path: "/hooks/cron.py".into(),
        cwd: "/work".into(),
        timeout_ms: 1_000,
        custom: Default::default(),
    }
}

fn request(target: &HookTarget, hook: HookName) -> HookRequest {
    HookRequest::for_cron(
        target,
        HookCaller {
            identity: AgentName::from_validated("creator"),
            team: TeamName::from_validated("team"),
            host: None,
        },
        hook,
        &["--controlled".to_owned()],
    )
}

#[tokio::test]
async fn cron_engine_consumer_keeps_tick_and_stop_contracts_separate() {
    let runner = Arc::new(ScriptedCronHookRunner::default());
    runner
        .tick_replies
        .lock()
        .expect("test queue")
        .push_back(Ok(CronHookOutcome {
            reply: CronTickReply::PassThrough { output: vec![] },
            elapsed_ms: 0,
        }));
    runner
        .stop_replies
        .lock()
        .expect("test queue")
        .push_back(Ok(
            CronStopReply::new(false, None, vec![]).expect("valid non-stop reply")
        ));

    let target = target();
    let deadline = RequestDeadline::after(Duration::from_secs(1));
    let tick = request(&target, HookName::cron_tick());
    let stop = request(&target, HookName::cron_stop());

    let tick_reply = runner
        .run_cron(&target, &tick, deadline)
        .await
        .expect("cron tick reply");
    assert!(matches!(tick_reply.reply, CronTickReply::PassThrough { output } if output.is_empty()));
    let stop_reply = runner
        .stop_cron(&target, &stop, deadline)
        .await
        .expect("cron stop reply");
    assert!(!stop_reply.stop);

    let calls = runner.calls.lock().expect("recorded calls");
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].1.hook, HookName::cron_tick());
    assert!(!calls[0].3);
    assert_eq!(calls[1].1.hook, HookName::cron_stop());
    assert!(calls[1].3);
}
