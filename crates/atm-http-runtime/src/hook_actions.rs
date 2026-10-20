//! The one production hook-action dispatcher.
//!
//! [`UnhookedRouter`] runs each hook action through the router with no task
//! filter installed, so an action never re-enters the chain that produced it
//! (ANC-008). Every action is checked before the first one runs.

use std::time::Instant;

use async_trait::async_trait;
use atm_core::api::AuthenticatedIngress;
use atm_core::boundary::{DispatchSpec, HookActionDispatcher, HookName, sealed};
use atm_core::error::AtmError;
use atm_core::hook::HookAction;
use atm_core::protocol::next_request_id;

use crate::CanonicalWriteHandler;
use crate::hook_filter::elapsed_ms;
use crate::storage_and_nudge_router::StorageAndNudgeRouter;

/// A router clone with no task filter; built only by `StorageAndNudgeRouter::unhooked`.
pub struct UnhookedRouter(StorageAndNudgeRouter);

impl UnhookedRouter {
    pub(crate) fn new(router: StorageAndNudgeRouter) -> Self {
        Self(router)
    }
}

impl sealed::Sealed for UnhookedRouter {}

#[async_trait]
impl HookActionDispatcher for UnhookedRouter {
    async fn dispatch_actions(
        &self,
        spec: DispatchSpec,
        actions: Vec<HookAction>,
    ) -> Result<Vec<(Option<HookName>, atm_core::api::ApiResponse)>, AtmError> {
        let deadline = spec
            .deadline
            .ok_or_else(|| AtmError::hook_malformed("hook action dispatch has no deadline"))?;
        let mut requests = Vec::with_capacity(actions.len());
        for action in &actions {
            let request = action.into_api_request(&spec.team, &spec.default_as)?;
            let identity = action.run_as.as_ref().unwrap_or(&spec.default_as);
            self.0
                .check_roster_member(&spec.team, identity)
                .map_err(|_| {
                    AtmError::hook_malformed(&format!(
                        "action {}: run_as {identity} is not on the roster",
                        action.command
                    ))
                })?;
            requests.push((
                HookName::parse_task_hook(&action.command).ok(),
                action.command.clone(),
                identity.clone(),
                request,
            ));
        }
        if spec.dry_run {
            return Ok(Vec::new());
        }
        let total = requests.len();
        let mut done = Vec::with_capacity(total);
        for (index, (hook_name, command, run_as, request)) in requests.into_iter().enumerate() {
            let started = Instant::now();
            let result = self
                .0
                .dispatch_with_request_id(
                    request,
                    AuthenticatedIngress::Local,
                    deadline,
                    next_request_id(),
                )
                .await;
            log_action(&spec, &command, &run_as, result.is_ok(), started);
            let response = result.map_err(|error| {
                error.with_context(format!(
                    "hook action {}/{total} ({command}) failed after {index} committed",
                    index + 1
                ))
            })?;
            done.push((hook_name, response));
        }
        Ok(done)
    }
}

/// One ADR-067 `action` entry per dispatched action.
fn log_action(
    spec: &DispatchSpec,
    command: &str,
    run_as: &impl std::fmt::Display,
    ok: bool,
    started: Instant,
) {
    tracing::info!(
        subsystem = "atm_hook",
        action = "action",
        team = %spec.team,
        phase = %spec.phase,
        hook = %spec.hook,
        action_command = command,
        run_as = %run_as,
        outcome = if ok { "action_ok" } else { "action_err" },
        elapsed_ms = elapsed_ms(started),
    );
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use atm_core::api::RequestDeadline;
    use atm_core::boundary::{
        DispatchSpec, HookActionDispatcher, HookName, TaskAssign, TaskCallCtx, TaskClose,
        TaskCommands,
    };
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::hook::{HookAction, HookActionsResponse, HookReply};
    use atm_core::protocol::ResponseEnvelope;
    use atm_core::send::WriteRequest;
    use atm_runtime_test_support::InMemoryHookRegistrationStore;

    use crate::hook_chains::ChainTracker;
    use crate::hook_filter::test_support::{
        agent, ctx, deadline, handled_action, hook, phase, reply, shape, team,
    };
    use crate::hook_filter::{ActiveHookSnapshot, HookedTaskCommands};
    use crate::storage_and_nudge_router::StorageAndNudgeRouter;
    use crate::storage_and_nudge_router::tests::{
        Fixture, fixture, task_write_request, write_request,
    };
    use crate::task_commands::NormalTaskCommands;
    use atm_core::boundary::HookRunner;
    use atm_core::boundary::TaskCloseOutcome;
    use atm_core::boundary::TaskOp;

    fn spec(deadline: RequestDeadline, dry_run: bool) -> DispatchSpec {
        DispatchSpec {
            hook: hook("task.assign"),
            team: team(),
            phase: phase("bk"),
            default_as: agent("sender"),
            deadline: Some(deadline),
            dry_run,
        }
    }

    /// The hooked caller is a roster member, as the filter's `default_as` requires.
    fn sender_ctx() -> TaskCallCtx {
        TaskCallCtx {
            caller_identity: agent("sender"),
            ..ctx(deadline())
        }
    }

    /// A task assign addressed off the roster, so the write fails at dispatch.
    fn unroutable_assign(fixture: &Fixture, task: &str) -> HookAction {
        let mut request = assign_request(fixture, task);
        request.to = Some("ghost@test-team".parse().expect("address"));
        HookAction {
            command: "task.assign".to_owned(),
            run_as: None,
            request: serde_json::to_value(request).expect("json"),
        }
    }

    fn send_request(fixture: &Fixture) -> WriteRequest {
        write_request(fixture.home_dir.clone(), fixture.current_dir.clone())
    }

    fn assign_request(fixture: &Fixture, task: &str) -> WriteRequest {
        task_write_request(fixture, task)
    }

    fn assign_action(fixture: &Fixture, task: &str, run_as: Option<&str>) -> HookAction {
        HookAction {
            command: "task.assign".to_owned(),
            run_as: run_as.map(agent),
            request: serde_json::to_value(assign_request(fixture, task)).expect("json"),
        }
    }

    fn close_request(fixture: &Fixture, task: &str) -> WriteRequest {
        let mut request = task_write_request(fixture, task);
        request.task_op = Some(TaskOp::Close {
            outcome: TaskCloseOutcome::Completed,
            reason: None,
        });
        request
    }

    /// The real filter over the router, with the production action dispatcher.
    fn hooked(
        router: &StorageAndNudgeRouter,
        store: &Arc<InMemoryHookRegistrationStore>,
        runner: Arc<dyn HookRunner>,
    ) -> HookedTaskCommands<super::UnhookedRouter> {
        let snapshot = Arc::new(ActiveHookSnapshot::load(store.clone()).expect("snapshot"));
        HookedTaskCommands::new(
            Arc::new(NormalTaskCommands::new(router.clone())),
            Arc::new(router.unhooked()),
            snapshot,
            runner,
            Arc::new(ChainTracker::new()),
            router.control_path_sync_bridge.clone(),
        )
    }

    #[tokio::test]
    async fn send_action_persists_a_message() {
        let fixture = fixture(true, None, None);
        let action = handled_action("send", &send_request(&fixture));
        let done = fixture
            .router
            .unhooked()
            .dispatch_actions(spec(deadline(), false), vec![action])
            .await
            .expect("send action");
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].0, None);
        assert!(matches!(
            done[0].1.clone().into_inner(),
            ResponseEnvelope::Send(_)
        ));
    }

    #[tokio::test]
    async fn dry_run_handled_reply_dispatches_nothing() {
        let fixture = fixture(true, None, None);
        let action = handled_action("send", &send_request(&fixture));
        let done = fixture
            .router
            .unhooked()
            .dispatch_actions(spec(deadline(), true), vec![action])
            .await
            .expect("dry run");
        assert!(done.is_empty());
    }

    #[tokio::test]
    async fn two_assign_actions_land_in_order_with_as_actor() {
        let fixture = fixture(true, None, None);
        let actions = vec![
            assign_action(&fixture, "T-ORDER-A", None),
            assign_action(&fixture, "T-ORDER-B", Some("third")),
        ];
        let done = fixture
            .router
            .unhooked()
            .dispatch_actions(spec(deadline(), false), actions)
            .await
            .expect("assign actions");
        let commands: Vec<Option<HookName>> =
            done.iter().map(|(command, _)| command.clone()).collect();
        assert_eq!(
            commands,
            vec![
                Some(HookName::parse("task.assign").expect("task hook")),
                Some(HookName::parse("task.assign").expect("task hook")),
            ]
        );
        for task in ["T-ORDER-A", "T-ORDER-B"] {
            let row = fixture
                .task_store
                .load_task(&team(), &task.parse().expect("task id"))
                .expect("load task");
            assert!(row.is_some(), "{task} was not committed");
        }
    }

    #[tokio::test]
    async fn task_close_hook_returning_send_only_returns_hook_actions() {
        let fixture = fixture(true, None, None);
        let store = Arc::new(InMemoryHookRegistrationStore::default());
        crate::hook_filter::test_support::register(&store, "task.close", "bk", 1);
        let runner = Arc::new(atm_core::boundary::DummyHookRunner::default());
        reply(
            &runner,
            HookReply::Handled(vec![handled_action("send", &send_request(&fixture))]),
        );
        let commands = hooked(&fixture.router, &store, runner.clone());
        let response = commands
            .close(
                TaskClose(Box::new(close_request(&fixture, "T-CLOSE-SEND"))),
                &sender_ctx(),
            )
            .await
            .expect("close through hook");
        let http = crate::message_handler::map_api_response(response.clone())
            .expect("http response for hook actions");
        assert_eq!(http.status(), axum::http::StatusCode::OK);
        assert_eq!(
            http.headers()
                .get("x-atm-hook-actions")
                .and_then(|value| value.to_str().ok()),
            Some("1")
        );
        match response.into_inner() {
            ResponseEnvelope::HookActions(HookActionsResponse { dispatched }) => {
                assert_eq!(dispatched, 1);
            }
            other => panic!("expected HookActions, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn handled_returns_last_action_response_for_hooked_command() {
        let fixture = fixture(true, None, None);
        let store = Arc::new(InMemoryHookRegistrationStore::default());
        crate::hook_filter::test_support::register(&store, "task.assign", "bk", 1);
        let runner = Arc::new(atm_core::boundary::DummyHookRunner::default());
        reply(
            &runner,
            HookReply::Handled(vec![
                handled_action("send", &send_request(&fixture)),
                assign_action(&fixture, "T-LAST", None),
            ]),
        );
        let commands = hooked(&fixture.router, &store, runner.clone());
        let response = commands
            .assign(
                TaskAssign(Box::new(assign_request(&fixture, "T-LAST-HOOKED"))),
                &sender_ctx(),
            )
            .await
            .expect("assign through hook");
        assert!(
            matches!(response.into_inner(), ResponseEnvelope::Send(_)),
            "the last task.assign action response is returned"
        );
    }

    #[tokio::test]
    async fn second_action_failure_keeps_first_action_committed() {
        let fixture = fixture(true, None, None);
        let actions = vec![
            assign_action(&fixture, "T-KEEP", None),
            unroutable_assign(&fixture, "T-KEEP-2"),
        ];
        let error = fixture
            .router
            .unhooked()
            .dispatch_actions(spec(deadline(), false), actions)
            .await
            .expect_err("the unroutable assign must fail");
        assert!(
            error
                .message()
                .starts_with("hook action 2/2 (task.assign) failed after 1 committed: "),
            "unexpected message: {}",
            error.message()
        );
        let row = fixture
            .task_store
            .load_task(&team(), &"T-KEEP".parse().expect("task id"))
            .expect("load task");
        assert!(row.is_some(), "the first action stays committed");
    }

    #[tokio::test]
    async fn action_for_another_team_is_malformed_and_runs_nothing() {
        let fixture = fixture(true, None, None);
        let mut other_team = assign_request(&fixture, "T-OTHER-TEAM");
        other_team.caller_team = "other-team".parse().expect("team");
        let actions = vec![
            assign_action(&fixture, "T-NOT-RUN", None),
            HookAction {
                command: "task.assign".to_owned(),
                run_as: None,
                request: serde_json::to_value(other_team).expect("json"),
            },
        ];
        let error = fixture
            .router
            .unhooked()
            .dispatch_actions(spec(deadline(), false), actions)
            .await
            .expect_err("another team is refused");
        assert_eq!(error.code(), AtmErrorCode::HookMalformed);
        assert!(
            fixture
                .task_store
                .load_task(&team(), &"T-NOT-RUN".parse().expect("task id"))
                .expect("load task")
                .is_none(),
            "validation runs before the first action"
        );
    }

    #[tokio::test]
    async fn action_as_off_roster_is_malformed_and_runs_nothing() {
        let fixture = fixture(true, None, None);
        let actions = vec![assign_action(&fixture, "T-OFF-ROSTER", Some("stranger"))];
        let error = fixture
            .router
            .unhooked()
            .dispatch_actions(spec(deadline(), false), actions)
            .await
            .expect_err("off-roster run_as is refused");
        assert_eq!(error.code(), AtmErrorCode::HookMalformed);
        assert!(
            fixture
                .task_store
                .load_task(&team(), &"T-OFF-ROSTER".parse().expect("task id"))
                .expect("load task")
                .is_none()
        );
    }

    #[tokio::test]
    async fn hook_register_action_is_malformed() {
        let fixture = fixture(true, None, None);
        let action = HookAction {
            command: "hook.register".to_owned(),
            run_as: None,
            request: serde_json::json!({}),
        };
        let error = fixture
            .router
            .unhooked()
            .dispatch_actions(spec(deadline(), false), vec![action])
            .await
            .expect_err("hook.register is not an action");
        assert_eq!(error.code(), AtmErrorCode::HookMalformed);
    }

    #[tokio::test]
    async fn action_of_the_hooked_command_does_not_reenter_the_filter() {
        let fixture = fixture(true, None, None);
        let store = Arc::new(InMemoryHookRegistrationStore::default());
        crate::hook_filter::test_support::register(&store, "task.assign", "bk", 1);
        let runner = Arc::new(atm_core::boundary::DummyHookRunner::default());
        reply(
            &runner,
            HookReply::Handled(vec![assign_action(&fixture, "T-REENTER", None)]),
        );
        let commands = hooked(&fixture.router, &store, runner.clone());
        commands
            .assign(
                TaskAssign(Box::new(assign_request(&fixture, "T-OUTER"))),
                &sender_ctx(),
            )
            .await
            .expect("hooked assign");
        assert_eq!(
            runner.calls.lock().expect("calls").len(),
            1,
            "the action re-entered the filter"
        );
    }

    #[tokio::test]
    async fn action_body_identical_to_cli_body_gives_the_cli_result() {
        let fixture = fixture(true, None, None);
        let request = send_request(&fixture);
        let via_action = fixture
            .router
            .unhooked()
            .dispatch_actions(
                spec(deadline(), false),
                vec![handled_action("send", &request)],
            )
            .await
            .expect("action send");
        let direct = fixture
            .router
            .clone()
            .dispatch(
                atm_core::api::ApiRequest::Write(Box::new(request)),
                atm_core::api::AuthenticatedIngress::Local,
                deadline(),
            )
            .await
            .expect("direct send");
        let via_action_json =
            serde_json::to_value(via_action[0].1.clone().into_inner()).expect("action json");
        let direct_json = serde_json::to_value(direct.into_inner()).expect("cli json");
        assert_eq!(shape(&via_action_json), shape(&direct_json));
        assert!(matches!(
            via_action[0].1.clone().into_inner(),
            ResponseEnvelope::Send(_)
        ));
    }

    type CapturedEvents = Arc<std::sync::Mutex<Vec<Vec<(String, String)>>>>;

    /// Captures every tracing event as its field pairs, for the ADR-067 entries.
    #[derive(Clone, Default)]
    struct Captured(CapturedEvents);

    #[derive(Default)]
    struct FieldPairs(Vec<(String, String)>);

    impl tracing::field::Visit for FieldPairs {
        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            self.0.push((field.name().to_owned(), value.to_owned()));
        }
        fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
            self.0.push((field.name().to_owned(), value.to_string()));
        }
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.0.push((field.name().to_owned(), format!("{value:?}")));
        }
    }

    struct Capture(Captured);

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Capture {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _context: tracing_subscriber::layer::Context<'_, S>,
        ) {
            let mut fields = FieldPairs::default();
            event.record(&mut fields);
            self.0.0.lock().expect("captured log").push(fields.0);
        }
    }

    /// The `atm_hook` entries as sorted field pairs, timing excluded, in emission order.
    fn hook_entries(captured: &Captured) -> Vec<Vec<(String, String)>> {
        let events = captured.0.lock().expect("captured log");
        events
            .iter()
            .filter(|fields| {
                fields
                    .iter()
                    .any(|(k, v)| k == "subsystem" && v == "atm_hook")
            })
            .map(|fields| {
                let mut pairs: Vec<(String, String)> = fields
                    .iter()
                    .filter(|(k, _)| k != "elapsed_ms")
                    .cloned()
                    .collect();
                pairs.sort();
                pairs
            })
            .collect()
    }

    /// One expected `atm_hook` entry as sorted field pairs.
    fn entry(fields: &[(&str, &str)]) -> Vec<(String, String)> {
        let mut pairs: Vec<(String, String)> = fields
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        pairs.sort();
        pairs
    }

    /// The expected `run` entry for `task.assign` in team `test-team`, called by `sender`.
    fn run_entry(phase: &str, action_count: &str, outcome: &str) -> Vec<(String, String)> {
        entry(&[
            ("subsystem", "atm_hook"),
            ("action", "run"),
            ("team", "test-team"),
            ("phase", phase),
            ("hook", "task.assign"),
            ("caller", "sender"),
            ("action_count", action_count),
            ("outcome", outcome),
        ])
    }

    /// The expected `action` entry in phase `bk`, run as `sender` (the caller, with no `run_as`).
    fn action_entry(outcome: &str) -> Vec<(String, String)> {
        entry(&[
            ("subsystem", "atm_hook"),
            ("action", "action"),
            ("team", "test-team"),
            ("phase", "bk"),
            ("hook", "task.assign"),
            ("action_command", "task.assign"),
            ("run_as", "sender"),
            ("outcome", outcome),
        ])
    }

    #[tokio::test]
    async fn two_action_run_logs_one_run_and_two_action_entries() {
        let captured = Captured::default();
        let _guard =
            tracing::subscriber::set_default(tracing_subscriber::layer::SubscriberExt::with(
                tracing_subscriber::Registry::default(),
                Capture(captured.clone()),
            ));
        let fixture = fixture(true, None, None);
        let store = Arc::new(InMemoryHookRegistrationStore::default());
        crate::hook_filter::test_support::register(&store, "task.assign", "bk", 1);
        let runner = Arc::new(atm_core::boundary::DummyHookRunner::default());
        reply(
            &runner,
            HookReply::Handled(vec![
                assign_action(&fixture, "T-LOG-A", None),
                assign_action(&fixture, "T-LOG-B", None),
            ]),
        );
        let commands = hooked(&fixture.router, &store, runner.clone());

        commands
            .assign(
                TaskAssign(Box::new(task_write_request(&fixture, "T-LOG-IN"))),
                &sender_ctx(),
            )
            .await
            .expect("hooked assign");

        assert_eq!(
            hook_entries(&captured),
            vec![
                run_entry("bk", "2", "actions"),
                action_entry("action_ok"),
                action_entry("action_ok"),
            ]
        );
    }

    #[tokio::test]
    async fn two_phase_chain_logs_not_applicable_then_actions() {
        let captured = Captured::default();
        let _guard =
            tracing::subscriber::set_default(tracing_subscriber::layer::SubscriberExt::with(
                tracing_subscriber::Registry::default(),
                Capture(captured.clone()),
            ));
        let fixture = fixture(true, None, None);
        let store = Arc::new(InMemoryHookRegistrationStore::default());
        crate::hook_filter::test_support::register(&store, "task.assign", "bj", 1);
        crate::hook_filter::test_support::register(&store, "task.assign", "bk", 2);
        let runner = Arc::new(atm_core::boundary::DummyHookRunner::default());
        reply(&runner, HookReply::NotApplicable);
        reply(
            &runner,
            HookReply::Handled(vec![
                assign_action(&fixture, "T-CHAIN-A", None),
                assign_action(&fixture, "T-CHAIN-B", None),
            ]),
        );
        let commands = hooked(&fixture.router, &store, runner.clone());

        commands
            .assign(
                TaskAssign(Box::new(task_write_request(&fixture, "T-CHAIN-IN"))),
                &sender_ctx(),
            )
            .await
            .expect("hooked assign");

        assert_eq!(
            hook_entries(&captured),
            vec![
                run_entry("bj", "0", "not_applicable"),
                run_entry("bk", "2", "actions"),
                action_entry("action_ok"),
                action_entry("action_ok"),
            ]
        );
    }

    #[tokio::test]
    async fn runner_timeout_logs_a_timeout_run_and_no_action_entries() {
        let captured = Captured::default();
        let _guard =
            tracing::subscriber::set_default(tracing_subscriber::layer::SubscriberExt::with(
                tracing_subscriber::Registry::default(),
                Capture(captured.clone()),
            ));
        let fixture = fixture(true, None, None);
        let store = Arc::new(InMemoryHookRegistrationStore::default());
        crate::hook_filter::test_support::register(&store, "task.assign", "bk", 1);
        let runner = Arc::new(atm_core::boundary::DummyHookRunner::default());
        runner
            .replies
            .lock()
            .expect("replies")
            .push_back(Err(AtmError::hook_timeout(
                &crate::hook_filter::test_support::phase("bk"),
                "hook exceeded its deadline",
            )));
        let commands = hooked(&fixture.router, &store, runner.clone());

        let Err(error) = commands
            .assign(
                TaskAssign(Box::new(task_write_request(&fixture, "T-TIMEOUT-IN"))),
                &sender_ctx(),
            )
            .await
        else {
            panic!("a runner timeout fails the hooked assign");
        };

        assert_eq!(error.code(), AtmErrorCode::HookTimeout);
        assert_eq!(
            hook_entries(&captured),
            vec![run_entry("bk", "0", "timeout")]
        );
    }
}
