//! Hook and cron control routes (ADR-068 D5, ADR-069 D1, D4).
//!
//! Every route is local-only and requires the caller on the team roster; no
//! lead check applies. Registry writes refresh the active snapshot once, in
//! the same synchronous bridge call as the store write (design D-8). Cron
//! routes pass the scheduler's own results and errors through unchanged.

use std::sync::Arc;
use std::time::Duration;

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
        return Err(AtmError::validation_with_recovery(
            format!(
                "hook and cron routes reject {ingress:?} ingress; they require an authenticated local HTTP adapter"
            ),
            "Send the request through the authenticated local HTTP adapter.",
        )
        .with_cause(format!("request arrived through {ingress:?} ingress")));
    }
    Ok(())
}

fn check_timeout(timeout_ms: u32, max: u32, name: &str) -> Result<(), AtmError> {
    if timeout_ms == 0 || timeout_ms > max {
        return Err(AtmError::validation_with_recovery(
            format!("{name}={timeout_ms} is outside the accepted 1..={max} milliseconds"),
            format!("Set {name} to a value from 1 through {max} milliseconds and retry."),
        )
        .with_cause(format!("{name} timeout bounds validation failed")));
    }
    Ok(())
}

fn cron_scheduler_budget(deadline: RequestDeadline, operation: &str) -> Result<Duration, AtmError> {
    deadline.remaining().ok_or_else(|| {
        AtmError::daemon_unavailable(format!("request deadline expired before {operation}"))
    })
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
            .ok_or_else(|| {
                AtmError::daemon_unavailable_with_recovery(
                    "hook snapshot is unavailable because hook-filter startup wiring was not installed",
                    "Install the hook filter during daemon startup before serving hook registry routes, then retry.",
                )
                .with_cause("StorageAndNudgeRouter::with_hook_filter was not called")
            })
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
        deadline: RequestDeadline,
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
        let schedule = tokio::time::timeout(
            cron_scheduler_budget(deadline, "cron schedule start")?,
            scheduler.start(CronStartSpec {
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
            }),
        )
        .await
        .map_err(|_| {
            AtmError::daemon_unavailable("request deadline expired during cron schedule start")
        })??;
        Ok(ApiResponse::new(ResponseEnvelope::CronStarted(schedule)))
    }

    pub(crate) async fn cron_end(
        &self,
        request: CronEndRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        require_local(ingress)?;
        self.check_roster_member(&request.caller_team, &request.caller_identity)?;
        let scheduler = self.cron_scheduler()?;
        let rows = tokio::time::timeout(
            cron_scheduler_budget(deadline, "cron schedule end")?,
            scheduler.end(
                &request.caller_team,
                &request.phase,
                request.name.as_deref(),
            ),
        )
        .await
        .map_err(|_| {
            AtmError::daemon_unavailable("request deadline expired during cron schedule end")
        })??;
        Ok(ApiResponse::new(ResponseEnvelope::CronEnded(rows)))
    }

    pub(crate) async fn cron_list(
        &self,
        request: CronListQuery,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        require_local(ingress)?;
        self.check_roster_member(&request.caller_team, &request.caller_identity)?;
        let scheduler = self.cron_scheduler()?;
        let rows = tokio::time::timeout(
            cron_scheduler_budget(deadline, "cron schedule list")?,
            scheduler.list(Some(&request.caller_team), request.phase.as_ref()),
        )
        .await
        .map_err(|_| {
            AtmError::daemon_unavailable("request deadline expired during cron schedule list")
        })??;
        Ok(ApiResponse::new(ResponseEnvelope::CronList(rows)))
    }

    fn cron_scheduler(&self) -> Result<Arc<dyn CronScheduler>, AtmError> {
        self.hooks
            .cron
            .get()
            .cloned()
            .ok_or_else(|| {
                AtmError::daemon_unavailable_with_recovery(
                    "cron scheduler is unavailable because daemon startup did not install it",
                    "Install the cron scheduler once during daemon startup before serving cron routes, then retry.",
                )
                .with_cause("StorageAndNudgeRouter::install_cron_scheduler was not called")
            })
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use std::task::{Context, Poll, Wake, Waker};
    use std::time::Duration;

    use atm_core::api::{AuthenticatedIngress, RequestDeadline};
    use atm_core::boundary::{CronScheduler, HookRegistrationStore, RecordingCronScheduler};
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::hook::{
        CronEndRequest, CronListQuery, CronStartRequest, HookDisableRequest, HookListQuery,
        HookRegisterRequest,
    };
    use atm_core::protocol::ResponseEnvelope;
    use atm_runtime_test_support::InMemoryHookRegistrationStore;
    use serde_json::{Value, json};
    use tokio::sync::Notify;

    use crate::hook_filter::ActiveHookSnapshot;
    use crate::hook_filter::test_support::{
        agent, deadline, handled_action, hook, phase, reply, team,
    };
    use crate::storage_and_nudge_router::StorageAndNudgeRouter;
    use crate::storage_and_nudge_router::tests::{Fixture, fixture};
    use crate::task_commands::NormalTaskCommands;

    struct NoopWake;

    impl Wake for NoopWake {
        fn wake(self: Arc<Self>) {}
    }

    fn assert_pending_once<F: Future>(future: Pin<&mut F>) {
        let waker = Waker::from(Arc::new(NoopWake));
        let mut context = Context::from_waker(&waker);
        assert!(
            matches!(future.poll(&mut context), Poll::Pending),
            "operation should be queued behind the in-flight bridge job"
        );
    }
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
        let router = crate::hook_filter::test_support::install_filter(
            router,
            Arc::new(snapshot),
            Arc::new(atm_core::boundary::DummyHookRunner::default()),
        );
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

    fn assert_refusal(error: &AtmError, expected_code: AtmErrorCode, expected_message: &str) {
        assert_eq!(error.code(), expected_code);
        let expected_message = match expected_code {
            AtmErrorCode::HookModuleInvalid => format!(
                "HOOK.MODULE_INVALID: {expected_message}\n  Recovery: Register a task hook module path and name the hook from the supported list, then retry."
            ),
            AtmErrorCode::AgentNotFound => format!(
                "{expected_message}\n  Recovery: Correct the ATM address or team selection and retry."
            ),
            _ => expected_message.to_owned(),
        };
        assert_eq!(error.message(), expected_message);
    }

    fn assert_timeout_validation(error: &AtmError, name: &str, timeout_ms: u32, max: u32) {
        assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
        assert_eq!(
            error.message(),
            format!(
                "{name}={timeout_ms} is outside the accepted 1..={max} milliseconds\n  Recovery: Set {name} to a value from 1 through {max} milliseconds and retry."
            )
        );
        let expected_cause = format!("{name} timeout bounds validation failed");
        assert_eq!(error.cause(), Some(expected_cause.as_str()));
    }

    #[test]
    fn non_local_routes_report_the_ingress_cause_and_recovery() {
        let error = super::require_local(AuthenticatedIngress::Peer).expect_err("peer denied");
        assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
        assert!(error.message().contains("reject Peer ingress"));
        assert!(
            error.message().contains(
                "Recovery: Send the request through the authenticated local HTTP adapter."
            )
        );
        assert_eq!(error.cause(), Some("request arrived through Peer ingress"));
    }

    #[test]
    fn missing_hook_snapshot_reports_startup_cause_and_recovery() {
        let fixture = fixture(true, None, None);
        let error = match fixture.router.snapshot() {
            Ok(_) => panic!("snapshot unexpectedly installed"),
            Err(error) => error,
        };
        assert_eq!(error.code(), AtmErrorCode::DaemonUnavailable);
        assert_eq!(
            error.message(),
            "hook snapshot is unavailable because hook-filter startup wiring was not installed\n  Recovery: Install the hook filter during daemon startup before serving hook registry routes, then retry."
        );
        assert_eq!(
            error.cause(),
            Some("StorageAndNudgeRouter::with_hook_filter was not called")
        );
    }

    #[tokio::test]
    async fn dirty_reload_and_registration_write_serialize_on_the_bridge() {
        let routes = routes();
        let snapshot = routes.router.hooks.snapshot.clone().expect("snapshot");
        // One failed refresh leaves the snapshot dirty; the store answers again after it.
        routes.hooks.fail_active_all.store(true, Ordering::SeqCst);
        assert!(snapshot.refresh().is_err());
        routes.hooks.fail_active_all.store(false, Ordering::SeqCst);
        let pause = Arc::new(routes.hooks.pause_next_active_all());

        let reload = {
            let snapshot = snapshot.clone();
            let bridge = routes.router.control_path_sync_bridge.clone();
            tokio::spawn(async move {
                snapshot
                    .current(&bridge, deadline(), &team(), &hook("task.assign"))
                    .await
            })
        };
        {
            let pause = pause.clone();
            tokio::task::spawn_blocking(move || pause.wait_entered())
                .await
                .expect("reload reached the store");
        }
        let write = {
            let router = routes.router.clone();
            tokio::spawn(async move {
                router
                    .hook_register(
                        register_request("sender", "task.assign", "bk"),
                        AuthenticatedIngress::Local,
                        deadline(),
                    )
                    .await
                    .map(|_| ())
            })
        };
        pause.release();

        reload.await.expect("reload task").expect("reload");
        write.await.expect("write task").expect("write");
        let rows = snapshot
            .current(
                &routes.router.control_path_sync_bridge,
                deadline(),
                &team(),
                &hook("task.assign"),
            )
            .await
            .expect("current");
        assert_eq!(
            rows.len(),
            1,
            "a reload that read before the write must not install its stale map after the write"
        );
    }

    #[tokio::test]
    async fn doctor_store_read_and_registration_write_serialize_on_the_bridge() {
        let routes = routes();
        let pause = Arc::new(routes.hooks.pause_next_active_all());

        let doctor = {
            let router = routes.router.clone();
            tokio::spawn(async move {
                crate::doctor_hooks::collect(&router, None, deadline())
                    .await
                    .entries
            })
        };
        {
            let pause = pause.clone();
            tokio::task::spawn_blocking(move || pause.wait_entered())
                .await
                .expect("doctor reached the store");
        }
        let mut write = Box::pin(routes.router.hook_register(
            register_request("sender", "task.assign", "bk"),
            AuthenticatedIngress::Local,
            deadline(),
        ));
        assert_pending_once(write.as_mut());
        pause.release();

        let entries = doctor.await.expect("doctor task");
        write.await.expect("write");
        assert!(
            entries.is_empty(),
            "the doctor read preceded the write, so it saw no rows"
        );
    }

    fn list_request() -> HookListQuery {
        HookListQuery {
            caller_identity: agent("sender"),
            caller_team: team(),
            phase: None,
        }
    }

    #[tokio::test]
    async fn registry_route_without_installed_filter_fails_closed() {
        let fixture = fixture(true, None, None);
        let hooks = Arc::new(InMemoryHookRegistrationStore::default());
        let mut router = fixture.router.clone();
        router.service_runtime = router
            .service_runtime
            .clone()
            .with_hook_registration_store(hooks);

        let error = router
            .hook_register(
                register_request("sender", "task.assign", "bk"),
                AuthenticatedIngress::Local,
                deadline(),
            )
            .await
            .expect_err("a router without a hook filter has no snapshot to refresh");

        assert_eq!(error.code(), AtmErrorCode::DaemonUnavailable);
        assert_eq!(
            error.message(),
            "hook snapshot is unavailable because hook-filter startup wiring was not installed\n  Recovery: Install the hook filter during daemon startup before serving hook registry routes, then retry.",
        );
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
    async fn disable_refuses_cron_and_unknown_hook_names_and_keeps_rows() {
        let routes = routes();
        register(&routes, register_request("sender", "task.assign", "bk"))
            .await
            .expect("register");
        for (name, expected_message) in [
            (
                "cron.tick",
                "cron.tick is not a task hook; expected one of task.assign, task.start, task.close, task.move, task.list, task.events",
            ),
            (
                "task.bogus",
                "unknown hook name task.bogus; expected one of task.assign, task.start, task.close, task.move, task.list, task.events, cron.tick, cron.stop",
            ),
        ] {
            let result = routes
                .router
                .hook_disable(
                    HookDisableRequest {
                        caller_identity: agent("sender"),
                        caller_team: team(),
                        phase: phase("bk"),
                        hook: Some(name.to_owned()),
                    },
                    AuthenticatedIngress::Local,
                    deadline(),
                )
                .await;
            let Err(err) = result else {
                panic!("disable must refuse the non-task hook name {name}");
            };
            assert_refusal(&err, AtmErrorCode::HookModuleInvalid, expected_message);
        }
        let active = routes.hooks.active_all().expect("rows");
        assert_eq!(active.len(), 1, "a refused disable changes no rows");
    }

    #[tokio::test]
    async fn register_refuses_cron_tick_unknown_name_and_over_cap_timeout() {
        let routes = routes();
        let tick = register(&routes, register_request("sender", "cron.tick", "bk"))
            .await
            .expect_err("cron.tick is not a task hook");
        assert_refusal(
            &tick,
            AtmErrorCode::HookModuleInvalid,
            "cron.tick is not a task hook; expected one of task.assign, task.start, task.close, task.move, task.list, task.events",
        );
        let unknown = register(&routes, register_request("sender", "task.bogus", "bk"))
            .await
            .expect_err("unknown task hook");
        assert_refusal(
            &unknown,
            AtmErrorCode::HookModuleInvalid,
            "unknown hook name task.bogus; expected one of task.assign, task.start, task.close, task.move, task.list, task.events, cron.tick, cron.stop",
        );
        let mut over = register_request("sender", "task.assign", "bk");
        over.timeout_ms = atm_core::boundary::HOOK_TIMEOUT_MAX_MS + 1;
        let capped = register(&routes, over).await.expect_err("over cap timeout");
        assert_timeout_validation(
            &capped,
            "timeout_ms",
            atm_core::boundary::HOOK_TIMEOUT_MAX_MS + 1,
            atm_core::boundary::HOOK_TIMEOUT_MAX_MS,
        );
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
        assert_refusal(
            &off,
            AtmErrorCode::AgentNotFound,
            "agent 'stranger' was not found in team 'test-team'; add it with `atm teams add-member test-team stranger`",
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
            .current(
                &routes.router.control_path_sync_bridge,
                deadline(),
                &team(),
                &hook("task.assign"),
            )
            .await
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
    async fn sequential_register_and_disable_leave_snapshot_equal_to_store() {
        // Sequential: each write is awaited to completion before the next. The overlap
        // case is covered by register_and_disable_overlap_on_the_bridge_and_snapshot_equals_store.
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
                .current(
                    &routes.router.control_path_sync_bridge,
                    deadline(),
                    &team(),
                    &hook("task.assign"),
                )
                .await
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
    async fn register_and_disable_overlap_on_the_bridge_and_snapshot_equals_store() {
        let routes = routes();
        let snapshot = routes.router.hooks.snapshot.clone().expect("snapshot");
        register(&routes, register_request("sender", "task.assign", "bj"))
            .await
            .expect("bj");
        // Hold the next store read: the register's refresh is then in flight inside its bridge job.
        let pause = Arc::new(routes.hooks.pause_next_active_all());
        let register_task = {
            let router = routes.router.clone();
            tokio::spawn(async move {
                router
                    .hook_register(
                        register_request("sender", "task.assign", "bk"),
                        AuthenticatedIngress::Local,
                        deadline(),
                    )
                    .await
                    .map(|_| ())
            })
        };
        {
            let pause = pause.clone();
            tokio::task::spawn_blocking(move || pause.wait_entered())
                .await
                .expect("register reached the store");
        }
        let mut disable = Box::pin(routes.router.hook_disable(
            HookDisableRequest {
                caller_identity: agent("sender"),
                caller_team: team(),
                phase: phase("bj"),
                hook: None,
            },
            AuthenticatedIngress::Local,
            deadline(),
        ));
        assert_pending_once(disable.as_mut());
        pause.release();
        register_task
            .await
            .expect("register task")
            .expect("register");
        disable.await.expect("disable");

        let active: Vec<String> = snapshot
            .current(
                &routes.router.control_path_sync_bridge,
                deadline(),
                &team(),
                &hook("task.assign"),
            )
            .await
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
        assert_eq!(stored, vec!["bk".to_owned()]);
    }

    /// The longest a gate wait may block. It only fails a test that never reaches the
    /// gate; nothing retries and nothing sleeps.
    const GATE_WAIT_BOUND: std::time::Duration = std::time::Duration::from_secs(10);

    /// Completes `wait`, or fails the test once the 10 s failure bound is exceeded.
    /// `timeout_message` names the wait and the bound, and is the `expect` text.
    async fn bounded<F: std::future::Future>(timeout_message: &'static str, wait: F) -> F::Output {
        tokio::time::timeout(GATE_WAIT_BOUND, wait)
            .await
            .expect(timeout_message)
    }

    /// The in-flight gate: the hook raises `entered`, then waits for `release`.
    #[derive(Clone, Default)]
    struct InFlightGate {
        entered: Arc<Notify>,
        release: Arc<Notify>,
    }

    impl InFlightGate {
        /// Installs the gate as the runner's `before_reply` hook.
        fn install(&self, runner: &atm_core::boundary::DummyHookRunner) {
            let gate = self.clone();
            *runner.before_reply.lock().expect("before reply") = Some(Arc::new(move |_index| {
                let gate = gate.clone();
                Box::pin(async move {
                    gate.entered.notify_one();
                    bounded(
                        "timed out after 10 s waiting for the test to release the in-flight hook",
                        gate.release.notified(),
                    )
                    .await;
                })
            }));
        }

        async fn wait_entered(&self) {
            bounded(
                "timed out after 10 s waiting for the in-flight hook to reach the runner",
                self.entered.notified(),
            )
            .await;
        }

        fn release_hook(&self) {
            self.release.notify_one();
        }
    }

    #[tokio::test]
    async fn disable_during_in_flight_hook_does_not_cancel_its_actions() {
        let routes = routes();
        register(&routes, register_request("sender", "task.assign", "bk"))
            .await
            .expect("register");
        let gate = InFlightGate::default();
        let runner = Arc::new(atm_core::boundary::DummyHookRunner::default());
        gate.install(&runner);
        let mut request = crate::hook_filter::test_support::assign_request(false);
        request.to = Some("recipient@test-team".parse().expect("address"));
        reply(
            &runner,
            atm_core::hook::HookReply::Handled(vec![handled_action(
                atm_core::hook::HookActionCommand::TaskAssign,
                &request,
            )]),
        );
        let commands = Arc::new(crate::hook_filter::HookedTaskCommands::new(
            Arc::new(NormalTaskCommands::new(routes.router.clone())),
            Arc::new(routes.router.unhooked()),
            routes.router.hooks.snapshot.clone().expect("snapshot"),
            runner.clone(),
            routes.router.hook_chains(),
            routes.router.control_path_sync_bridge.clone(),
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
        gate.wait_entered().await;
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
        gate.release_hook();
        let response = bounded(
            "timed out after 10 s waiting for the in-flight chain to complete",
            in_flight,
        )
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
    async fn register_refresh_failure_then_hooked_call_sees_the_row() {
        let routes = routes();
        let snapshot = routes.router.hooks.snapshot.clone().expect("snapshot");
        routes.hooks.fail_active_all.store(true, Ordering::SeqCst);
        let error = register(&routes, register_request("sender", "task.assign", "bk"))
            .await
            .expect_err("refresh failure is reported");
        assert!(error.message().contains(
            "registration committed; snapshot refresh failed; the next hooked call reloads it"
        ));
        routes.hooks.fail_active_all.store(false, Ordering::SeqCst);
        let rows = snapshot
            .current(
                &routes.router.control_path_sync_bridge,
                deadline(),
                &team(),
                &hook("task.assign"),
            )
            .await
            .expect("the hooked call reloads the dirty snapshot");
        let phases: Vec<&str> = rows.iter().map(|row| row.phase.as_str()).collect();
        assert_eq!(phases, vec!["bk"]);
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
                    deadline(),
                )
                .await
                .expect_err("timeout outside 1..=30000");
            assert_timeout_validation(
                &error,
                "timeout_ms",
                timeout,
                atm_core::boundary::CRON_TIMEOUT_MAX_MS,
            );
        }
        routes
            .router
            .cron_start(
                cron_request("sender", atm_core::boundary::CRON_TIMEOUT_MAX_MS, json!({})),
                AuthenticatedIngress::Local,
                deadline(),
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
        let custom_message = format!(
            "custom must be an object of at most {} serialized bytes",
            atm_core::hook::HOOK_CUSTOM_MAX_BYTES
        );
        assert_refusal(
            &non_object,
            AtmErrorCode::HookModuleInvalid,
            &custom_message,
        );
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
        assert_refusal(&over, AtmErrorCode::HookModuleInvalid, &custom_message);
        let cron = routes
            .router
            .cron_start(
                cron_request("sender", 1_000, oversize),
                AuthenticatedIngress::Local,
                deadline(),
            )
            .await
            .expect_err("cron over 64 KiB custom");
        assert_refusal(&cron, AtmErrorCode::HookModuleInvalid, &custom_message);
        assert!(routes.hooks.active_all().expect("rows").is_empty());
        assert!(routes.cron.calls.lock().expect("calls").is_empty());
    }

    #[tokio::test]
    async fn oversize_name_args_and_paths_refused() {
        // Returns the refusal, failing the test if the call was accepted.
        fn expect_refused<T>(result: Result<T, AtmError>, what: &str) -> AtmError {
            match result {
                Ok(_) => panic!("{what} must be refused"),
                Err(error) => error,
            }
        }
        let routes = routes();
        let path_over_cap = std::path::PathBuf::from(format!(
            "/{}",
            "p".repeat(atm_core::hook::HOOK_PATH_MAX_BYTES)
        ));
        let mut long_name = cron_request("sender", 1_000, json!({}));
        long_name.name = "n".repeat(atm_core::hook::CRON_NAME_MAX_BYTES + 1);
        let error = expect_refused(
            routes
                .router
                .cron_start(long_name, AuthenticatedIngress::Local, deadline())
                .await,
            "a cron name over the cap",
        );
        assert_refusal(
            &error,
            AtmErrorCode::HookModuleInvalid,
            &format!(
                "name must contain 1..={} bytes",
                atm_core::hook::CRON_NAME_MAX_BYTES
            ),
        );
        let mut many_args = cron_request("sender", 1_000, json!({}));
        many_args.args = vec!["a".to_owned(); atm_core::hook::CRON_ARGS_MAX + 1];
        let error = expect_refused(
            routes
                .router
                .cron_start(many_args, AuthenticatedIngress::Local, deadline())
                .await,
            "cron args over the item cap",
        );
        assert_refusal(
            &error,
            AtmErrorCode::HookModuleInvalid,
            &format!(
                "args must contain at most {} items",
                atm_core::hook::CRON_ARGS_MAX
            ),
        );
        let mut long_register = register_request("sender", "task.assign", "bk");
        long_register.module_path = path_over_cap.clone();
        let error = expect_refused(
            register(&routes, long_register).await,
            "a register module_path over the cap",
        );
        assert_refusal(
            &error,
            AtmErrorCode::HookModuleInvalid,
            &format!(
                "module_path must be at most {} bytes",
                atm_core::hook::HOOK_PATH_MAX_BYTES
            ),
        );
        let mut long_cron = cron_request("sender", 1_000, json!({}));
        long_cron.module_path = path_over_cap;
        let error = expect_refused(
            routes
                .router
                .cron_start(long_cron, AuthenticatedIngress::Local, deadline())
                .await,
            "a cron module_path over the cap",
        );
        assert_refusal(
            &error,
            AtmErrorCode::HookModuleInvalid,
            &format!(
                "module_path must be at most {} bytes",
                atm_core::hook::HOOK_PATH_MAX_BYTES
            ),
        );
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
                deadline(),
            )
            .await
            .expect("cron start");
        match response.into_inner() {
            ResponseEnvelope::CronStarted(schedule) => {
                assert_eq!(schedule.created_by, agent("sender"));
                assert_eq!(schedule.custom, custom.as_object().expect("object").clone());
                assert!(
                    matches!(schedule.stop, atm_core::boundary::StopCondition::Hook),
                    "the stop condition reaches the response unchanged"
                );
            }
            other => panic!("expected CronStarted, got {other:?}"),
        }
        let stored_stop = routes.cron.rows.lock().expect("rows")[0].stop.clone();
        assert!(
            matches!(stored_stop, atm_core::boundary::StopCondition::Hook),
            "the scheduler stores the stop condition unchanged"
        );
        let duplicate = routes
            .router
            .cron_start(
                cron_request("sender", 5_000, custom.clone()),
                AuthenticatedIngress::Local,
                deadline(),
            )
            .await
            .expect_err("an active schedule refuses a second start");
        assert!(
            duplicate.is_validation(),
            "the scheduler error keeps its class"
        );
        assert!(
            duplicate
                .message()
                .contains("cron schedule is already active"),
            "the scheduler error passes through: {duplicate}"
        );
        routes
            .router
            .cron_list(
                CronListQuery {
                    caller_identity: agent("sender"),
                    caller_team: team(),
                    phase: None,
                },
                AuthenticatedIngress::Local,
                deadline(),
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
                deadline(),
            )
            .await
            .expect("cron end");
        assert_eq!(
            *routes.cron.calls.lock().expect("calls"),
            vec!["start", "start", "list", "end"]
        );
    }

    #[tokio::test]
    async fn cron_routes_refuse_an_expired_request_deadline_before_scheduler() {
        let routes = routes();
        let expired = RequestDeadline::after(Duration::ZERO);

        for error in [
            routes
                .router
                .cron_start(
                    cron_request("sender", 1_000, json!({})),
                    AuthenticatedIngress::Local,
                    expired,
                )
                .await
                .expect_err("expired deadline refuses cron start"),
            routes
                .router
                .cron_list(
                    CronListQuery {
                        caller_identity: agent("sender"),
                        caller_team: team(),
                        phase: None,
                    },
                    AuthenticatedIngress::Local,
                    expired,
                )
                .await
                .expect_err("expired deadline refuses cron list"),
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
                    expired,
                )
                .await
                .expect_err("expired deadline refuses cron end"),
        ] {
            assert_eq!(error.code(), AtmErrorCode::DaemonUnavailable);
            assert!(error.message().contains("request deadline expired"));
        }
        assert!(routes.cron.calls.lock().expect("calls").is_empty());
    }

    #[tokio::test]
    async fn cron_routes_without_scheduler_are_unavailable() {
        let fixture = fixture(true, None, None);
        let error = fixture
            .router
            .cron_start(
                cron_request("sender", 1_000, json!({})),
                AuthenticatedIngress::Local,
                deadline(),
            )
            .await
            .expect_err("no scheduler installed");
        assert_eq!(error.code(), AtmErrorCode::DaemonUnavailable);
        assert!(
            error
                .message()
                .contains("daemon startup did not install it")
        );
        assert!(error.message().contains(
            "Recovery: Install the cron scheduler once during daemon startup before serving cron routes, then retry."
        ));
        assert_eq!(
            error.cause(),
            Some("StorageAndNudgeRouter::install_cron_scheduler was not called")
        );
    }
}
