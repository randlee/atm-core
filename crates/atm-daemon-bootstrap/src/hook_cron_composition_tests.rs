use std::{
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, atomic::Ordering},
    time::Duration,
};

use atm_core::{
    LocalServiceRuntime,
    api::{ApiRequest, ApiResponse, AuthenticatedIngress, RequestDeadline},
    boundary::{
        CronScheduler, CronStartSpec, DummyHookRunner, HookOutcome, HookRegistration, HookRunner,
        PhaseId, StopCondition,
    },
    error::{AtmError, AtmErrorCode},
    hook::{CronListQuery, CronStartRequest, HookListQuery, HookRegisterRequest, HookReply},
    observability::NullObservability,
    protocol::ResponseEnvelope,
    test_support::{TEST_RECIPIENT, TEST_SENDER, TEST_TEAM},
};
use atm_cron::{AtmCron, AtmCronDeps, AtmCronTestAccess, Clock, ManualClock, SeqJitter};
use atm_http_runtime::{
    CanonicalWriteHandler, PeerPoolConfig, RuntimeHealth, StorageAndNudgeRouter, UnhookedRouter,
};
use atm_runtime::RuntimeAssembly;
use atm_runtime_test_support::{
    InMemoryCronScheduleStore, InMemoryHookRegistrationStore, open_isolated_sqlite_boundary,
};
use serde_json::{Value, json};

use crate::{
    AtmTempSweeperRuntime, DaemonLaunchIdentity, DaemonWorkers,
    cron_composition::{compose_cron, compose_runtime_cron, run_shutdown_steps, start_cron_worker},
    replacement_handler::{
        ReplacementHandlerConfig, SelectedPeerAdapterSelection, build_replacement_handler,
    },
};

const TEST_OTHER_PHASE: &str = "test-phase";

fn deadline() -> RequestDeadline {
    RequestDeadline::after(Duration::from_secs(60))
}
fn stop_deadline() -> tokio::time::Instant {
    tokio::time::Instant::now() + Duration::from_secs(60)
}
fn now() -> chrono::DateTime<chrono::Utc> {
    "2026-10-19T10:00:00Z".parse().unwrap()
}

struct World {
    root: tempfile::TempDir,
    runtime: LocalServiceRuntime,
    handler: Arc<StorageAndNudgeRouter>,
    runner: Arc<dyn HookRunner>,
    dispatcher: Arc<UnhookedRouter>,
    hook_filter: Arc<atm_http_runtime::HookedTaskCommands>,
    workers: DaemonWorkers,
}

fn sqlite_composition_startup_guard() -> &'static Mutex<()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD.get_or_init(|| Mutex::new(()))
}

impl World {
    fn new() -> Self {
        // Constructing this real SQLite fixture loads the durable hook snapshot
        // synchronously. Serializing only that startup avoids test-process pool
        // oversubscription while preserving the production storage path.
        let _startup = sqlite_composition_startup_guard()
            .lock()
            .expect("SQLite composition startup guard is not poisoned");
        let root = tempfile::tempdir().unwrap();
        let assembly = open_isolated_sqlite_boundary(root.path())
            .unwrap()
            .for_daemon();
        Self::assemble(root, assembly).unwrap()
    }

    fn assemble(root: tempfile::TempDir, assembly: RuntimeAssembly) -> Result<Self, AtmError> {
        let runtime = assembly.service_runtime.clone();
        let task_telemetry = assembly.task_telemetry.clone();
        let crate::replacement_handler::ReplacementHandler {
            handler,
            recovery_sweep,
            runner,
            dispatcher,
            hook_filter,
            ..
        } = build_replacement_handler(
            assembly,
            ReplacementHandlerConfig {
                observability: Arc::new(NullObservability),
                selector_factory: |_, _, _, _| {
                    Arc::new(atm_core::boundary::NoopMessageReceivedHookSelector)
                        as Arc<dyn atm_core::boundary::MessageReceivedHookSelector>
                },
                daemon_launch_identity: DaemonLaunchIdentity::default(),
                peer_wire_mode: atm_core::peer_wire::PeerWireMode::plaintext_test(),
                peer_adapter_selection: SelectedPeerAdapterSelection {
                    adapter: None,
                    pool_config: PeerPoolConfig::default(),
                },
                runtime_health: RuntimeHealth::default(),
                diagnostic_counters: None,
                bare_cli: Default::default(),
                herdr_config: Default::default(),
                herdr_process: Some(Arc::new(
                    atm_herdr::testing::FakeHerdrProcessAdapter::default(),
                )),
                daemon_home: root.path().into(),
            },
        )?;
        std::fs::create_dir_all(root.path().join("temp")).unwrap();
        let atm_temp_sweeper = AtmTempSweeperRuntime::start(
            root.path().join("temp"),
            atm_core::SweepConfig {
                ttl: Duration::from_secs(3600),
                interval: Duration::from_secs(3600),
            },
            Arc::new(NullObservability),
            DaemonLaunchIdentity::default(),
        );
        Ok(Self {
            root,
            runtime,
            handler,
            runner,
            dispatcher,
            hook_filter,
            workers: DaemonWorkers::for_process(
                task_telemetry,
                recovery_sweep,
                atm_temp_sweeper,
                None,
            ),
        })
    }

    fn cron(&self, clock: Arc<ManualClock>) -> Arc<AtmCron> {
        compose_cron(&self.handler, self.deps(clock)).unwrap()
    }

    fn deps(&self, clock: Arc<ManualClock>) -> AtmCronDeps {
        AtmCronDeps {
            store: self.runtime.cron_schedule_store().unwrap(),
            runner: self.runner.clone(),
            dispatcher: self.dispatcher.clone(),
            clock,
            jitter: Arc::new(SeqJitter::default()),
            host: None,
        }
    }

    fn spec(&self) -> CronStartSpec {
        CronStartSpec {
            team: TEST_TEAM.parse().unwrap(),
            phase: PhaseId::parse(TEST_TEAM).unwrap(),
            name: "test".into(),
            cron_expr: "0 * * * * *".into(),
            module_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/cron_fixture/fixture_cron.py"),
            cwd: self.root.path().into(),
            args: vec!["status".into(), "--json".into()],
            timeout_ms: 30000,
            custom: serde_json::Map::new(),
            stop: StopCondition::Hook,
            created_by: TEST_SENDER.parse().unwrap(),
        }
    }

    fn seed_roster(&self) {
        let team = TEST_TEAM.parse::<atm_core::types::TeamName>().unwrap();
        self.runtime
            .shared_roster_store_arc()
            .save_roster(&atm_storage::RosterSnapshot {
                team_name: team.clone(),
                members: [TEST_SENDER, TEST_RECIPIENT]
                    .into_iter()
                    .map(|name| atm_core::boundary::RosterEntry {
                        team_name: team.clone(),
                        agent_name: name.parse().unwrap(),
                        member_kind: atm_storage::RosterMemberKind::Permanent,
                        harness: atm_storage::RosterHarness::PythonGraft,
                        agent_type: Default::default(),
                        model: Default::default(),
                        recipient_pane_id: None,
                        metadata_json: Default::default(),
                    })
                    .collect(),
                refreshed_at: None,
            })
            .unwrap();
    }

    fn write(&self) -> atm_core::send::WriteRequest {
        atm_core::send::WriteRequest::new(
            self.root.path().into(),
            self.root.path().into(),
            TEST_SENDER.parse().unwrap(),
            &format!("{TEST_RECIPIENT}@{TEST_TEAM}"),
            TEST_TEAM.parse().unwrap(),
            atm_core::send::SendMessageSource::Inline("work".into()),
            None,
            false,
            Some(format!("{TEST_TEAM}-5-test").parse().unwrap()),
            false,
        )
        .unwrap()
    }

    async fn request(&self, request: ApiRequest) -> Result<ApiResponse, AtmError> {
        CanonicalWriteHandler::dispatch(
            self.handler.as_ref(),
            request,
            AuthenticatedIngress::Local,
            deadline(),
        )
        .await
    }

    async fn shutdown(self) {
        run_shutdown_steps(
            async { Ok(()) },
            &self.handler,
            self.workers,
            stop_deadline(),
        )
        .await
        .unwrap();
    }
}

fn registration(root: &std::path::Path, phase: &str, hook: &str) -> HookRegistration {
    HookRegistration {
        team: TEST_TEAM.parse().unwrap(),
        phase: PhaseId::parse(phase).unwrap(),
        hook: atm_core::boundary::HookName::parse_task_hook(hook).unwrap(),
        module_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/hook_fixture/fixture_hook.py"),
        cwd: root.into(),
        timeout_ms: 30000,
        custom: serde_json::Map::new(),
        active: true,
        registered_by: TEST_SENDER.parse().unwrap(),
        registered_at: format!(
            "2026-10-19T00:00:{}Z",
            if phase == TEST_OTHER_PHASE {
                "00"
            } else {
                "01"
            }
        )
        .parse()
        .unwrap(),
        disabled_at: None,
    }
}

#[tokio::test]
async fn start_persists_row_with_args_and_timeout() {
    let world = World::new();
    let clock = Arc::new(ManualClock::new(now()));
    let cron = world.cron(clock);
    let spec = world.spec();
    let row = cron.start(spec.clone()).await.unwrap();
    assert_eq!(row.team, spec.team);
    assert_eq!(row.phase, spec.phase);
    assert_eq!(row.name, spec.name);
    assert_eq!(row.cron_expr, spec.cron_expr);
    assert_eq!(row.custom, spec.custom);
    assert_eq!(row.created_by, spec.created_by);
    assert_eq!(row.created_at, now().into());
    assert!(row.active);
    assert_eq!(row.args, spec.args);
    assert_eq!(row.timeout_ms, spec.timeout_ms);
    assert_eq!(row.module_path, spec.module_path);
    assert_eq!(row.cwd, spec.cwd);
    assert_eq!(row.stop, spec.stop);
    assert_eq!(row.jitter_s, 0);
    assert_eq!(
        world
            .runtime
            .cron_schedule_store()
            .unwrap()
            .list(None, None)
            .unwrap(),
        vec![row]
    );
    world.shutdown().await;
}

#[tokio::test]
async fn list_shows_last_run_and_outcome_after_record_run() {
    let world = World::new();
    let cron = world.cron(Arc::new(ManualClock::new(now())));
    let row = cron.start(world.spec()).await.unwrap();
    world
        .runtime
        .cron_schedule_store()
        .unwrap()
        .record_run(row.id, row.generation, now().into(), "ok")
        .unwrap();
    let rows = cron.list(None, None).await.unwrap();
    assert_eq!(rows[0].last_run_at, Some(now().into()));
    assert_eq!(rows[0].last_outcome.as_deref(), Some("ok"));
    world.shutdown().await;
}

#[tokio::test]
async fn start_refuses_until_in_the_past_with_daemon_message() {
    let world = World::new();
    let cron = world.cron(Arc::new(ManualClock::new(now())));
    let mut spec = world.spec();
    spec.stop = StopCondition::Until {
        at: now() - chrono::Duration::seconds(1),
    };
    let error = cron.start(spec).await.unwrap_err();
    assert!(error.message().contains("until is not in the future"));
    assert!(cron.list(None, None).await.unwrap().is_empty());
    world.shutdown().await;
}

#[tokio::test]
async fn start_refuses_invalid_expression_with_cron_expr_invalid() {
    let world = World::new();
    let cron = world.cron(Arc::new(ManualClock::new(now())));
    for expression in ["x y", "* * * * *"] {
        let mut spec = world.spec();
        spec.cron_expr = expression.into();
        assert_eq!(
            cron.start(spec).await.unwrap_err().code(),
            AtmErrorCode::CronExprInvalid
        );
    }
    assert!(cron.list(None, None).await.unwrap().is_empty());
    world.shutdown().await;
}

#[tokio::test]
async fn start_refuses_active_duplicate_name() {
    let world = World::new();
    let cron = world.cron(Arc::new(ManualClock::new(now())));
    cron.start(world.spec()).await.unwrap();
    assert!(cron.start(world.spec()).await.is_err());
    assert_eq!(cron.list(None, None).await.unwrap().len(), 1);
    world.shutdown().await;
}

#[tokio::test]
async fn end_phase_keeps_rows_inactive_with_ended_at() {
    let world = World::new();
    let clock = Arc::new(ManualClock::new(now()));
    let cron = world.cron(clock.clone());
    let spec = world.spec();
    cron.start(spec.clone()).await.unwrap();
    clock.advance(chrono::Duration::seconds(10));
    cron.end(&spec.team, &spec.phase, None).await.unwrap();
    let rows = cron.list(None, None).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].active);
    assert_eq!(rows[0].ended_at, Some(clock.now().into()));
    world.shutdown().await;
}

#[tokio::test]
async fn filter_and_cron_share_one_runner_and_one_dispatcher() {
    let world = World::new();
    let filter = &world.hook_filter;
    assert!(Arc::ptr_eq(&world.runner, filter.runner()));
    assert!(Arc::ptr_eq(&world.dispatcher, filter.actions()));
    assert!(Arc::ptr_eq(&world.handler.hook_chains(), filter.chains()));
    let cron = compose_runtime_cron(
        &world.handler,
        &world.runtime,
        world.runner.clone(),
        world.dispatcher.clone(),
        Arc::new(ManualClock::new(now())),
        Arc::new(SeqJitter::default()),
        None,
    )
    .unwrap();
    assert!(Arc::ptr_eq(cron.hook_runner(), filter.runner()));
    let expected: Arc<dyn atm_core::boundary::HookActionDispatcher> = filter.actions().clone();
    assert!(Arc::ptr_eq(cron.action_dispatcher(), &expected));
    world.shutdown().await;
}

#[tokio::test]
async fn snapshot_load_error_is_deferred_past_daemon_composition() {
    let root = tempfile::tempdir().unwrap();
    let mut assembly = open_isolated_sqlite_boundary(root.path())
        .unwrap()
        .for_daemon();
    let store = Arc::new(InMemoryHookRegistrationStore::default());
    store.fail_active_all.store(true, Ordering::SeqCst);
    assembly.service_runtime = assembly.service_runtime.with_hook_registration_store(store);
    assert!(World::assemble(root, assembly).is_ok());
}

#[tokio::test]
async fn cron_worker_is_first_shutdown_step() {
    let mut world = World::new();
    let cron = world.cron(Arc::new(ManualClock::new(now())));
    start_cron_worker(&cron, &mut world.workers).await.unwrap();
    let probe = Arc::new(crate::shutdown_probe::Probe::default());
    crate::shutdown_probe::observe(&probe, world.shutdown()).await;
    let steps = probe.steps();
    assert_eq!(
        steps
            .iter()
            .take(3)
            .map(|step| step.step)
            .collect::<Vec<_>>(),
        ["cron_worker", "hook_chains", "listener"]
    );
}

#[tokio::test(start_paused = true)]
async fn blocked_cron_hook_leaves_budget_for_hook_chains_and_later_shutdown_steps() {
    let mut world = World::new();
    let clock = Arc::new(ManualClock::new(now()));
    let runner = Arc::new(DummyHookRunner::default());
    runner.replies.lock().unwrap().push_back(Ok(HookOutcome {
        reply: HookReply::PassThrough,
        elapsed_ms: 1,
    }));
    let (entered, mut entered_rx) = tokio::sync::watch::channel(false);
    let (_release, release_rx) = tokio::sync::watch::channel(false);
    let (cancelled, mut cancelled_rx) = tokio::sync::watch::channel(false);
    *runner.before_reply.lock().unwrap() = Some(Arc::new(move |_| {
        let entered = entered.clone();
        let mut release_rx = release_rx.clone();
        let cancelled = cancelled.clone();
        Box::pin(async move {
            struct CancelOnDrop(tokio::sync::watch::Sender<bool>);
            impl Drop for CancelOnDrop {
                fn drop(&mut self) {
                    self.0.send_replace(true);
                }
            }
            let _cancel = CancelOnDrop(cancelled);
            entered.send_replace(true);
            let _ = release_rx.wait_for(|released| *released).await;
        })
    }));
    let mut deps = world.deps(clock.clone());
    deps.runner = runner;
    let cron = compose_cron(&world.handler, deps).unwrap();
    let mut spec = world.spec();
    spec.stop = StopCondition::Until {
        at: now() + chrono::Duration::hours(1),
    };
    cron.start(spec).await.unwrap();
    start_cron_worker(&cron, &mut world.workers).await.unwrap();
    clock.wait_parks_above(0).await;
    clock.advance_settled(chrono::Duration::minutes(1)).await;
    entered_rx.wait_for(|entered| *entered).await.unwrap();

    let World {
        handler, workers, ..
    } = world;
    let probe = Arc::new(crate::shutdown_probe::Probe::new());
    let shutdown_probe = probe.clone();
    let shared_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let shutdown = tokio::spawn(async move {
        crate::shutdown_probe::observe(&shutdown_probe, async move {
            run_shutdown_steps(
                async {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    Ok(())
                },
                &handler,
                workers,
                shared_deadline,
            )
            .await
        })
        .await
    });

    probe.wait_for_start("cron_worker").await;
    tokio::time::advance(Duration::from_secs(1)).await;
    probe.wait_for_start("hook_chains").await;
    probe.wait_for_start("listener").await;
    assert!(tokio::time::Instant::now() < shared_deadline);
    tokio::time::advance(Duration::from_secs(3)).await;
    shutdown.await.unwrap().unwrap();
    cancelled_rx.wait_for(|cancelled| *cancelled).await.unwrap();

    let steps = probe.steps();
    let cron_step = steps
        .iter()
        .find(|step| step.step == "cron_worker")
        .unwrap();
    let hook_chains = steps
        .iter()
        .find(|step| step.step == "hook_chains")
        .unwrap();
    let listener = steps.iter().find(|step| step.step == "listener").unwrap();
    assert!(cron_step.deadline < shared_deadline);
    assert!(cron_step.returned < shared_deadline);
    assert_eq!(hook_chains.deadline, shared_deadline);
    assert_eq!(listener.deadline, shared_deadline);
    assert!(hook_chains.started < shared_deadline);
    assert!(listener.started < shared_deadline);
    assert!(listener.returned < shared_deadline);
}

#[tokio::test]
async fn cron_initial_load_error_aborts_startup() {
    let mut world = World::new();
    let store = Arc::new(InMemoryCronScheduleStore::default());
    store.fail_active_all.store(true, Ordering::SeqCst);
    let mut deps = world.deps(Arc::new(ManualClock::new(now())));
    deps.store = store;
    let cron = compose_cron(&world.handler, deps).unwrap();
    assert!(start_cron_worker(&cron, &mut world.workers).await.is_err());
    assert!(world.workers.cron.is_none());
    world.shutdown().await;
}

#[tokio::test]
async fn register_second_phase_same_hook_succeeds() {
    let world = World::new();
    world.seed_roster();
    for phase in [TEST_OTHER_PHASE, TEST_TEAM] {
        world
            .request(ApiRequest::HookRegister(HookRegisterRequest {
                caller_identity: TEST_SENDER.parse().unwrap(),
                caller_team: TEST_TEAM.parse().unwrap(),
                phase: PhaseId::parse(phase).unwrap(),
                hook: "task.close".into(),
                module_path: registration(world.root.path(), phase, "task.close").module_path,
                cwd: world.root.path().into(),
                timeout_ms: 2500,
                custom: json!({}),
            }))
            .await
            .unwrap();
    }
    let ResponseEnvelope::HookList(rows) = world
        .request(ApiRequest::HookList(HookListQuery {
            caller_identity: TEST_SENDER.parse().unwrap(),
            caller_team: TEST_TEAM.parse().unwrap(),
            phase: None,
        }))
        .await
        .unwrap()
        .into_inner()
    else {
        panic!("hook list")
    };
    assert_eq!(
        rows.iter()
            .map(|row| row.phase.as_str())
            .collect::<Vec<_>>(),
        [TEST_OTHER_PHASE, TEST_TEAM]
    );
    assert!(rows.iter().all(|row| row.active));
    world.shutdown().await;
}

#[tokio::test]
async fn cron_scheduler_installed_once_and_cron_routes_reach_it() {
    let world = World::new();
    world.seed_roster();
    let clock = Arc::new(ManualClock::new(now()));
    let cron = world.cron(clock.clone());
    assert!(world.handler.install_cron_scheduler(cron).is_err());
    let spec = world.spec();
    world
        .request(ApiRequest::CronStart(CronStartRequest {
            caller_identity: spec.created_by,
            caller_team: spec.team,
            phase: spec.phase,
            name: spec.name,
            cron_expr: spec.cron_expr,
            module_path: spec.module_path,
            cwd: spec.cwd,
            args: spec.args,
            timeout_ms: spec.timeout_ms,
            stop: spec.stop,
            custom: json!({}),
        }))
        .await
        .unwrap();
    let ResponseEnvelope::CronList(rows) = world
        .request(ApiRequest::CronList(CronListQuery {
            caller_identity: TEST_SENDER.parse().unwrap(),
            caller_team: TEST_TEAM.parse().unwrap(),
            phase: None,
        }))
        .await
        .unwrap()
        .into_inner()
    else {
        panic!("cron list")
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "test");
    world.shutdown().await;
}

#[tokio::test]
async fn snapshot_load_from_reopened_store_serves_pre_restart_registrations() {
    let root = tempfile::tempdir().unwrap();
    {
        let assembly = open_isolated_sqlite_boundary(root.path())
            .unwrap()
            .for_daemon();
        let store = assembly.service_runtime.hook_registration_store().unwrap();
        for phase in [TEST_OTHER_PHASE, TEST_TEAM] {
            let cwd = root.path().join(phase);
            std::fs::create_dir(&cwd).unwrap();
            std::fs::write(
                cwd.join("fixture_reply.json"),
                json!({"result":"refused","message":phase}).to_string(),
            )
            .unwrap();
            store
                .register(registration(&cwd, phase, "task.close"))
                .unwrap();
        }
    }
    let assembly = open_isolated_sqlite_boundary(root.path())
        .unwrap()
        .for_daemon();
    let rows = assembly
        .service_runtime
        .hook_registration_store()
        .unwrap()
        .active_all()
        .unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row.phase.as_str())
            .collect::<Vec<_>>(),
        [TEST_OTHER_PHASE, TEST_TEAM]
    );
    let world = World::assemble(root, assembly).unwrap();
    world.seed_roster();
    let mut request = world.write();
    request.task_op = Some(atm_core::boundary::TaskOp::Close {
        outcome: atm_core::boundary::TaskCloseOutcome::Completed,
        reason: None,
    });
    let error = world
        .request(ApiRequest::Write(Box::new(request)))
        .await
        .err()
        .unwrap();
    assert_eq!(error.code(), AtmErrorCode::HookRefused);
    assert!(error.message().contains(TEST_OTHER_PHASE));
    assert!(
        world
            .root
            .path()
            .join(TEST_OTHER_PHASE)
            .join("fixture_request.json")
            .exists()
    );
    assert!(
        !world
            .root
            .path()
            .join(TEST_TEAM)
            .join("fixture_request.json")
            .exists()
    );
    world.shutdown().await;
}

#[tokio::test]
async fn close_end_to_end_with_refusing_hook_leaves_task_open() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("fixture_reply.json"),
        json!({"result":"refused","message":"not ready"}).to_string(),
    )
    .unwrap();
    let assembly = open_isolated_sqlite_boundary(root.path())
        .unwrap()
        .for_daemon();
    assembly
        .service_runtime
        .hook_registration_store()
        .unwrap()
        .register(registration(root.path(), TEST_TEAM, "task.close"))
        .unwrap();
    let world = World::assemble(root, assembly).unwrap();
    world.seed_roster();
    let mut request = world.write();
    world
        .request(ApiRequest::Write(Box::new(request.clone())))
        .await
        .unwrap();
    request.task_op = Some(atm_core::boundary::TaskOp::Start);
    request.caller_identity = TEST_RECIPIENT.parse().unwrap();
    world
        .request(ApiRequest::Write(Box::new(request.clone())))
        .await
        .unwrap();
    request.task_op = Some(atm_core::boundary::TaskOp::Close {
        outcome: atm_core::boundary::TaskCloseOutcome::Completed,
        reason: None,
    });
    let error = world
        .request(ApiRequest::Write(Box::new(request)))
        .await
        .err()
        .unwrap();
    assert_eq!(error.code(), AtmErrorCode::HookRefused);
    assert!(error.message().contains("not ready"));
    let row = world
        .runtime
        .task_store()
        .unwrap()
        .load_task(
            &TEST_TEAM.parse().unwrap(),
            &format!("{TEST_TEAM}-5-test").parse().unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(row.state, atm_storage::TaskState::Active);
    world.shutdown().await;
}

#[tokio::test]
async fn daemon_restart_with_schedule_due_while_down_fires_once_at_next_occurrence_and_not_at_startup()
 {
    let world = World::new();
    let cron = world.cron(Arc::new(ManualClock::new(
        now() - chrono::Duration::minutes(1),
    )));
    let mut spec = world.spec();
    spec.stop = StopCondition::Until {
        at: now() + chrono::Duration::hours(1),
    };
    cron.start(spec).await.unwrap();
    let World {
        root,
        runtime,
        handler,
        workers,
        runner: old_runner,
        dispatcher,
        ..
    } = world;
    run_shutdown_steps(async { Ok(()) }, &handler, workers, stop_deadline())
        .await
        .unwrap();
    drop((cron, runtime, handler, old_runner, dispatcher));
    let assembly = open_isolated_sqlite_boundary(root.path())
        .unwrap()
        .for_daemon();
    let world = World::assemble(root, assembly).unwrap();
    let clock = Arc::new(ManualClock::new(now() + chrono::Duration::seconds(30)));
    let runner = Arc::new(DummyHookRunner::default());
    runner
        .replies
        .lock()
        .unwrap()
        .push_back(Ok(atm_core::boundary::HookOutcome {
            reply: atm_core::hook::HookReply::PassThrough,
            elapsed_ms: 0,
        }));
    let mut deps = world.deps(clock.clone());
    deps.runner = runner.clone();
    let restarted = compose_cron(&world.handler, deps).unwrap();
    let worker = restarted.start_worker().await.unwrap();
    clock.wait_parks_above(0).await;
    assert!(runner.calls.lock().unwrap().is_empty());
    clock.advance_settled(chrono::Duration::seconds(30)).await;
    worker.shutdown(stop_deadline()).await;
    assert_eq!(runner.calls.lock().unwrap().len(), 1);
    world.shutdown().await;
}

#[tokio::test]
async fn fixture_cron_receives_args_through_the_composed_scheduler() {
    let mut world = World::new();
    let clock = Arc::new(ManualClock::new(now()));
    let cron = world.cron(clock.clone());
    cron.start(world.spec()).await.unwrap();
    start_cron_worker(&cron, &mut world.workers).await.unwrap();
    clock.wait_parks_above(0).await;
    clock.advance_settled(chrono::Duration::minutes(1)).await;
    world
        .workers
        .cron
        .take()
        .unwrap()
        .shutdown(stop_deadline())
        .await;
    let requests: Vec<Value> =
        std::fs::read_to_string(world.root.path().join("cron_requests.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    assert_eq!(requests.len(), 2);
    for (entry, hook) in requests.iter().zip(["cron.stop", "cron.tick"]) {
        assert_eq!(entry["argv"], json!([hook]));
        assert_eq!(entry["request"]["hook"], hook);
        assert_eq!(
            entry["request"]["request"]["args"],
            json!(["status", "--json"])
        );
    }
    assert_eq!(
        world
            .runtime
            .cron_schedule_store()
            .unwrap()
            .list(None, None)
            .unwrap()[0]
            .last_outcome
            .as_deref(),
        Some("ok")
    );
    std::fs::write(world.root.path().join("stop.marker"), "stop").unwrap();
    let World {
        root,
        runtime,
        handler,
        workers,
        runner,
        dispatcher,
        ..
    } = world;
    run_shutdown_steps(async { Ok(()) }, &handler, workers, stop_deadline())
        .await
        .unwrap();
    drop((cron, runtime, handler, runner, dispatcher));
    let assembly = open_isolated_sqlite_boundary(root.path())
        .unwrap()
        .for_daemon();
    let mut world = World::assemble(root, assembly).unwrap();
    let clock = Arc::new(ManualClock::new(now() + chrono::Duration::minutes(1)));
    let cron = world.cron(clock.clone());
    start_cron_worker(&cron, &mut world.workers).await.unwrap();
    clock.wait_parks_above(0).await;
    clock.advance_settled(chrono::Duration::minutes(1)).await;
    world
        .workers
        .cron
        .take()
        .unwrap()
        .shutdown(stop_deadline())
        .await;
    let rows = world
        .runtime
        .cron_schedule_store()
        .unwrap()
        .list(None, None)
        .unwrap();
    assert!(!rows[0].active);
    assert_eq!(
        rows[0].last_outcome.as_deref(),
        Some("stopped: marker present")
    );
    let requests: Vec<Value> =
        std::fs::read_to_string(world.root.path().join("cron_requests.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[2]["argv"], json!(["cron.stop"]));
    world.shutdown().await;
}

#[tokio::test]
async fn doctor_runs_the_registered_module_doctor_for_its_team() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("fixture_reply.json"),
        json!({"success":true,"message":"ok",
        "findings":[{"severity":"warning","code":"goal-pending","message":"not shipped"}]})
        .to_string(),
    )
    .unwrap();
    let assembly = open_isolated_sqlite_boundary(root.path())
        .unwrap()
        .for_daemon();
    let mut row = registration(root.path(), TEST_TEAM, "task.close");
    row.custom = serde_json::from_value(json!({"goal":"ship"})).unwrap();
    assembly
        .service_runtime
        .hook_registration_store()
        .unwrap()
        .register(row)
        .unwrap();
    let world = World::assemble(root, assembly).unwrap();
    world.seed_roster();
    let query = |team: &str| {
        ApiRequest::Doctor(atm_core::doctor::DoctorQuery {
            home_dir: world.root.path().into(),
            current_dir: world.root.path().into(),
            team_override: Some(team.parse().unwrap()),
            all_teams: false,
            caller_team: Some(team.parse().unwrap()),
            caller_identity: Some(TEST_SENDER.parse().unwrap()),
        })
    };
    let ResponseEnvelope::Doctor(report) =
        world.request(query(TEST_TEAM)).await.unwrap().into_inner()
    else {
        panic!("doctor")
    };
    assert_eq!(report.hooks.len(), 1);
    let entry = &report.hooks[0];
    assert_eq!(entry.team.as_str(), TEST_TEAM);
    assert_eq!(entry.phase.as_str(), TEST_TEAM);
    assert_eq!(entry.message.as_deref(), Some("ok"));
    assert_eq!(entry.findings.len(), 1);
    assert_eq!(entry.findings[0].code, "goal-pending");
    assert_eq!(
        entry.findings[0].severity,
        atm_core::doctor::DoctorSeverity::Warning
    );
    assert_eq!(entry.findings[0].message, "not shipped");
    let request_file = world.root.path().join("fixture_request.json");
    let request: Value = serde_json::from_slice(&std::fs::read(&request_file).unwrap()).unwrap();
    assert_eq!(request["team"], TEST_TEAM);
    assert_eq!(request["phase"], TEST_TEAM);
    assert_eq!(request["custom"], json!({"goal":"ship"}));
    std::fs::remove_file(&request_file).unwrap();
    let ResponseEnvelope::Doctor(report) = world
        .request(query(&format!("{TEST_TEAM}-foreign")))
        .await
        .unwrap()
        .into_inner()
    else {
        panic!("doctor")
    };
    assert!(report.hooks.is_empty());
    assert!(!request_file.exists());
    world.shutdown().await;
}

#[tokio::test]
async fn cron_tick_action_runs_unhooked() {
    let root = tempfile::tempdir().unwrap();
    let cwd_a = root.path().join("a");
    let cwd_b = root.path().join("b");
    std::fs::create_dir(&cwd_a).unwrap();
    std::fs::create_dir(&cwd_b).unwrap();
    std::fs::write(
        cwd_b.join("fixture_reply.json"),
        json!({"result":"refused","message":"must not run"}).to_string(),
    )
    .unwrap();
    std::fs::write(cwd_a.join("fixture_hooks.txt"), "cron.tick\n").unwrap();
    let assembly = open_isolated_sqlite_boundary(root.path())
        .unwrap()
        .for_daemon();
    assembly
        .service_runtime
        .hook_registration_store()
        .unwrap()
        .register(registration(&cwd_b, TEST_TEAM, "task.assign"))
        .unwrap();
    let mut world = World::assemble(root, assembly).unwrap();
    world.seed_roster();
    let action_request = world.write();
    std::fs::write(
        cwd_a.join("fixture_reply.json"),
        json!({"result":"handled","atm-action":[{
        "command":"task.assign","request":action_request}]})
        .to_string(),
    )
    .unwrap();
    let clock = Arc::new(ManualClock::new(now()));
    let cron = world.cron(clock.clone());
    let mut spec = world.spec();
    spec.cwd = cwd_a;
    spec.module_path = registration(&cwd_b, TEST_TEAM, "task.assign").module_path;
    spec.stop = StopCondition::Until {
        at: now() + chrono::Duration::hours(1),
    };
    cron.start(spec).await.unwrap();
    start_cron_worker(&cron, &mut world.workers).await.unwrap();
    clock.wait_parks_above(0).await;
    clock.advance_settled(chrono::Duration::minutes(1)).await;
    world
        .workers
        .cron
        .take()
        .unwrap()
        .shutdown(stop_deadline())
        .await;
    let rows = world
        .runtime
        .cron_schedule_store()
        .unwrap()
        .list(None, None)
        .unwrap();
    assert_eq!(rows[0].last_outcome.as_deref(), Some("actions: 1"));
    assert!(
        world
            .runtime
            .task_store()
            .unwrap()
            .load_task(
                &TEST_TEAM.parse().unwrap(),
                &format!("{TEST_TEAM}-5-test").parse().unwrap(),
            )
            .unwrap()
            .is_some()
    );
    assert!(!cwd_b.join("fixture_request.json").exists());
    world.shutdown().await;
}
