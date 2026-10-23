use std::time::Duration;

use atm_core::{
    api::RequestDeadline,
    boundary::{
        CronGeneration, CronRunPermit, CronRunPermitIssuer, CronSchedule, CronScheduleId,
        DispatchSpec, HOOK_ACTION_RESERVE_MS, HookName, HookTarget, StopCondition, sealed,
    },
    error::AtmError,
    error_codes::AtmErrorCode,
    hook::{HookCaller, HookReply, HookRequest},
};
use chrono::{DateTime, Utc};

use crate::{AtmCron, scheduler::store_call};

/// Private scheduler-owned source for the opaque generation fence.
pub(crate) struct SchedulePermitIssuer {
    schedule_id: CronScheduleId,
    generation: CronGeneration,
}

impl sealed::Sealed for SchedulePermitIssuer {}

impl CronRunPermitIssuer for SchedulePermitIssuer {
    fn schedule_id(&self) -> CronScheduleId {
        self.schedule_id
    }

    fn generation(&self) -> CronGeneration {
        self.generation
    }
}

impl SchedulePermitIssuer {
    pub(crate) fn for_schedule(schedule: &CronSchedule) -> Self {
        Self {
            schedule_id: schedule.id,
            generation: schedule.generation,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CronRunOutcome {
    Ok,
    Refused(String),
    Actions(usize),
    SkippedInFlight,
    Exit(String),
    Timeout,
    Malformed,
    ActionErr(AtmErrorCode),
    Stopped(String),
    StopCheckFailed(AtmErrorCode),
}

impl CronRunOutcome {
    pub(crate) fn as_str(&self) -> String {
        match self {
            Self::Ok => "ok".into(),
            Self::Refused(message) => format!("refused: {message}"),
            Self::Actions(count) => format!("actions: {count}"),
            Self::SkippedInFlight => "skipped_in_flight".into(),
            Self::Exit(message) if !message.is_empty() => format!("exit: {message}"),
            Self::Exit(_) => "exit".into(),
            Self::Timeout => "timeout".into(),
            Self::Malformed => "malformed".into(),
            Self::ActionErr(code) => format!("action_err: {code}"),
            Self::Stopped(reason) => atm_core::boundary::stopped_outcome(reason),
            Self::StopCheckFailed(code) => format!("stop_check_failed: {code}"),
        }
    }
}

pub(crate) fn stop_due(stop: &StopCondition, now: DateTime<Utc>) -> bool {
    matches!(stop, StopCondition::Until { at } if now >= at.into_inner())
}

pub(crate) fn log(
    schedule: &CronSchedule,
    action: &str,
    outcome: &CronRunOutcome,
    elapsed_ms: i64,
) {
    let text = outcome.as_str();
    let class = text.split(':').next().unwrap_or(&text);
    let reason = if let CronRunOutcome::Stopped(reason) = outcome {
        reason.as_str()
    } else {
        ""
    };
    tracing::info!(subsystem = "atm_cron", action, outcome = class, team = %schedule.team,
        phase = %schedule.phase, hook = if action == "stop_check" { HookName::CRON_STOP } else { HookName::CRON_TICK },
        schedule = %schedule.id, elapsed_ms = elapsed_ms.max(0), reason, "cron {action}: {text}");
}

pub(crate) async fn record(cron: &AtmCron, schedule: &CronSchedule, outcome: &CronRunOutcome) {
    if let Some(engine) = &cron.typed_engine {
        let _ = engine
            .record(cron, schedule, transition_for_record(outcome))
            .await;
        return;
    }
    let (id, generation, at) = (schedule.id, schedule.generation, cron.deps.clock.now());
    let stopped_reason = match outcome {
        CronRunOutcome::Stopped(reason) => Some(reason.clone()),
        _ => None,
    };
    let text = outcome.as_str();
    let result = store_call(cron.deps.store.clone(), move |store| {
        if let Some(reason) = stopped_reason {
            store.record_stop(id, generation, at.into(), &reason)
        } else {
            store.record_run(id, generation, at.into(), &text)
        }
    })
    .await;
    if let Err(error) = result {
        tracing::error!(subsystem = "atm_cron", action = "record_failed", schedule = %id, %error, "cron record failed: {error}");
    }
}

fn transition_for_record(outcome: &CronRunOutcome) -> atm_core::boundary::CronRunTransition {
    use atm_core::boundary::{CronDiagnosticDetail, CronFailure, CronFailureKind, CronSuccess};

    let diagnostic = |detail: &str| {
        CronDiagnosticDetail::parse(detail.to_owned()).expect("static cron diagnostic is valid")
    };
    match outcome {
        CronRunOutcome::Ok => atm_core::boundary::CronRunTransition::Success {
            outcome: CronSuccess::Ok,
        },
        CronRunOutcome::Actions(count) => atm_core::boundary::CronRunTransition::Success {
            outcome: CronSuccess::Actions { count: *count },
        },
        CronRunOutcome::SkippedInFlight => atm_core::boundary::CronRunTransition::SkippedInFlight,
        CronRunOutcome::Stopped(reason) => atm_core::boundary::CronRunTransition::Stopped {
            reason: diagnostic(reason),
        },
        CronRunOutcome::Refused(message) => {
            atm_core::boundary::CronRunTransition::Failure(CronFailure {
                kind: CronFailureKind::Refused,
                code: None,
                detail: diagnostic(message),
            })
        }
        CronRunOutcome::Timeout => failure_for_record(CronFailureKind::Timeout, None),
        CronRunOutcome::Malformed => failure_for_record(CronFailureKind::MalformedReply, None),
        CronRunOutcome::Exit(_) => failure_for_record(CronFailureKind::RunnerExit, None),
        CronRunOutcome::ActionErr(code) => {
            failure_for_record(CronFailureKind::ActionFailed, Some(*code))
        }
        CronRunOutcome::StopCheckFailed(code) => {
            failure_for_record(CronFailureKind::StopCheckFailed, Some(*code))
        }
    }
}

fn failure_for_record(
    kind: atm_core::boundary::CronFailureKind,
    code: Option<AtmErrorCode>,
) -> atm_core::boundary::CronRunTransition {
    atm_core::boundary::CronRunTransition::Failure(atm_core::boundary::CronFailure {
        kind,
        code,
        detail: atm_core::boundary::CronDiagnosticDetail::parse("cron run failed".to_owned())
            .expect("static cron diagnostic is valid"),
    })
}

fn deadline_budget(timeout_ms: u32) -> Duration {
    Duration::from_millis(u64::from(timeout_ms) + u64::from(HOOK_ACTION_RESERVE_MS))
}

fn deadline(cron: &AtmCron, schedule: &CronSchedule) -> RequestDeadline {
    RequestDeadline::after_at(
        cron.deps.clock.monotonic_now(),
        deadline_budget(schedule.timeout_ms),
    )
}

fn request(
    cron: &AtmCron,
    schedule: &CronSchedule,
    target: &HookTarget,
    hook: HookName,
) -> HookRequest {
    HookRequest::for_cron(
        target,
        HookCaller {
            identity: schedule.created_by.clone(),
            team: schedule.team.clone(),
            host: cron.deps.host.clone(),
        },
        hook,
        &schedule.args,
    )
}

fn runner_error(error: AtmError) -> CronRunOutcome {
    match error.code() {
        AtmErrorCode::HookTimeout => CronRunOutcome::Timeout,
        AtmErrorCode::HookMalformed => CronRunOutcome::Malformed,
        _ => CronRunOutcome::Exit(String::new()),
    }
}

async fn reply_outcome(
    cron: &AtmCron,
    schedule: &CronSchedule,
    reply: HookReply,
) -> CronRunOutcome {
    match reply {
        HookReply::Refused { message } => CronRunOutcome::Refused(message),
        HookReply::PassThrough | HookReply::NotApplicable => CronRunOutcome::Ok,
        HookReply::Handled(actions) => {
            let count = actions.len();
            let issuer = SchedulePermitIssuer::for_schedule(schedule);
            match cron
                .deps
                .dispatcher
                .dispatch_cron_actions(
                    DispatchSpec {
                        hook: HookName::cron_tick(),
                        team: schedule.team.clone(),
                        phase: schedule.phase.clone(),
                        default_as: schedule.created_by.clone(),
                        deadline: None,
                        dry_run: false,
                    },
                    CronRunPermit::from_issuer(&issuer),
                    actions,
                )
                .await
            {
                Ok(_) => CronRunOutcome::Actions(count),
                Err(error) => CronRunOutcome::ActionErr(error.code()),
            }
        }
    }
}

pub(crate) async fn run_one(cron: &AtmCron, schedule: &CronSchedule) -> CronRunOutcome {
    if let Some(engine) = &cron.typed_engine {
        return engine.run_one(cron, schedule).await;
    }
    let started = cron.deps.clock.now();
    let target = HookTarget::from(schedule);
    if stop_due(&schedule.stop, started) {
        let outcome = CronRunOutcome::Stopped("until reached".into());
        record(cron, schedule, &outcome).await;
        log(schedule, "tick", &outcome, 0);
        return outcome;
    }
    if schedule.stop == StopCondition::Hook {
        let result = cron
            .deps
            .runner
            .stop(
                &target,
                &request(cron, schedule, &target, HookName::cron_stop()),
                deadline(cron, schedule),
            )
            .await;
        let check = match result {
            Ok(reply) if reply.stop => CronRunOutcome::Stopped(reply.reason.unwrap_or_default()),
            Ok(_) => CronRunOutcome::Ok,
            Err(error) => CronRunOutcome::StopCheckFailed(error.code()),
        };
        log(
            schedule,
            "stop_check",
            &check,
            (cron.deps.clock.now() - started).num_milliseconds(),
        );
        if check != CronRunOutcome::Ok {
            record(cron, schedule, &check).await;
            return check;
        }
    }
    let run_deadline = deadline(cron, schedule);
    let outcome = match cron
        .deps
        .runner
        .run(
            &target,
            &request(cron, schedule, &target, HookName::cron_tick()),
            run_deadline,
        )
        .await
    {
        Err(error) => runner_error(error),
        Ok(result) => reply_outcome(cron, schedule, result.reply).await,
    };
    log(
        schedule,
        "tick",
        &outcome,
        (cron.deps.clock.now() - started).num_milliseconds(),
    );
    record(cron, schedule, &outcome).await;
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "test-utils")]
    use atm_core::test_support::TEST_SENDER;
    use atm_core::types::IsoTimestamp;

    #[test]
    fn stop_due_is_the_one_until_rule() {
        let at: IsoTimestamp = "2026-10-19T00:00:00Z".parse().unwrap();
        let stop = StopCondition::Until { at };
        assert!(!stop_due(
            &stop,
            at.into_inner() - chrono::Duration::seconds(1)
        ));
        assert!(stop_due(&stop, at.into_inner()));
        assert!(!stop_due(&StopCondition::Hook, at.into_inner()));
    }

    #[test]
    fn cron_budget_is_not_the_task_hook_cap() {
        let budget = deadline_budget(20000);
        assert_eq!(budget, Duration::from_millis(20500));
        assert!(budget > Duration::from_millis(u64::from(atm_core::boundary::HOOK_TIMEOUT_MAX_MS)));
    }

    #[cfg(feature = "test-utils")]
    #[tokio::test]
    async fn record_run_writes_each_outcome_class() {
        use atm_core::boundary::{CronScheduler, CronStartSpec, PhaseId};
        use atm_runtime_test_support::{
            DummyHookRunner, InMemoryCronScheduleStore, RecordingHookActionDispatcher,
        };
        use std::sync::Arc;
        let store = Arc::new(InMemoryCronScheduleStore::new());
        let now = "2026-10-19T00:00:00Z".parse().unwrap();
        let cron = AtmCron::new(crate::AtmCronDeps {
            store: store.clone(),
            runner: Arc::new(DummyHookRunner::default()),
            dispatcher: Arc::new(RecordingHookActionDispatcher::default()),
            clock: Arc::new(crate::ManualClock::new(now)),
            host: None,
            jitter: Arc::new(crate::SeqJitter::default()),
        });
        let outcomes = [
            (CronRunOutcome::Ok, "ok"),
            (CronRunOutcome::Refused("quota".into()), "refused: quota"),
            (CronRunOutcome::Actions(3), "actions: 3"),
            (CronRunOutcome::SkippedInFlight, "skipped_in_flight"),
            (CronRunOutcome::Exit(String::new()), "exit"),
            (CronRunOutcome::Timeout, "timeout"),
            (CronRunOutcome::Malformed, "malformed"),
            (
                CronRunOutcome::ActionErr(AtmErrorCode::HookMalformed),
                "action_err: ATM_HOOK_MALFORMED",
            ),
            (
                CronRunOutcome::StopCheckFailed(AtmErrorCode::HookTimeout),
                "stop_check_failed: ATM_HOOK_TIMEOUT",
            ),
            (CronRunOutcome::Stopped("done".into()), "stopped: done"),
        ];
        for (index, (outcome, expected_outcome)) in outcomes.into_iter().enumerate() {
            let row = cron
                .start(CronStartSpec {
                    team: "test-team".parse().unwrap(),
                    phase: PhaseId::parse("bk").unwrap(),
                    name: atm_core::boundary::CronName::parse(&format!("job-{index}")).unwrap(),
                    cron_expr: "@hourly".into(),
                    module_path: "/module.py".into(),
                    cwd: "/work".into(),
                    args: vec![],
                    timeout_ms: 1000,
                    custom: Default::default(),
                    stop: StopCondition::Hook,
                    created_by: TEST_SENDER.parse().unwrap(),
                })
                .await
                .unwrap();
            record(&cron, &row, &outcome).await;
            let saved = cron
                .list(None, None)
                .await
                .unwrap()
                .into_iter()
                .find(|entry| entry.id == row.id)
                .unwrap();
            assert_eq!(saved.last_outcome.as_deref(), Some(expected_outcome));
            if matches!(outcome, CronRunOutcome::Stopped(_)) {
                assert!(!saved.active);
            } else {
                assert_eq!(saved.last_run_at.unwrap().into_inner(), now);
            }
        }
    }
}
