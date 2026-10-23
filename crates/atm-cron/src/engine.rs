//! Typed cron execution composition.
//!
//! This is deliberately separate from the legacy hook runner path while the
//! scheduler wiring migrates.  It persists each typed transition before it
//! attempts a lifecycle nudge; module output intents stay durable work for
//! the delivery owner.

use std::sync::Arc;

use atm_core::{
    api::RequestDeadline,
    boundary::{
        CronCommitResult, CronDeliveryIntent, CronDeliveryKey, CronDiagnosticDetail, CronFailure,
        CronFailureKind, CronHookRunner, CronLifecycleNudger, CronRunTransition, CronSchedule,
        CronSuccess, CronTransitionStore, CronTransitionWrite, HookName, HookTarget, StopCondition,
    },
    error::{AtmError, AtmErrorCode},
    hook::{CronHookOutcome, CronStopReply, CronTickReply, HookCaller, HookRequest},
};

use crate::{AtmCron, tick};

/// Ports needed by the typed cron engine.
///
/// Action-bearing replies intentionally fail closed until the separately
/// owned canonical `CronRunPermit` admission port is available.  This avoids
/// dispatching an action through the old unfenced dispatcher.
pub struct CronEngineDeps {
    pub runner: Arc<dyn CronHookRunner>,
    pub transitions: Arc<dyn CronTransitionStore>,
    pub lifecycle_nudger: Arc<dyn CronLifecycleNudger>,
}

pub(crate) struct CronEngine {
    deps: CronEngineDeps,
}

struct Execution {
    transition: CronRunTransition,
    outputs: Vec<atm_core::boundary::CronModuleOutput>,
}

impl CronEngine {
    pub(crate) fn new(deps: CronEngineDeps) -> Self {
        Self { deps }
    }

    async fn commit(
        &self,
        cron: &AtmCron,
        schedule: &CronSchedule,
        transition: CronRunTransition,
        outputs: Vec<atm_core::boundary::CronModuleOutput>,
    ) -> Result<bool, AtmError> {
        let write = CronTransitionWrite {
            id: schedule.id,
            generation: schedule.generation,
            at: cron.deps.clock.now().into(),
            transition,
            outputs,
        };
        let transitions = self.deps.transitions.clone();
        let result = tokio::task::spawn_blocking(move || transitions.commit(write))
            .await
            .map_err(|source| {
                AtmError::new(AtmErrorCode::InternalError, "cron transition task failed")
                    .with_cause(source)
            })??;
        let CronCommitResult::Applied { intents } = result else {
            return Ok(false);
        };
        for intent in intents {
            let CronDeliveryIntent::Lifecycle(intent) = intent else {
                continue;
            };
            let receipt = match self.deps.lifecycle_nudger.deliver(&intent).await {
                Ok(receipt) => receipt,
                Err(error) => {
                    tracing::error!(subsystem = "atm_cron", action = "lifecycle_nudge_failed", schedule = %schedule.id, %error, "cron lifecycle nudge failed: {error}");
                    continue;
                }
            };
            let transitions = self.deps.transitions.clone();
            let key = intent.key();
            let marked = tokio::task::spawn_blocking(move || {
                transitions.mark_delivered(CronDeliveryKey::Lifecycle(key), receipt)
            })
            .await
            .map_err(|source| {
                AtmError::new(
                    AtmErrorCode::InternalError,
                    "cron delivery receipt task failed",
                )
                .with_cause(source)
            });
            match marked {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => {
                    tracing::error!(subsystem = "atm_cron", action = "lifecycle_receipt_failed", schedule = %schedule.id, %error, "cron lifecycle receipt persistence failed: {error}")
                }
                Err(error) => {
                    tracing::error!(subsystem = "atm_cron", action = "lifecycle_receipt_task_failed", schedule = %schedule.id, %error, "cron lifecycle receipt task failed: {error}")
                }
            }
        }
        Ok(true)
    }

    pub(crate) async fn record(
        &self,
        cron: &AtmCron,
        schedule: &CronSchedule,
        transition: CronRunTransition,
    ) -> bool {
        match self.commit(cron, schedule, transition, Vec::new()).await {
            Ok(applied) => applied,
            Err(error) => {
                tracing::error!(subsystem = "atm_cron", action = "transition_failed", schedule = %schedule.id, %error, "cron transition commit failed: {error}");
                false
            }
        }
    }

    pub(crate) async fn run_one(
        &self,
        cron: &AtmCron,
        schedule: &CronSchedule,
    ) -> tick::CronRunOutcome {
        let started = cron.deps.clock.now();
        let target = HookTarget::from(schedule);
        let execution = if tick::stop_due(&schedule.stop, started) {
            Execution::new(
                CronRunTransition::Stopped {
                    reason: diagnostic("until reached"),
                },
                Vec::new(),
            )
        } else {
            if schedule.stop == StopCondition::Hook {
                match self.stop(cron, schedule, &target).await {
                    Ok(stop) if stop.stop => {
                        let transition = CronRunTransition::Stopped {
                            reason: stop.reason.expect("CronStopReply invariant"),
                        };
                        let outcome = outcome_for(&transition);
                        let _ = self.commit(cron, schedule, transition, stop.output).await;
                        tick::log(schedule, "tick", &outcome, elapsed(cron, started));
                        return outcome;
                    }
                    Ok(stop) => {
                        if !self
                            .commit(
                                cron,
                                schedule,
                                CronRunTransition::StopCheckContinues,
                                stop.output,
                            )
                            .await
                            .unwrap_or(false)
                        {
                            return tick::CronRunOutcome::SkippedInFlight;
                        }
                    }
                    Err(error) => {
                        let transition = failure(
                            CronFailureKind::StopCheckFailed,
                            Some(error.code()),
                            "cron stop check failed",
                        );
                        let outcome = outcome_for(&transition);
                        let _ = self.commit(cron, schedule, transition, Vec::new()).await;
                        tick::log(schedule, "stop_check", &outcome, elapsed(cron, started));
                        return outcome;
                    }
                }
            }
            self.tick(cron, schedule, &target).await
        };
        let outcome = outcome_for(&execution.transition);
        match self
            .commit(cron, schedule, execution.transition, execution.outputs)
            .await
        {
            Ok(_) => {}
            Err(error) => {
                tracing::error!(subsystem = "atm_cron", action = "transition_failed", schedule = %schedule.id, %error, "cron transition commit failed: {error}")
            }
        }
        tick::log(schedule, "tick", &outcome, elapsed(cron, started));
        outcome
    }

    async fn stop(
        &self,
        cron: &AtmCron,
        schedule: &CronSchedule,
        target: &HookTarget,
    ) -> Result<CronStopReply, AtmError> {
        self.deps
            .runner
            .stop_cron(
                target,
                &request(cron, schedule, target, HookName::cron_stop()),
                deadline(cron, schedule),
            )
            .await
    }

    async fn tick(
        &self,
        cron: &AtmCron,
        schedule: &CronSchedule,
        target: &HookTarget,
    ) -> Execution {
        match self
            .deps
            .runner
            .run_cron(
                target,
                &request(cron, schedule, target, HookName::cron_tick()),
                deadline(cron, schedule),
            )
            .await
        {
            Err(error) => Execution::new(runner_failure(error), Vec::new()),
            Ok(CronHookOutcome { reply, .. }) => match reply {
                CronTickReply::PassThrough { output } => Execution::new(
                    CronRunTransition::Success {
                        outcome: CronSuccess::Ok,
                    },
                    output,
                ),
                CronTickReply::NotApplicable => Execution::new(
                    CronRunTransition::Success {
                        outcome: CronSuccess::Ok,
                    },
                    Vec::new(),
                ),
                CronTickReply::Refused { message } => Execution::new(
                    CronRunTransition::Failure(CronFailure {
                        kind: CronFailureKind::Refused,
                        code: None,
                        detail: message,
                    }),
                    Vec::new(),
                ),
                CronTickReply::Handled { actions, output } if actions.is_empty() => Execution::new(
                    CronRunTransition::Success {
                        outcome: CronSuccess::Actions { count: 0 },
                    },
                    output,
                ),
                CronTickReply::Handled { output, .. } => Execution::new(
                    failure(
                        CronFailureKind::ActionFailed,
                        None,
                        "cron action admission permit unavailable",
                    ),
                    output,
                ),
            },
        }
    }
}

impl Execution {
    fn new(
        transition: CronRunTransition,
        outputs: Vec<atm_core::boundary::CronModuleOutput>,
    ) -> Self {
        Self {
            transition,
            outputs,
        }
    }
}

fn failure(kind: CronFailureKind, code: Option<AtmErrorCode>, detail: &str) -> CronRunTransition {
    CronRunTransition::Failure(CronFailure {
        kind,
        code,
        detail: diagnostic(detail),
    })
}

fn diagnostic(value: &str) -> CronDiagnosticDetail {
    CronDiagnosticDetail::parse(value.to_owned()).expect("static cron diagnostic is valid")
}

fn runner_failure(error: AtmError) -> CronRunTransition {
    let kind = match error.code() {
        AtmErrorCode::HookTimeout => CronFailureKind::Timeout,
        AtmErrorCode::HookMalformed => CronFailureKind::MalformedReply,
        _ => CronFailureKind::RunnerExit,
    };
    failure(kind, Some(error.code()), "cron hook runner failed")
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

fn deadline(cron: &AtmCron, schedule: &CronSchedule) -> RequestDeadline {
    RequestDeadline::after_at(
        cron.deps.clock.monotonic_now(),
        std::time::Duration::from_millis(u64::from(schedule.timeout_ms) + 500),
    )
}

fn elapsed(cron: &AtmCron, started: chrono::DateTime<chrono::Utc>) -> i64 {
    (cron.deps.clock.now() - started).num_milliseconds()
}

fn outcome_for(transition: &CronRunTransition) -> tick::CronRunOutcome {
    match transition {
        CronRunTransition::Success {
            outcome: CronSuccess::Ok,
        } => tick::CronRunOutcome::Ok,
        CronRunTransition::Success {
            outcome: CronSuccess::Actions { count },
        } => tick::CronRunOutcome::Actions(*count),
        CronRunTransition::StopCheckContinues => tick::CronRunOutcome::Ok,
        CronRunTransition::Stopped { reason } => {
            tick::CronRunOutcome::Stopped(reason.as_str().to_owned())
        }
        CronRunTransition::SkippedInFlight => tick::CronRunOutcome::SkippedInFlight,
        CronRunTransition::Failure(failure) => match failure.kind {
            CronFailureKind::Timeout => tick::CronRunOutcome::Timeout,
            CronFailureKind::MalformedReply => tick::CronRunOutcome::Malformed,
            CronFailureKind::ActionFailed => {
                tick::CronRunOutcome::ActionErr(failure.code.unwrap_or(AtmErrorCode::InternalError))
            }
            CronFailureKind::StopCheckFailed => tick::CronRunOutcome::StopCheckFailed(
                failure.code.unwrap_or(AtmErrorCode::InternalError),
            ),
            CronFailureKind::Refused => {
                tick::CronRunOutcome::Refused(failure.detail.as_str().to_owned())
            }
            CronFailureKind::RunnerExit => {
                tick::CronRunOutcome::Exit(failure.detail.as_str().to_owned())
            }
        },
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod tests {
    use super::*;
    use atm_core::{
        boundary::{CronScheduler, CronStartSpec, PhaseId},
        hook::CronTickReply,
        test_support::TEST_SENDER,
    };
    use atm_runtime_test_support::{
        InMemoryCronScheduleStore, RecordingCronLifecycleNudger, RecordingHookActionDispatcher,
        ScriptedCronHookRunner,
    };
    use atm_storage::{CronModuleOutput, CronOutputBody, CronRunTransition};

    fn output(body: &str) -> CronModuleOutput {
        CronModuleOutput::Message {
            body: CronOutputBody::parse(body.to_owned()).expect("test output"),
        }
    }

    async fn typed_cron(
        runner: Arc<ScriptedCronHookRunner>,
        transitions: Arc<atm_storage::testing::InMemoryCronTransitionStore>,
    ) -> (AtmCron, CronSchedule) {
        let cron = AtmCron::new(crate::AtmCronDeps {
            store: Arc::new(InMemoryCronScheduleStore::new()),
            runner: Arc::new(atm_runtime_test_support::DummyHookRunner::default()),
            dispatcher: Arc::new(RecordingHookActionDispatcher::default()),
            clock: Arc::new(crate::ManualClock::new(
                "2026-10-19T00:00:00Z".parse().expect("timestamp"),
            )),
            host: None,
            jitter: Arc::new(crate::SeqJitter::default()),
        })
        .with_typed_execution(CronEngineDeps {
            runner,
            transitions,
            lifecycle_nudger: Arc::new(RecordingCronLifecycleNudger::default()),
        });
        let schedule = cron
            .start(CronStartSpec {
                team: "test-team".parse().expect("team"),
                phase: PhaseId::parse("bk").expect("phase"),
                name: atm_core::boundary::CronName::parse("typed").expect("name"),
                cron_expr: "@hourly".to_owned(),
                module_path: "/module.py".into(),
                cwd: "/work".into(),
                args: vec![],
                timeout_ms: 1000,
                custom: Default::default(),
                stop: StopCondition::Hook,
                created_by: TEST_SENDER.parse().expect("sender"),
            })
            .await
            .expect("schedule");
        (cron, schedule)
    }

    #[tokio::test]
    async fn pass_through_persists_output_with_success_transition() {
        let runner = Arc::new(ScriptedCronHookRunner::default());
        runner
            .tick_replies
            .lock()
            .expect("tick queue")
            .push_back(Ok(CronHookOutcome {
                reply: CronTickReply::PassThrough {
                    output: vec![output("ordered module output")],
                },
                elapsed_ms: 0,
            }));
        runner
            .stop_replies
            .lock()
            .expect("stop queue")
            .push_back(Ok(
                CronStopReply::new(false, None, vec![]).expect("stop reply")
            ));
        let transitions = Arc::new(atm_storage::testing::InMemoryCronTransitionStore::default());
        let (cron, schedule) = typed_cron(runner, transitions.clone()).await;

        assert_eq!(
            tick::run_one(&cron, &schedule).await,
            tick::CronRunOutcome::Ok
        );
        let writes = transitions.writes.lock().expect("writes");
        assert_eq!(writes.len(), 2);
        assert!(matches!(
            writes[0].transition,
            CronRunTransition::StopCheckContinues
        ));
        assert!(matches!(
            writes[1].transition,
            CronRunTransition::Success { .. }
        ));
        assert_eq!(writes[1].outputs, vec![output("ordered module output")]);
    }

    #[tokio::test]
    async fn action_reply_fails_closed_without_the_permit_admission_port() {
        let runner = Arc::new(ScriptedCronHookRunner::default());
        runner
            .tick_replies
            .lock()
            .expect("tick queue")
            .push_back(Ok(CronHookOutcome {
                reply: CronTickReply::Handled {
                    actions: vec![atm_core::hook::HookAction {
                        command: atm_core::hook::HookActionCommand::Doctor,
                        run_as: None,
                        request: serde_json::json!({}),
                    }],
                    output: vec![output("retain output")],
                },
                elapsed_ms: 0,
            }));
        runner
            .stop_replies
            .lock()
            .expect("stop queue")
            .push_back(Ok(
                CronStopReply::new(false, None, vec![]).expect("stop reply")
            ));
        let transitions = Arc::new(atm_storage::testing::InMemoryCronTransitionStore::default());
        let (cron, schedule) = typed_cron(runner, transitions.clone()).await;

        let outcome = tick::run_one(&cron, &schedule).await;
        assert!(matches!(outcome, tick::CronRunOutcome::ActionErr(_)));
        let writes = transitions.writes.lock().expect("writes");
        assert!(matches!(
            writes[1].transition,
            CronRunTransition::Failure(_)
        ));
        assert_eq!(writes[1].outputs, vec![output("retain output")]);
    }
}
