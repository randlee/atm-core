//! Typed cron execution composition.
//!
//! This is deliberately separate from the legacy hook runner path while the
//! scheduler wiring migrates.  It persists each typed transition before it
//! attempts lifecycle nudges and canonical module-mail delivery.

use std::sync::Arc;

use atm_core::{
    api::RequestDeadline,
    boundary::{
        CronCommitResult, CronDeliveryIntent, CronDeliveryKey, CronDiagnosticDetail, CronFailure,
        CronFailureKind, CronHookRunner, CronLifecycleNudger, CronModuleMailDelivery,
        CronRunPermit, CronRunTransition, CronSchedule, CronScheduleId, CronSuccess,
        CronTransitionStore, CronTransitionWrite, DeliveryReceipt, DispatchSpec, HookName,
        HookTarget, PendingCronDelivery, StopCondition,
    },
    error::{AtmError, AtmErrorCode},
    hook::{CronStopReply, CronTickReply, HookAction, HookCaller, HookRequest},
};

use crate::{
    AtmCron,
    tick::{self, SchedulePermitIssuer},
};

/// Ports needed by the typed cron engine.
///
/// Action-bearing replies intentionally fail closed until the separately
/// owned canonical `CronRunPermit` admission port is available.  This avoids
/// dispatching an action through the old unfenced dispatcher.
pub struct CronEngineDeps {
    pub runner: Arc<dyn CronHookRunner>,
    pub transitions: Arc<dyn CronTransitionStore>,
    pub lifecycle_delivery: Arc<dyn CronLifecycleNudger>,
    pub module_mail_delivery: Arc<dyn CronModuleMailDelivery>,
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
        let mut intents = self.pending_intents(schedule).await?;
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
        let CronCommitResult::Applied { intents: committed } = result else {
            return Ok(false);
        };
        intents.extend(committed);
        for intent in intents {
            if let Some((key, receipt)) = self.deliver_intent(schedule, intent).await {
                self.mark_intent_delivered(schedule.id, key, receipt).await;
            }
        }
        Ok(true)
    }

    async fn pending_intents(
        &self,
        schedule: &CronSchedule,
    ) -> Result<Vec<CronDeliveryIntent>, AtmError> {
        let pending_store = self.deps.transitions.clone();
        let pending = tokio::task::spawn_blocking(move || pending_store.pending(128))
            .await
            .map_err(|source| {
                AtmError::new(
                    AtmErrorCode::InternalError,
                    "cron pending delivery task failed",
                )
                .with_cause(source)
            })??;
        Ok(pending
            .into_iter()
            .filter_map(|pending| match pending {
                PendingCronDelivery::ReadyToSend(intent) => match &intent {
                    CronDeliveryIntent::Lifecycle(notice)
                        if notice.key().schedule_id == schedule.id
                            && notice.key().generation == schedule.generation =>
                    {
                        Some(intent)
                    }
                    CronDeliveryIntent::ModuleMail(mail)
                        if matches!(mail.key, CronDeliveryKey::ModuleMail { schedule_id, generation, .. }
                            if schedule_id == schedule.id && generation == schedule.generation) =>
                    {
                        Some(intent)
                    }
                    _ => None,
                },
                PendingCronDelivery::ReconciliationRequired { .. } => None,
            })
            .collect())
    }

    async fn deliver_intent(
        &self,
        schedule: &CronSchedule,
        intent: CronDeliveryIntent,
    ) -> Option<(CronDeliveryKey, DeliveryReceipt)> {
        match intent {
            CronDeliveryIntent::Lifecycle(intent) => {
                let key = CronDeliveryKey::Lifecycle(intent.key());
                match self.deps.lifecycle_delivery.deliver(&intent).await {
                    Ok(receipt) => Some((key, receipt)),
                    Err(error) => {
                        tracing::error!(subsystem = "atm_cron", action = "lifecycle_delivery_failed", schedule = %schedule.id, %error, "cron lifecycle delivery failed: {error}");
                        None
                    }
                }
            }
            CronDeliveryIntent::ModuleMail(intent) => {
                let CronDeliveryKey::ModuleMail {
                    schedule_id,
                    generation,
                    ..
                } = intent.key
                else {
                    tracing::error!(subsystem = "atm_cron", action = "module_mail_key_invalid", schedule = %schedule.id, "cron module mail intent has a lifecycle key");
                    return None;
                };
                if schedule_id != schedule.id || generation != schedule.generation {
                    tracing::error!(subsystem = "atm_cron", action = "module_mail_generation_mismatch", schedule = %schedule.id, "cron module mail intent does not match the committed schedule generation");
                    return None;
                }
                let issuer = SchedulePermitIssuer::for_schedule(schedule);
                let permit = CronRunPermit::from_issuer(&issuer);
                match self
                    .deps
                    .module_mail_delivery
                    .deliver(&intent, &permit)
                    .await
                {
                    Ok(receipt) => Some((intent.key, receipt)),
                    Err(error) => {
                        tracing::error!(subsystem = "atm_cron", action = "module_mail_delivery_failed", schedule = %schedule.id, message_id = %intent.message_id, %error, "cron module mail delivery failed: {error}");
                        None
                    }
                }
            }
        }
    }

    async fn mark_intent_delivered(
        &self,
        schedule_id: CronScheduleId,
        key: CronDeliveryKey,
        receipt: DeliveryReceipt,
    ) {
        let transitions = self.deps.transitions.clone();
        let marked = tokio::task::spawn_blocking(move || transitions.mark_delivered(key, receipt))
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
                tracing::error!(subsystem = "atm_cron", action = "lifecycle_receipt_failed", schedule = %schedule_id, %error, "cron lifecycle receipt persistence failed: {error}")
            }
            Err(error) => {
                tracing::error!(subsystem = "atm_cron", action = "lifecycle_receipt_task_failed", schedule = %schedule_id, %error, "cron lifecycle receipt task failed: {error}")
            }
        }
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
        let reply = self
            .deps
            .runner
            .run_cron(
                target,
                &request(cron, schedule, target, HookName::cron_tick()),
                deadline(cron, schedule),
            )
            .await
            .map(|outcome| outcome.reply);
        self.execution_for_reply(cron, schedule, reply).await
    }

    async fn execution_for_reply(
        &self,
        cron: &AtmCron,
        schedule: &CronSchedule,
        reply: Result<CronTickReply, AtmError>,
    ) -> Execution {
        match reply {
            Err(error) => Execution::new(runner_failure(error), Vec::new()),
            Ok(reply) => match reply {
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
                CronTickReply::Handled { actions, output } => {
                    self.dispatch_handled_actions(cron, schedule, actions, output)
                        .await
                }
            },
        }
    }

    async fn dispatch_handled_actions(
        &self,
        cron: &AtmCron,
        schedule: &CronSchedule,
        actions: Vec<HookAction>,
        output: Vec<atm_core::boundary::CronModuleOutput>,
    ) -> Execution {
        let count = actions.len();
        if actions.is_empty() {
            return Execution::new(
                CronRunTransition::Success {
                    outcome: CronSuccess::Actions { count },
                },
                output,
            );
        }
        let issuer = SchedulePermitIssuer::for_schedule(schedule);
        let result = cron
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
            .await;
        let transition = match result {
            Ok(_) => CronRunTransition::Success {
                outcome: CronSuccess::Actions { count },
            },
            Err(error) => failure(
                CronFailureKind::ActionFailed,
                Some(error.code()),
                "cron action admission failed",
            ),
        };
        Execution::new(transition, output)
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
        hook::{CronHookOutcome, CronTickReply},
        test_support::TEST_SENDER,
    };
    use atm_runtime_test_support::{
        InMemoryCronScheduleStore, RecordingCronLifecycleNudger, RecordingCronModuleMailDelivery,
        RecordingHookActionDispatcher, ScriptedCronHookRunner,
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
    ) -> (
        AtmCron,
        CronSchedule,
        Arc<RecordingHookActionDispatcher>,
        Arc<RecordingCronModuleMailDelivery>,
    ) {
        let dispatcher = Arc::new(RecordingHookActionDispatcher::default());
        let module_mail = Arc::new(RecordingCronModuleMailDelivery::default());
        module_mail
            .replies
            .lock()
            .expect("module mail replies")
            .push_back(Ok(atm_storage::DeliveryReceipt::Accepted {
                receipt_id: "recorded".to_owned(),
            }));
        let cron = AtmCron::new(crate::AtmCronDeps {
            store: Arc::new(InMemoryCronScheduleStore::new()),
            runner: Arc::new(atm_runtime_test_support::DummyHookRunner::default()),
            dispatcher: dispatcher.clone(),
            clock: Arc::new(crate::ManualClock::new(
                "2026-10-19T00:00:00Z".parse().expect("timestamp"),
            )),
            host: None,
            jitter: Arc::new(crate::SeqJitter::default()),
        })
        .with_typed_execution(CronEngineDeps {
            runner,
            transitions,
            lifecycle_delivery: Arc::new(RecordingCronLifecycleNudger::default()),
            module_mail_delivery: module_mail.clone(),
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
        (cron, schedule, dispatcher, module_mail)
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
        let (cron, schedule, _, module_mail) = typed_cron(runner, transitions.clone()).await;
        transitions
            .commit_results
            .lock()
            .expect("commit results")
            .extend([
                Ok(atm_storage::CronCommitResult::Applied { intents: vec![] }),
                Ok(atm_storage::CronCommitResult::Applied {
                    intents: vec![atm_storage::CronDeliveryIntent::ModuleMail(
                        atm_storage::CronModuleMailIntent {
                            key: atm_storage::CronDeliveryKey::ModuleMail {
                                schedule_id: schedule.id,
                                generation: schedule.generation,
                                transition_seq: 2,
                                output_index: 0,
                            },
                            message_id: atm_core::schema::AtmMessageId::new(),
                            team: schedule.team.clone(),
                            recipient: schedule.created_by.clone(),
                            name: atm_core::boundary::CronName::parse(&schedule.name)
                                .expect("cron name"),
                            output: output("ordered module output"),
                        },
                    )],
                }),
            ]);

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
        let delivered = module_mail.delivered.lock().expect("module mail delivered");
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].1, schedule.id);
        assert_eq!(delivered[0].2, schedule.generation);
        assert_eq!(transitions.delivered.lock().expect("receipts").len(), 1);
    }

    #[tokio::test]
    async fn pending_module_mail_retries_with_same_id_but_unknown_is_not_resent() {
        let runner = Arc::new(ScriptedCronHookRunner::default());
        let transitions = Arc::new(atm_storage::testing::InMemoryCronTransitionStore::default());
        let (cron, schedule, _, module_mail) = typed_cron(runner, transitions.clone()).await;
        let key = atm_storage::CronDeliveryKey::ModuleMail {
            schedule_id: schedule.id,
            generation: schedule.generation,
            transition_seq: 9,
            output_index: 0,
        };
        let intent = atm_storage::CronModuleMailIntent {
            key,
            message_id: atm_core::schema::AtmMessageId::new(),
            team: schedule.team.clone(),
            recipient: schedule.created_by.clone(),
            name: atm_core::boundary::CronName::parse(&schedule.name).expect("cron name"),
            output: output("retry with stable identity"),
        };
        let stable_id = intent.message_id;
        {
            let mut pending = transitions.pending_intents.lock().expect("pending intents");
            pending.push(atm_storage::PendingCronDelivery::ReadyToSend(
                atm_storage::CronDeliveryIntent::ModuleMail(intent.clone()),
            ));
            pending.push(atm_storage::PendingCronDelivery::ReconciliationRequired {
                intent: atm_storage::CronDeliveryIntent::ModuleMail(intent),
                retry_cause: atm_storage::DeliveryRetryCause::parse(
                    "prior handoff uncertain".into(),
                )
                .expect("retry cause"),
            });
        }

        let engine = cron.typed_engine.as_ref().expect("typed engine");
        assert!(
            engine
                .commit(
                    &cron,
                    &schedule,
                    CronRunTransition::Success {
                        outcome: atm_storage::CronSuccess::Ok
                    },
                    vec![],
                )
                .await
                .expect("commit retries pending work")
        );

        let delivered = module_mail
            .delivered
            .lock()
            .expect("module mail deliveries");
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].0.message_id, stable_id);
        let receipts = transitions.delivered.lock().expect("receipts");
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].0, key);
    }

    #[tokio::test]
    async fn action_reply_uses_scheduler_permit_for_canonical_admission() {
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
                        request: serde_json::json!({
                            "home_dir": "/home/test",
                            "current_dir": "/work/test"
                        }),
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
        let (cron, schedule, dispatcher, _) = typed_cron(runner, transitions.clone()).await;

        let outcome = tick::run_one(&cron, &schedule).await;
        assert_eq!(outcome, tick::CronRunOutcome::Actions(1));
        let calls = dispatcher.calls.lock().expect("dispatch calls");
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].cron_permit,
            Some((schedule.id, schedule.generation))
        );
        let writes = transitions.writes.lock().expect("writes");
        assert!(matches!(
            writes[1].transition,
            CronRunTransition::Success { .. }
        ));
        assert_eq!(writes[1].outputs, vec![output("retain output")]);
    }
}
