//! Hook and cron control routes (ADR-068 D5, ADR-069 D1, D4).
//!
//! Every route is local-only and requires the caller on the team roster; no
//! lead check applies. Registry writes refresh the active snapshot once, in
//! the same synchronous bridge call as the store write (design D-8). Cron
//! routes pass the scheduler's own results and errors through unchanged.

use std::sync::Arc;

use atm_core::api::{ApiResponse, AuthenticatedIngress, RequestDeadline};
use atm_core::boundary::{CronScheduler, CronStartSpec, HookName, HookRegistration};
use atm_core::error::AtmError;
use atm_core::hook::{
    CronEndRequest, CronListQuery, CronStartRequest, HookDisableRequest, HookListQuery,
    HookRegisterRequest,
};
use atm_core::protocol::ResponseEnvelope;
use atm_core::types::{AgentName, IsoTimestamp, TeamName};

use crate::hook_filter::ActiveHookSnapshot;
use crate::storage_and_nudge_router::{StorageAndNudgeRouter, validate_heartbeat_member};

const SNAPSHOT_REFRESH_FAILED: &str =
    "registration committed; snapshot refresh failed; the next hooked call reloads it";

fn require_local(ingress: AuthenticatedIngress) -> Result<(), AtmError> {
    if ingress != AuthenticatedIngress::Local {
        return Err(AtmError::validation(
            "hook and cron routes are available only through authenticated local HTTP adapters",
        ));
    }
    Ok(())
}

fn check_timeout(timeout_ms: u32, max: u32, name: &str) -> Result<(), AtmError> {
    if timeout_ms == 0 || timeout_ms > max {
        return Err(AtmError::hook_module_invalid(&format!(
            "{name} must be 1..={max} milliseconds"
        )));
    }
    Ok(())
}

/// Refreshes the snapshot after a committed write. A failure keeps the row and
/// reports the refresh, since the next hooked call reloads from the store.
fn refresh_after_commit(snapshot: &ActiveHookSnapshot) -> Result<(), AtmError> {
    snapshot
        .refresh()
        .map_err(|cause| AtmError::daemon_unavailable_with_cause(SNAPSHOT_REFRESH_FAILED, cause))
}

impl StorageAndNudgeRouter {
    pub(crate) fn check_roster_member(
        &self,
        team: &TeamName,
        member: &AgentName,
    ) -> Result<(), AtmError> {
        validate_heartbeat_member(&self.service_runtime, team, member)
    }

    fn snapshot(&self) -> Result<Arc<ActiveHookSnapshot>, AtmError> {
        self.hooks
            .snapshot
            .clone()
            .ok_or_else(|| AtmError::daemon_unavailable("hook snapshot was not installed"))
    }

    pub(crate) async fn hook_register(
        &self,
        request: HookRegisterRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        require_local(ingress)?;
        let hook = HookName::parse_task_hook(&request.hook)?;
        check_timeout(
            request.timeout_ms,
            atm_core::boundary::HOOK_TIMEOUT_MAX_MS,
            "timeout_ms",
        )?;
        let custom = request.check_limits()?;
        self.check_roster_member(&request.caller_team, &request.caller_identity)?;
        let snapshot = self.snapshot()?;
        let store = self.service_runtime.hook_registration_store()?;
        let registration = HookRegistration {
            team: request.caller_team,
            phase: request.phase,
            hook,
            module_path: request.module_path,
            cwd: request.cwd,
            timeout_ms: request.timeout_ms,
            custom,
            active: true,
            registered_by: request.caller_identity,
            registered_at: IsoTimestamp::now(),
            disabled_at: None,
        };
        let row = self
            .control_path_sync_bridge
            .run(deadline, move || {
                let row = store.register(registration)?;
                refresh_after_commit(&snapshot)?;
                Ok(row)
            })
            .await?;
        Ok(ApiResponse::new(ResponseEnvelope::HookRegistered(row)))
    }

    pub(crate) async fn hook_disable(
        &self,
        request: HookDisableRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        require_local(ingress)?;
        let hook = request
            .hook
            .as_deref()
            .map(HookName::parse_task_hook)
            .transpose()?;
        self.check_roster_member(&request.caller_team, &request.caller_identity)?;
        let snapshot = self.snapshot()?;
        let store = self.service_runtime.hook_registration_store()?;
        let rows = self
            .control_path_sync_bridge
            .run(deadline, move || {
                let rows = store.disable(
                    &request.caller_team,
                    &request.phase,
                    hook.as_ref(),
                    IsoTimestamp::now(),
                )?;
                refresh_after_commit(&snapshot)?;
                Ok(rows)
            })
            .await?;
        Ok(ApiResponse::new(ResponseEnvelope::HookDisabled(rows)))
    }

    pub(crate) async fn hook_list(
        &self,
        request: HookListQuery,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        require_local(ingress)?;
        self.check_roster_member(&request.caller_team, &request.caller_identity)?;
        let store = self.service_runtime.hook_registration_store()?;
        let rows = self
            .control_path_sync_bridge
            .run(deadline, move || {
                store.list(&request.caller_team, request.phase.as_ref())
            })
            .await?;
        Ok(ApiResponse::new(ResponseEnvelope::HookList(rows)))
    }

    pub(crate) async fn cron_start(
        &self,
        request: CronStartRequest,
        ingress: AuthenticatedIngress,
    ) -> Result<ApiResponse, AtmError> {
        require_local(ingress)?;
        check_timeout(
            request.timeout_ms,
            atm_core::boundary::CRON_TIMEOUT_MAX_MS,
            "timeout_ms",
        )?;
        let custom = request.check_limits()?;
        self.check_roster_member(&request.caller_team, &request.caller_identity)?;
        let scheduler = self.cron_scheduler()?;
        let schedule = scheduler
            .start(CronStartSpec {
                team: request.caller_team,
                phase: request.phase,
                name: request.name,
                cron_expr: request.cron_expr,
                module_path: request.module_path,
                cwd: request.cwd,
                args: request.args,
                timeout_ms: request.timeout_ms,
                custom,
                stop: request.stop,
                created_by: request.caller_identity,
            })
            .await?;
        Ok(ApiResponse::new(ResponseEnvelope::CronStarted(schedule)))
    }

    pub(crate) async fn cron_end(
        &self,
        request: CronEndRequest,
        ingress: AuthenticatedIngress,
    ) -> Result<ApiResponse, AtmError> {
        require_local(ingress)?;
        self.check_roster_member(&request.caller_team, &request.caller_identity)?;
        let scheduler = self.cron_scheduler()?;
        let rows = scheduler
            .end(
                &request.caller_team,
                &request.phase,
                request.name.as_deref(),
            )
            .await?;
        Ok(ApiResponse::new(ResponseEnvelope::CronEnded(rows)))
    }

    pub(crate) async fn cron_list(
        &self,
        request: CronListQuery,
        ingress: AuthenticatedIngress,
    ) -> Result<ApiResponse, AtmError> {
        require_local(ingress)?;
        self.check_roster_member(&request.caller_team, &request.caller_identity)?;
        let scheduler = self.cron_scheduler()?;
        let rows = scheduler
            .list(Some(&request.caller_team), request.phase.as_ref())
            .await?;
        Ok(ApiResponse::new(ResponseEnvelope::CronList(rows)))
    }

    fn cron_scheduler(&self) -> Result<Arc<dyn CronScheduler>, AtmError> {
        self.hooks
            .cron
            .get()
            .cloned()
            .ok_or_else(|| AtmError::daemon_unavailable("cron scheduler not installed"))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    use atm_core::api::AuthenticatedIngress;
    use atm_core::boundary::{CronScheduler, HookRegistrationStore, RecordingCronScheduler};
    use atm_core::error::AtmErrorCode;
    use atm_core::hook::{
        CronEndRequest, CronListQuery, CronStartRequest, HookDisableRequest, HookListQuery,
        HookRegisterRequest,
    };
    use atm_core::protocol::ResponseEnvelope;
    use atm_runtime_test_support::InMemoryHookRegistrationStore;
    use serde_json::{Value, json};
    use tokio::sync::Notify;

    use crate::hook_chains::ChainTracker;
    use crate::hook_filter::ActiveHookSnapshot;
    use crate::hook_filter::test_support::{
        agent, deadline, handled_action, hook, phase, reply, team,
    };
    use crate::storage_and_nudge_router::StorageAndNudgeRouter;
    use crate::storage_and_nudge_router::tests::{Fixture, fixture};
    use crate::task_commands::NormalTaskCommands;

    /// A router with an in-memory hook store, its snapshot, and a recording cron scheduler.
    struct Routes {
        _fixture: Fixture,
        router: StorageAndNudgeRouter,
        hooks: Arc<InMemoryHookRegistrationStore>,
        cron: Arc<RecordingCronScheduler>,
    }

    fn routes() -> Routes {
        let fixture = fixture(true, None, None);
        let hooks = Arc::new(InMemoryHookRegistrationStore::default());
        let mut router = fixture.router.clone();
        router.service_runtime = router
            .service_runtime
            .clone()
            .with_hook_registration_store(hooks.clone());
        let snapshot = ActiveHookSnapshot::load(hooks.clone()).expect("snapshot");
        let router = router.with_hook_snapshot(Arc::new(snapshot));
        let cron = Arc::new(RecordingCronScheduler::default());
        router
            .install_cron_scheduler(cron.clone() as Arc<dyn CronScheduler>)
            .expect("install cron");
        Routes {
            _fixture: fixture,
            router,
            hooks,
            cron,
        }
    }

    fn register_request(caller: &str, hook_name: &str, phase_name: &str) -> HookRegisterRequest {
        HookRegisterRequest {
            caller_identity: agent(caller),
            caller_team: team(),
            phase: phase(phase_name),
            hook: hook_name.to_owned(),
            module_path: "/modules/gate.wasm".into(),
            cwd: "/work".into(),
            timeout_ms: 1_000,
            custom: json!({"goal": "ship"}),
        }
    }

    async fn register(
        routes: &Routes,
        request: HookRegisterRequest,
    ) -> Result<(), atm_core::error::AtmError> {
        routes
            .router
            .hook_register(request, AuthenticatedIngress::Local, deadline())
            .await
            .map(|_| ())
    }

    fn cron_request(caller: &str, timeout_ms: u32, custom: Value) -> CronStartRequest {
        CronStartRequest {
            caller_identity: agent(caller),
            caller_team: team(),
            phase: phase("bk"),
            name: "nightly".to_owned(),
            cron_expr: "0 3 * * *".to_owned(),
            module_path: "/modules/nightly.wasm".into(),
            cwd: "/work".into(),
            args: vec!["--quick".to_owned()],
            timeout_ms,
            stop: atm_core::boundary::StopCondition::Hook,
            custom,
        }
    }

    fn list_request() -> HookListQuery {
        HookListQuery {
            caller_identity: agent("sender"),
            caller_team: team(),
            phase: None,
        }
    }

    #[tokio::test]
    async fn disable_keeps_rows_and_list_returns_them() {
        let routes = routes();
        register(&routes, register_request("sender", "task.assign", "bk"))
            .await
            .expect("register");
        routes
            .router
            .hook_disable(
                HookDisableRequest {
                    caller_identity: agent("sender"),
                    caller_team: team(),
                    phase: phase("bk"),
                    hook: None,
                },
                AuthenticatedIngress::Local,
                deadline(),
            )
            .await
            .expect("disable");
        let response = routes
            .router
            .hook_list(list_request(), AuthenticatedIngress::Local, deadline())
            .await
            .expect("list");
        match response.into_inner() {
            ResponseEnvelope::HookList(rows) => {
                assert_eq!(rows.len(), 1, "the disabled row is kept");
                assert!(!rows[0].active);
            }
            other => panic!("expected HookList, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn register_refuses_cron_tick_unknown_name_and_over_cap_timeout() {
        let routes = routes();
        let tick = register(&routes, register_request("sender", "cron.tick", "bk"))
            .await
            .expect_err("cron.tick is not a task hook");
        assert!(tick.is_validation() || tick.code() == AtmErrorCode::HookModuleInvalid);
        let unknown = register(&routes, register_request("sender", "task.bogus", "bk"))
            .await
            .expect_err("unknown task hook");
        assert!(unknown.is_validation() || unknown.code() == AtmErrorCode::HookModuleInvalid);
        let mut over = register_request("sender", "task.assign", "bk");
        over.timeout_ms = atm_core::boundary::HOOK_TIMEOUT_MAX_MS + 1;
        let capped = register(&routes, over).await.expect_err("over cap timeout");
        assert!(capped.message().contains("timeout_ms must be 1..="));
        assert!(routes.hooks.active_all().expect("rows").is_empty());
    }

    #[tokio::test]
    async fn register_same_hook_in_a_second_phase_adds_a_chain_row() {
        let routes = routes();
        register(&routes, register_request("sender", "task.assign", "bj"))
            .await
            .expect("bj");
        register(&routes, register_request("sender", "task.assign", "bk"))
            .await
            .expect("bk");
        let rows = routes.hooks.active_all().expect("rows");
        let phases: Vec<&str> = rows.iter().map(|row| row.phase.as_str()).collect();
        assert_eq!(phases, vec!["bj", "bk"]);
    }

    #[tokio::test]
    async fn register_from_non_lead_member_succeeds_and_off_roster_fails() {
        let routes = routes();
        register(&routes, register_request("sender", "task.assign", "bk"))
            .await
            .expect("a roster member that is not the lead may register");
        let off = register(&routes, register_request("stranger", "task.assign", "bk"))
            .await
            .expect_err("off-roster caller");
        assert!(
            off.message().contains("was not found in team"),
            "off-roster caller is refused: {off}"
        );
    }

    #[tokio::test]
    async fn reregister_active_key_keeps_chain_position() {
        let routes = routes();
        register(&routes, register_request("sender", "task.assign", "bj"))
            .await
            .expect("bj");
        register(&routes, register_request("sender", "task.assign", "bk"))
            .await
            .expect("bk");
        let mut again = register_request("sender", "task.assign", "bj");
        again.timeout_ms = 2_000;
        register(&routes, again).await.expect("re-register bj");
        let phases: Vec<String> = routes
            .hooks
            .active_all()
            .expect("rows")
            .iter()
            .map(|row| row.phase.as_str().to_owned())
            .collect();
        assert_eq!(phases, vec!["bj".to_owned(), "bk".to_owned()]);
    }

    #[tokio::test]
    async fn register_and_disable_each_refresh_the_snapshot_once() {
        let routes = routes();
        let before = routes.hooks.calls.load(Ordering::SeqCst);
        register(&routes, register_request("sender", "task.assign", "bk"))
            .await
            .expect("register");
        // One register and one refresh read of the store.
        assert_eq!(routes.hooks.calls.load(Ordering::SeqCst) - before, 2);
        let before = routes.hooks.calls.load(Ordering::SeqCst);
        routes
            .router
            .hook_disable(
                HookDisableRequest {
                    caller_identity: agent("sender"),
                    caller_team: team(),
                    phase: phase("bk"),
                    hook: None,
                },
                AuthenticatedIngress::Local,
                deadline(),
            )
            .await
            .expect("disable");
        assert_eq!(routes.hooks.calls.load(Ordering::SeqCst) - before, 2);
    }

    #[tokio::test]
    async fn snapshot_order_equals_store_order() {
        let routes = routes();
        register(&routes, register_request("sender", "task.assign", "bj"))
            .await
            .expect("bj");
        register(&routes, register_request("sender", "task.assign", "bk"))
            .await
            .expect("bk");
        let snapshot = routes.router.hooks.snapshot.clone().expect("snapshot");
        let from_snapshot: Vec<String> = snapshot
            .active(&team(), &hook("task.assign"))
            .expect("active")
            .iter()
            .map(|row| row.phase.as_str().to_owned())
            .collect();
        let from_store: Vec<String> = routes
            .hooks
            .active_all()
            .expect("rows")
            .iter()
            .map(|row| row.phase.as_str().to_owned())
            .collect();
        assert_eq!(from_snapshot, from_store);
    }

    #[tokio::test]
    async fn concurrent_register_and_disable_leave_snapshot_equal_to_store() {
        // Deterministic stand-in for interleaving: each write is awaited to completion
        // before the next, and the snapshot is checked against the store after each step.
        let routes = routes();
        let snapshot = routes.router.hooks.snapshot.clone().expect("snapshot");
        for phase_name in ["bj", "bk", "bl"] {
            register(
                &routes,
                register_request("sender", "task.assign", phase_name),
            )
            .await
            .expect("register");
            routes
                .router
                .hook_disable(
                    HookDisableRequest {
                        caller_identity: agent("sender"),
                        caller_team: team(),
                        phase: phase(phase_name),
                        hook: None,
                    },
                    AuthenticatedIngress::Local,
                    deadline(),
                )
                .await
                .expect("disable");
            let active: Vec<String> = snapshot
                .active(&team(), &hook("task.assign"))
                .expect("active")
                .iter()
                .map(|row| row.phase.as_str().to_owned())
                .collect();
            let stored: Vec<String> = routes
                .hooks
                .active_all()
                .expect("rows")
                .iter()
                .filter(|row| row.hook.as_str() == "task.assign")
                .map(|row| row.phase.as_str().to_owned())
                .collect();
            assert_eq!(active, stored);
        }
    }

    #[tokio::test]
    async fn disable_during_in_flight_hook_does_not_cancel_its_actions() {
        let routes = routes();
        register(&routes, register_request("sender", "task.assign", "bk"))
            .await
            .expect("register");
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let runner = Arc::new(atm_core::boundary::DummyHookRunner::default());
        {
            let entered = entered.clone();
            let release = release.clone();
            *runner.before_reply.lock().expect("before reply") = Some(Arc::new(move |_index| {
                let entered = entered.clone();
                let release = release.clone();
                Box::pin(async move {
                    entered.notify_one();
                    release.notified().await;
                })
            }));
        }
        let mut request = crate::hook_filter::test_support::assign_request(false);
        request.to = Some("recipient@test-team".parse().expect("address"));
        reply(
            &runner,
            atm_core::hook::HookReply::Handled(vec![handled_action("task.assign", &request)]),
        );
        let commands = Arc::new(crate::hook_filter::HookedTaskCommands::new(
            Arc::new(NormalTaskCommands::new(routes.router.clone())),
            Arc::new(routes.router.unhooked()),
            routes.router.hooks.snapshot.clone().expect("snapshot"),
            runner.clone(),
            Arc::new(ChainTracker::new()),
        ));
        let in_flight = {
            let commands = commands.clone();
            tokio::spawn(async move {
                use atm_core::boundary::{TaskAssign, TaskCallCtx, TaskCommands};
                let ctx = TaskCallCtx {
                    caller_identity: agent("sender"),
                    ..crate::hook_filter::test_support::ctx(deadline())
                };
                commands.assign(TaskAssign(Box::new(request)), &ctx).await
            })
        };
        entered.notified().await;
        routes
            .router
            .hook_disable(
                HookDisableRequest {
                    caller_identity: agent("sender"),
                    caller_team: team(),
                    phase: phase("bk"),
                    hook: None,
                },
                AuthenticatedIngress::Local,
                deadline(),
            )
            .await
            .expect("disable while the hook is in flight");
        release.notify_one();
        let response = in_flight
            .await
            .expect("join")
            .expect("in-flight chain completes");
        // The chain's last action answers the hooked command; a send response proves
        // the in-flight action still ran after the disable.
        match response.into_inner() {
            ResponseEnvelope::Send(_) => {}
            other => panic!("expected the action's Send response, got {other:?}"),
        }
        assert_eq!(runner.calls.lock().expect("calls").len(), 1);
    }

    #[tokio::test]
    async fn register_refresh_failure_keeps_row_and_reports() {
        let routes = routes();
        routes.hooks.fail_active_all.store(true, Ordering::SeqCst);
        let error = register(&routes, register_request("sender", "task.assign", "bk"))
            .await
            .expect_err("refresh failure is reported");
        assert!(error.message().contains(
            "registration committed; snapshot refresh failed; the next hooked call reloads it"
        ));
        routes.hooks.fail_active_all.store(false, Ordering::SeqCst);
        assert_eq!(routes.hooks.active_all().expect("rows").len(), 1);
    }

    #[tokio::test]
    async fn cron_start_refuses_cron_timeout_above_30s() {
        let routes = routes();
        for timeout in [0, 30_001] {
            let error = routes
                .router
                .cron_start(
                    cron_request("sender", timeout, json!({})),
                    AuthenticatedIngress::Local,
                )
                .await
                .expect_err("timeout outside 1..=30000");
            assert!(error.message().contains(&format!(
                "timeout_ms must be 1..={} milliseconds",
                atm_core::boundary::CRON_TIMEOUT_MAX_MS
            )));
        }
        routes
            .router
            .cron_start(
                cron_request("sender", atm_core::boundary::CRON_TIMEOUT_MAX_MS, json!({})),
                AuthenticatedIngress::Local,
            )
            .await
            .expect("30 000 ms is accepted");
        assert_eq!(routes.cron.calls.lock().expect("calls").len(), 1);
    }

    #[tokio::test]
    async fn custom_non_object_and_over_64k_refused() {
        let routes = routes();
        let non_object = register(
            &routes,
            HookRegisterRequest {
                custom: json!([1]),
                ..register_request("sender", "task.assign", "bk")
            },
        )
        .await
        .expect_err("non-object custom");
        assert!(non_object.is_validation() || non_object.code() == AtmErrorCode::HookModuleInvalid);
        let oversize = json!({"x": "x".repeat(atm_core::hook::HOOK_CUSTOM_MAX_BYTES)});
        let over = register(
            &routes,
            HookRegisterRequest {
                custom: oversize.clone(),
                ..register_request("sender", "task.assign", "bk")
            },
        )
        .await
        .expect_err("over 64 KiB custom");
        assert!(
            over.message()
                .contains("custom must be an object of at most")
        );
        let cron = routes
            .router
            .cron_start(
                cron_request("sender", 1_000, oversize),
                AuthenticatedIngress::Local,
            )
            .await
            .expect_err("cron over 64 KiB custom");
        assert!(
            cron.message()
                .contains("custom must be an object of at most")
        );
        assert!(routes.hooks.active_all().expect("rows").is_empty());
        assert!(routes.cron.calls.lock().expect("calls").is_empty());
    }

    #[tokio::test]
    async fn oversize_name_args_and_paths_refused() {
        let routes = routes();
        let mut long_name = cron_request("sender", 1_000, json!({}));
        long_name.name = "n".repeat(atm_core::hook::CRON_NAME_MAX_BYTES + 1);
        assert!(
            routes
                .router
                .cron_start(long_name, AuthenticatedIngress::Local)
                .await
                .is_err()
        );
        let mut many_args = cron_request("sender", 1_000, json!({}));
        many_args.args = vec!["a".to_owned(); atm_core::hook::CRON_ARGS_MAX + 1];
        assert!(
            routes
                .router
                .cron_start(many_args, AuthenticatedIngress::Local)
                .await
                .is_err()
        );
        let mut long_path = register_request("sender", "task.assign", "bk");
        long_path.module_path = std::path::PathBuf::from(format!(
            "/{}",
            "p".repeat(atm_core::hook::HOOK_PATH_MAX_BYTES)
        ));
        assert!(register(&routes, long_path).await.is_err());
        assert!(routes.hooks.active_all().expect("rows").is_empty());
        assert!(routes.cron.calls.lock().expect("calls").is_empty());
    }

    #[tokio::test]
    async fn absent_custom_is_an_empty_object() {
        let routes = routes();
        let mut request = register_request("sender", "task.assign", "bk");
        request.custom = Value::Null;
        register(&routes, request)
            .await
            .expect("absent custom registers");
        let rows = routes.hooks.active_all().expect("rows");
        assert_eq!(rows[0].custom, serde_json::Map::new());
    }

    #[tokio::test]
    async fn cron_arms_pass_through_to_the_scheduler() {
        let routes = routes();
        let custom = json!({"owner": "ops"});
        let response = routes
            .router
            .cron_start(
                cron_request("sender", 5_000, custom.clone()),
                AuthenticatedIngress::Local,
            )
            .await
            .expect("cron start");
        match response.into_inner() {
            ResponseEnvelope::CronStarted(schedule) => {
                assert_eq!(schedule.created_by, agent("sender"));
                assert_eq!(schedule.custom, custom.as_object().expect("object").clone());
            }
            other => panic!("expected CronStarted, got {other:?}"),
        }
        routes
            .router
            .cron_list(
                CronListQuery {
                    caller_identity: agent("sender"),
                    caller_team: team(),
                    phase: None,
                },
                AuthenticatedIngress::Local,
            )
            .await
            .expect("cron list");
        routes
            .router
            .cron_end(
                CronEndRequest {
                    caller_identity: agent("sender"),
                    caller_team: team(),
                    phase: phase("bk"),
                    name: None,
                },
                AuthenticatedIngress::Local,
            )
            .await
            .expect("cron end");
        assert_eq!(
            *routes.cron.calls.lock().expect("calls"),
            vec!["start", "list", "end"]
        );
    }

    #[tokio::test]
    async fn cron_routes_without_scheduler_are_unavailable() {
        let fixture = fixture(true, None, None);
        let error = fixture
            .router
            .cron_start(
                cron_request("sender", 1_000, json!({})),
                AuthenticatedIngress::Local,
            )
            .await
            .expect_err("no scheduler installed");
        assert_eq!(error.code(), AtmErrorCode::DaemonUnavailable);
        assert!(error.message().contains("cron scheduler not installed"));
    }
}
