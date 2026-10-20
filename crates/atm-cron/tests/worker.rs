use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use atm_core::{
    boundary::{
        CronSchedule, CronScheduleStore, CronScheduler, CronStartSpec, HookCall, HookOutcome,
        PhaseId, StopCondition,
    },
    error::{AtmError, AtmErrorCode},
    hook::{HookReply, StopReply},
    test_support::TEST_SENDER,
};
use atm_cron::{AtmCron, AtmCronDeps, Clock, CronWorkerHandle, ManualClock, SeqJitter};
use atm_runtime_test_support::{
    DummyHookRunner, InMemoryCronScheduleStore, RecordingHookActionDispatcher,
};
use chrono::{DateTime, Utc};
use tokio::sync::watch;

const TEST_WAIT_TIMEOUT: Duration = Duration::from_secs(10);

thread_local! {
    static LOGS: std::cell::RefCell<Option<Arc<std::sync::Mutex<Vec<String>>>>> = const { std::cell::RefCell::new(None) };
}

struct LogsLayer;

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for LogsLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        struct Fields(String);
        impl tracing::field::Visit for Fields {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                self.0.push_str(&format!("{}={value:?} ", field.name()));
            }
        }
        let mut fields = Fields(String::new());
        event.record(&mut fields);
        LOGS.with(|logs| {
            if let Some(logs) = logs.borrow().as_ref() {
                logs.lock().unwrap().push(fields.0);
            }
        });
    }
}

fn initialize_logs() {
    static INITIALIZE: std::sync::Once = std::sync::Once::new();
    INITIALIZE.call_once(|| {
        use tracing_subscriber::prelude::*;
        tracing::subscriber::set_global_default(tracing_subscriber::registry().with(LogsLayer))
            .unwrap();
    });
}

fn capture_logs() -> Arc<std::sync::Mutex<Vec<String>>> {
    let logs = Arc::new(std::sync::Mutex::new(Vec::new()));
    LOGS.with(|slot| *slot.borrow_mut() = Some(logs.clone()));
    logs
}

struct Fixture {
    cron: Arc<AtmCron>,
    store: Arc<InMemoryCronScheduleStore>,
    runner: Arc<DummyHookRunner>,
    dispatcher: Arc<RecordingHookActionDispatcher>,
    clock: Arc<ManualClock>,
    jitter: Arc<SeqJitter>,
}

impl Fixture {
    fn new() -> Self {
        Self::with_dispatcher(RecordingHookActionDispatcher::default())
    }

    fn with_dispatcher(dispatcher: RecordingHookActionDispatcher) -> Self {
        initialize_logs();
        let store = Arc::new(InMemoryCronScheduleStore::new());
        let runner = Arc::new(DummyHookRunner::default());
        let dispatcher = Arc::new(dispatcher);
        let clock = Arc::new(ManualClock::new("2026-10-19T00:00:00Z".parse().unwrap()));
        let jitter = Arc::new(SeqJitter::default());
        let cron = Arc::new(AtmCron::new(AtmCronDeps {
            store: store.clone(),
            runner: runner.clone(),
            dispatcher: dispatcher.clone(),
            clock: clock.clone(),
            host: Some("test-host".parse().unwrap()),
            jitter: jitter.clone(),
        }));
        Self {
            cron,
            store,
            runner,
            dispatcher,
            clock,
            jitter,
        }
    }

    fn spec(&self, name: &str) -> CronStartSpec {
        CronStartSpec {
            team: "test-team".parse().unwrap(),
            phase: PhaseId::parse("bk").unwrap(),
            name: name.into(),
            cron_expr: "0 * * * * *".into(),
            module_path: "/module.py".into(),
            cwd: "/work".into(),
            args: vec!["status".into(), "--json".into()],
            timeout_ms: 20000,
            custom: serde_json::from_str(r#"{"goal":{"ready":true}}"#).unwrap(),
            stop: StopCondition::Until {
                at: self.clock.now() + chrono::Duration::days(1),
            },
            created_by: TEST_SENDER.parse().unwrap(),
        }
    }

    fn reply(&self, reply: HookReply) {
        self.runner
            .replies
            .lock()
            .unwrap()
            .push_back(Ok(HookOutcome {
                reply,
                elapsed_ms: 1,
            }));
    }

    async fn worker(&self) -> CronWorkerHandle {
        let parks = self.clock.parks();
        let handle = self.cron.start_worker().await.unwrap();
        self.clock.wait_parks_above(parks).await;
        handle
    }

    async fn step(&self, seconds: i64) {
        self.clock
            .advance_settled(chrono::Duration::seconds(seconds))
            .await;
    }

    fn row(&self) -> CronSchedule {
        self.store.list(None, None).unwrap().remove(0)
    }

    fn calls(&self) -> usize {
        self.runner.calls.lock().unwrap().len()
    }

    fn hold(&self) -> (watch::Sender<bool>, watch::Receiver<usize>) {
        let (release, receiver) = watch::channel(false);
        let (entered, calls) = watch::channel(0);
        *self.runner.before_reply.lock().unwrap() = Some(Arc::new(move |index| {
            let mut receiver = receiver.clone();
            let entered = entered.clone();
            Box::pin(async move {
                entered.send_replace(index + 1);
                wait_for_release(&mut receiver).await;
            })
        }));
        (release, calls)
    }
}

async fn shutdown(handle: &CronWorkerHandle) {
    handle
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(10))
        .await;
}

async fn wait_for_entered(receiver: &mut watch::Receiver<usize>, expected: usize) {
    let observed = receiver.clone();
    match tokio::time::timeout(
        TEST_WAIT_TIMEOUT,
        receiver.wait_for(|count| *count == expected),
    )
    .await
    {
        Ok(result) => {
            result.expect("hook entry sender alive");
        }
        Err(_) => panic!(
            "hook entry wait timed out: expected count {expected}, observed {}",
            *observed.borrow()
        ),
    }
}

async fn wait_for_release(receiver: &mut watch::Receiver<bool>) {
    let observed = receiver.clone();
    match tokio::time::timeout(TEST_WAIT_TIMEOUT, receiver.wait_for(|released| *released)).await {
        Ok(result) => {
            result.expect("hook release sender alive");
        }
        Err(_) => panic!(
            "hook release wait timed out: expected true, observed {}",
            *observed.borrow()
        ),
    }
}

#[tokio::test]
async fn atm_cron_public_surface_is_worker_and_scheduler_trait() {
    let fixture = Fixture::new();
    let worker = fixture.cron.start_worker().await.unwrap();

    // Keep scheduler operations behind the public trait boundary while the
    // worker lifecycle remains an AtmCron operation.
    let scheduler: Arc<dyn CronScheduler> = fixture.cron.clone();
    scheduler.start(fixture.spec("api-surface")).await.unwrap();
    assert_eq!(scheduler.list(None, None).await.unwrap().len(), 1);
    scheduler
        .end(
            &"test-team".parse().unwrap(),
            &PhaseId::parse("bk").unwrap(),
            Some("api-surface"),
        )
        .await
        .unwrap();
    let rows = scheduler.list(None, None).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].active);

    shutdown(&worker).await;
}

#[test]
fn atm_cron_has_no_unapproved_public_inherent_methods() {
    let source = syn::parse_file(include_str!("../src/scheduler.rs"))
        .expect("scheduler.rs must remain valid Rust syntax");
    let implementation = source
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Impl(implementation)
                if implementation.trait_.is_none()
                    && matches!(
                        implementation.self_ty.as_ref(),
                        syn::Type::Path(path) if path.path.is_ident("AtmCron")
                    ) =>
            {
                Some(implementation)
            }
            _ => None,
        })
        .expect("scheduler.rs must define the inherent AtmCron implementation");
    let mut public_methods: Vec<_> = implementation
        .items
        .iter()
        .filter_map(|item| match item {
            syn::ImplItem::Fn(method) if matches!(method.vis, syn::Visibility::Public(_)) => {
                Some(method.sig.ident.to_string())
            }
            _ => None,
        })
        .collect();
    public_methods.sort();

    assert_eq!(public_methods, ["new", "start_worker"]);
}

#[tokio::test]
async fn start_refuses_invalid_expression_before_store_write() {
    let fixture = Fixture::new();
    let mut spec = fixture.spec("invalid");
    spec.cron_expr = "* * * * *".into();
    assert!(fixture.cron.start(spec).await.is_err());
    assert_eq!(fixture.store.writes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn start_refuses_until_in_the_past() {
    let fixture = Fixture::new();
    for offset in [-1, 0] {
        let mut spec = fixture.spec("past");
        spec.stop = StopCondition::Until {
            at: fixture.clock.now() + chrono::Duration::seconds(offset),
        };
        assert!(
            fixture
                .cron
                .start(spec)
                .await
                .unwrap_err()
                .message()
                .contains("until is not in the future")
        );
    }
    assert_eq!(fixture.store.writes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn start_after_end_updates_in_place_and_keeps_the_id() {
    let fixture = Fixture::new();
    let spec = fixture.spec("first");
    let row = fixture.cron.start(spec.clone()).await.unwrap();
    assert!(fixture.cron.start(spec.clone()).await.is_err());
    fixture
        .cron
        .end(&spec.team, &spec.phase, Some(&spec.name))
        .await
        .unwrap();
    let restarted = fixture.cron.start(spec).await.unwrap();
    assert_eq!(row.id, restarted.id);
    assert_eq!(restarted.generation, row.generation + 1);
}

#[tokio::test]
async fn start_returns_cron_expr_byte_identical() {
    let fixture = Fixture::new();
    let mut spec = fixture.spec("exact");
    spec.cron_expr = " 0 * * * * * ".into();
    assert_eq!(
        fixture.cron.start(spec.clone()).await.unwrap().cron_expr,
        spec.cron_expr
    );
}

#[tokio::test]
async fn start_stamps_created_at_from_the_injected_clock() {
    let fixture = Fixture::new();
    fixture.clock.advance(chrono::Duration::seconds(17));
    assert_eq!(
        fixture
            .cron
            .start(fixture.spec("stamp"))
            .await
            .unwrap()
            .created_at
            .into_inner(),
        fixture.clock.now()
    );
}

#[tokio::test]
async fn start_requires_one_stop_condition_by_type() {
    let fixture = Fixture::new();
    let mut spec = fixture.spec("stop");
    spec.stop = StopCondition::Hook;
    assert_eq!(
        fixture.cron.start(spec.clone()).await.unwrap().stop,
        spec.stop
    );
}

#[tokio::test]
async fn initial_load_error_is_returned_by_start_worker() {
    let fixture = Fixture::new();
    fixture.store.fail_active_all.store(true, Ordering::SeqCst);
    assert!(fixture.cron.start_worker().await.is_err());
}

#[tokio::test]
async fn stopped_worker_refuses_start_end_and_list() {
    let fixture = Fixture::new();
    let worker = fixture.worker().await;
    shutdown(&worker).await;
    let spec = fixture.spec("stopped");
    assert_eq!(
        fixture.cron.list(None, None).await.unwrap_err().code(),
        AtmErrorCode::DaemonUnavailable
    );
    assert!(
        fixture
            .cron
            .start(spec.clone())
            .await
            .unwrap_err()
            .message()
            .contains("cron worker stopped")
    );
    assert!(
        fixture
            .cron
            .end(&spec.team, &spec.phase, None)
            .await
            .unwrap_err()
            .message()
            .contains("cron worker stopped")
    );
    assert!(
        fixture
            .cron
            .list(None, None)
            .await
            .unwrap_err()
            .message()
            .contains("cron worker stopped")
    );
}

#[tokio::test]
async fn starting_worker_twice_returns_lifecycle_error_code() {
    let fixture = Fixture::new();
    let worker = fixture.cron.start_worker().await.unwrap();
    let error = fixture
        .cron
        .start_worker()
        .await
        .err()
        .expect("second worker start must be rejected");

    assert_eq!(error.code(), AtmErrorCode::DaemonLifecycleWedge);
    assert!(error.message().contains("cron worker already started"));
    shutdown(&worker).await;
}

#[tokio::test]
async fn shutdown_before_first_worker_poll_refuses_commands() {
    let fixture = Fixture::new();
    let worker = fixture.cron.start_worker().await.unwrap();
    worker.shutdown(tokio::time::Instant::now()).await;
    assert!(
        fixture
            .cron
            .list(None, None)
            .await
            .unwrap_err()
            .message()
            .contains("cron worker stopped")
    );
}

#[tokio::test]
async fn ticks_fire_only_when_the_manual_clock_reaches_them() {
    let fixture = Fixture::new();
    fixture.reply(HookReply::PassThrough);
    fixture.cron.start(fixture.spec("tick")).await.unwrap();
    let worker = fixture.worker().await;
    fixture.step(59).await;
    assert_eq!(fixture.calls(), 0);
    fixture.step(1).await;
    shutdown(&worker).await;
    assert_eq!(fixture.calls(), 1);
    assert_eq!(fixture.row().last_outcome.as_deref(), Some("ok"));
}

#[tokio::test]
async fn missed_ticks_are_not_replayed_after_a_pause() {
    let fixture = Fixture::new();
    fixture.reply(HookReply::PassThrough);
    fixture.cron.start(fixture.spec("missed")).await.unwrap();
    let worker = fixture.worker().await;
    fixture.step(180 * 60).await;
    shutdown(&worker).await;
    assert_eq!(fixture.calls(), 1);
}

#[tokio::test]
async fn tick_while_in_flight_is_skipped_and_logged() {
    let fixture = Fixture::new();
    let logs = capture_logs();
    fixture.reply(HookReply::PassThrough);
    fixture.cron.start(fixture.spec("held")).await.unwrap();
    let (release, mut entered) = fixture.hold();
    let worker = fixture.worker().await;
    fixture.step(60).await;
    wait_for_entered(&mut entered, 1).await;
    fixture.step(60).await;
    fixture.step(60).await;
    release.send_replace(true);
    shutdown(&worker).await;
    assert_eq!(fixture.calls(), 1);
    assert_eq!(
        logs.lock()
            .unwrap()
            .iter()
            .filter(|entry| entry.contains("outcome=\"skipped_in_flight\""))
            .count(),
        2
    );
}

#[tokio::test]
async fn until_reached_ends_the_schedule_without_a_run() {
    let fixture = Fixture::new();
    fixture.reply(HookReply::PassThrough);
    let mut spec = fixture.spec("until");
    spec.stop = StopCondition::Until {
        at: fixture.clock.now() + chrono::Duration::seconds(90),
    };
    let (release, mut entered) = fixture.hold();
    fixture.cron.start(spec).await.unwrap();
    let worker = fixture.worker().await;
    fixture.step(60).await;
    wait_for_entered(&mut entered, 1).await;
    release.send_replace(true);
    fixture.step(30).await;
    fixture.step(30).await;
    shutdown(&worker).await;
    let row = fixture.row();
    assert!(!row.active);
    assert_eq!(
        row.ended_at.unwrap().into_inner(),
        "2026-10-19T00:01:30Z".parse::<DateTime<Utc>>().unwrap()
    );
    assert_eq!(row.last_outcome.as_deref(), Some("stopped: until reached"));
    assert_eq!(fixture.calls(), 1);
}

#[tokio::test]
async fn refused_records_and_dispatches_nothing() {
    let fixture = Fixture::new();
    fixture.reply(HookReply::Refused {
        message: "quota".into(),
    });
    fixture.cron.start(fixture.spec("refused")).await.unwrap();
    let worker = fixture.worker().await;
    fixture.step(60).await;
    shutdown(&worker).await;
    assert_eq!(
        fixture.row().last_outcome.as_deref(),
        Some("refused: quota")
    );
    assert!(fixture.row().active);
    assert!(fixture.dispatcher.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn pass_through_and_not_applicable_record_ok() {
    for reply in [HookReply::PassThrough, HookReply::NotApplicable] {
        let fixture = Fixture::new();
        fixture.reply(reply);
        fixture.cron.start(fixture.spec("ok")).await.unwrap();
        let worker = fixture.worker().await;
        fixture.step(60).await;
        shutdown(&worker).await;
        assert_eq!(fixture.row().last_outcome.as_deref(), Some("ok"));
    }
}

#[tokio::test]
async fn stop_hook_true_ends_the_schedule_with_its_reason() {
    let fixture = Fixture::new();
    let mut spec = fixture.spec("stop");
    spec.stop = StopCondition::Hook;
    fixture
        .runner
        .stop_replies
        .lock()
        .unwrap()
        .push_back(Ok(StopReply {
            stop: true,
            reason: Some("phase bead closed".into()),
        }));
    fixture.cron.start(spec).await.unwrap();
    let worker = fixture.worker().await;
    fixture.step(60).await;
    shutdown(&worker).await;
    assert_eq!(fixture.calls(), 1);
    assert!(!fixture.row().active);
    assert_eq!(
        fixture.row().last_outcome.as_deref(),
        Some("stopped: phase bead closed")
    );
}

#[tokio::test]
async fn stop_hook_false_runs_the_tick() {
    let fixture = Fixture::new();
    fixture.reply(HookReply::PassThrough);
    let mut spec = fixture.spec("stop");
    spec.stop = StopCondition::Hook;
    fixture
        .runner
        .stop_replies
        .lock()
        .unwrap()
        .push_back(Ok(StopReply {
            stop: false,
            reason: None,
        }));
    fixture.cron.start(spec.clone()).await.unwrap();
    let worker = fixture.worker().await;
    fixture.step(60).await;
    shutdown(&worker).await;
    assert_eq!(fixture.calls(), 2);
    assert_eq!(fixture.row().last_outcome.as_deref(), Some("ok"));
    let calls = fixture.runner.calls.lock().unwrap();
    let HookCall::Stop(request) = &calls[0].1 else {
        panic!("not stop check")
    };
    assert_eq!(request.custom, spec.custom);
    assert_eq!(request.hook.as_str(), "cron.stop");
    assert_eq!(
        request.request,
        serde_json::json!({"args":spec.args,"created_by":TEST_SENDER})
    );
    assert_eq!(request.caller.identity, spec.created_by);
    assert_eq!(calls[0].0.timeout_ms, 20000);
    assert_eq!(calls[1].0.timeout_ms, 20000);
}

#[tokio::test]
async fn tick_request_has_the_seven_keys_and_args_in_request_only() {
    let fixture = Fixture::new();
    fixture.reply(HookReply::PassThrough);
    let spec = fixture.spec("request");
    fixture.cron.start(spec.clone()).await.unwrap();
    let worker = fixture.worker().await;
    fixture.step(60).await;
    shutdown(&worker).await;
    let calls = fixture.runner.calls.lock().unwrap();
    let (target, call, _) = &calls[0];
    let HookCall::Run(request) = call else {
        panic!("not tick")
    };
    assert_eq!(
        serde_json::to_value(request)
            .unwrap()
            .as_object()
            .unwrap()
            .len(),
        7
    );
    assert_eq!(request.hook.as_str(), "cron.tick");
    assert_eq!(request.custom, spec.custom);
    assert_eq!(
        request.request,
        serde_json::json!({"args": spec.args, "created_by": TEST_SENDER})
    );
    assert_eq!(request.caller.identity, spec.created_by);
    assert_eq!(request.caller.team, spec.team);
    assert_eq!(request.caller.host.as_ref().unwrap().as_str(), "test-host");
    assert_eq!(target.module_path, spec.module_path);
    assert_eq!(target.cwd, spec.cwd);
    assert_eq!(target.timeout_ms, spec.timeout_ms);
}

#[tokio::test]
async fn cron_tick_uses_cron_timeout_ms_not_timeout_ms() {
    let fixture = Fixture::new();
    fixture.reply(HookReply::PassThrough);
    let mut spec = fixture.spec("timeout");
    spec.stop = StopCondition::Hook;
    fixture
        .runner
        .stop_replies
        .lock()
        .unwrap()
        .push_back(Ok(StopReply {
            stop: false,
            reason: None,
        }));
    let (release, mut entered) = fixture.hold();
    fixture.cron.start(spec).await.unwrap();
    let worker = fixture.worker().await;
    fixture.step(60).await;
    wait_for_entered(&mut entered, 1).await;
    {
        let calls = fixture.runner.calls.lock().unwrap();
        assert_eq!(calls[0].0.timeout_ms, 20000);
        let remaining = calls[0].2.remaining_ms_at(fixture.clock.monotonic_now());
        assert_eq!(remaining, 20500);
        assert!(remaining > u64::from(atm_core::boundary::HOOK_TIMEOUT_MAX_MS));
        assert!(remaining <= 20500);
    }
    release.send_replace(true);
    shutdown(&worker).await;
    assert_eq!(fixture.calls(), 2);
    for (_, _, deadline) in fixture.runner.calls.lock().unwrap().iter() {
        assert_eq!(
            deadline.remaining_ms_at(fixture.clock.monotonic_now()),
            20500
        );
    }
}

#[tokio::test]
async fn worker_schedules_only_active_rows() {
    let fixture = Fixture::new();
    for name in ["first", "second", "ended"] {
        fixture.cron.start(fixture.spec(name)).await.unwrap();
        fixture.reply(HookReply::PassThrough);
    }
    let spec = fixture.spec("ended");
    fixture
        .cron
        .end(&spec.team, &spec.phase, Some("ended"))
        .await
        .unwrap();
    let worker = fixture.worker().await;
    fixture.step(90).await;
    shutdown(&worker).await;
    assert_eq!(fixture.calls(), 2);
}

#[tokio::test]
async fn restart_inside_jitter_window_still_fires_that_occurrence() {
    let fixture = Fixture::new();
    fixture.jitter.values.lock().unwrap().push_back(600);
    let mut spec = fixture.spec("window");
    spec.cron_expr = "@hourly".into();
    fixture.cron.start(spec).await.unwrap();
    fixture.reply(HookReply::PassThrough);
    fixture.clock.set("2026-10-19T10:00:30Z".parse().unwrap());
    let worker = fixture.worker().await;
    fixture.step(569).await;
    assert_eq!(fixture.calls(), 0);
    fixture.step(1).await;
    shutdown(&worker).await;
    assert_eq!(fixture.calls(), 1);
}

#[tokio::test]
async fn worker_fires_at_occurrence_plus_jitter() {
    let fixture = Fixture::new();
    fixture.clock.advance(chrono::Duration::minutes(30));
    fixture.jitter.values.lock().unwrap().push_back(600);
    let mut spec = fixture.spec("jitter");
    spec.cron_expr = "@hourly".into();
    fixture.cron.start(spec).await.unwrap();
    fixture.reply(HookReply::PassThrough);
    let worker = fixture.worker().await;
    fixture.step(1800).await;
    assert_eq!(fixture.calls(), 0);
    fixture.step(600).await;
    shutdown(&worker).await;
    assert_eq!(fixture.calls(), 1);
}

#[tokio::test]
async fn jitter_is_within_half_period_capped_at_30min() {
    for (expression, upper) in [("0 * * * * *", 30), ("@hourly", 1800), ("@daily", 1800)] {
        let fixture = Fixture::new();
        let mut spec = fixture.spec("jitter");
        spec.cron_expr = expression.into();
        fixture.jitter.values.lock().unwrap().push_back(upper - 1);
        assert_eq!(fixture.cron.start(spec).await.unwrap().jitter_s, upper - 1);
        assert_eq!(*fixture.jitter.calls.lock().unwrap(), vec![upper]);
    }
}

#[tokio::test]
async fn jitter_avoids_offsets_held_by_same_expression() {
    let fixture = Fixture::new();
    for index in 0..30 {
        let row = fixture
            .cron
            .start(fixture.spec(&format!("job-{index}")))
            .await
            .unwrap();
        assert_eq!(row.jitter_s, index);
    }
    assert_eq!(fixture.jitter.calls.lock().unwrap().last(), Some(&1));
    assert_eq!(
        fixture
            .cron
            .start(fixture.spec("full"))
            .await
            .unwrap()
            .jitter_s,
        0
    );
    assert_eq!(fixture.jitter.calls.lock().unwrap().last(), Some(&30));
    let mut other = fixture.spec("other");
    other.cron_expr = "1 * * * * *".into();
    assert_eq!(fixture.cron.start(other).await.unwrap().jitter_s, 0);
    assert_eq!(fixture.jitter.calls.lock().unwrap().last(), Some(&30));
}

#[tokio::test]
async fn jitter_persists_across_restart() {
    let fixture = Fixture::new();
    fixture.jitter.values.lock().unwrap().push_back(17);
    let row = fixture.cron.start(fixture.spec("persist")).await.unwrap();
    let worker = fixture.worker().await;
    shutdown(&worker).await;
    let restarted = Arc::new(AtmCron::new(AtmCronDeps {
        store: fixture.store.clone(),
        runner: fixture.runner.clone(),
        dispatcher: fixture.dispatcher.clone(),
        clock: fixture.clock.clone(),
        host: None,
        jitter: fixture.jitter.clone(),
    }));
    let count = fixture.clock.parks();
    let worker = restarted.start_worker().await.unwrap();
    fixture.clock.wait_parks_above(count).await;
    shutdown(&worker).await;
    assert_eq!(fixture.row().jitter_s, row.jitter_s);
    assert_eq!(fixture.jitter.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn concurrent_starts_same_expression_get_distinct_jitter() {
    let fixture = Fixture::new();
    let (first, second) = tokio::join!(
        fixture.cron.start(fixture.spec("first")),
        fixture.cron.start(fixture.spec("second"))
    );
    assert_ne!(first.unwrap().jitter_s, second.unwrap().jitter_s);
}

#[tokio::test]
async fn start_while_running_ticks_without_restart() {
    let fixture = Fixture::new();
    let worker = fixture.worker().await;
    let parks = fixture.clock.parks();
    fixture.cron.start(fixture.spec("added")).await.unwrap();
    fixture.clock.wait_parks_above(parks).await;
    fixture.reply(HookReply::PassThrough);
    fixture.step(60).await;
    shutdown(&worker).await;
    assert_eq!(fixture.calls(), 1);
}

#[tokio::test]
async fn end_stops_its_ticks() {
    let fixture = Fixture::new();
    let spec = fixture.spec("ended");
    fixture.cron.start(spec.clone()).await.unwrap();
    let worker = fixture.worker().await;
    let parks = fixture.clock.parks();
    fixture
        .cron
        .end(&spec.team, &spec.phase, Some(&spec.name))
        .await
        .unwrap();
    fixture.clock.wait_parks_above(parks).await;
    fixture.step(120).await;
    shutdown(&worker).await;
    assert_eq!(fixture.calls(), 0);
}

#[tokio::test]
async fn reload_keeps_in_flight_of_an_unchanged_slot() {
    let fixture = Fixture::new();
    let spec = fixture.spec("first");
    fixture.cron.start(spec.clone()).await.unwrap();
    fixture.reply(HookReply::PassThrough);
    let (release, mut entered) = fixture.hold();
    let worker = fixture.worker().await;
    fixture.step(60).await;
    wait_for_entered(&mut entered, 1).await;
    let parks = fixture.clock.parks();
    fixture
        .cron
        .end(&spec.team, &spec.phase, Some("absent"))
        .await
        .unwrap();
    fixture.clock.wait_parks_above(parks).await;
    release.send_replace(true);
    shutdown(&worker).await;
    assert_eq!(fixture.row().last_outcome.as_deref(), Some("ok"));
    assert_eq!(fixture.calls(), 1);
}

#[tokio::test]
async fn end_then_start_before_reload_applies_new_expression_and_jitter() {
    let fixture = Fixture::new();
    fixture.clock.advance(chrono::Duration::minutes(30));
    let spec = fixture.spec("restart");
    let row = fixture.cron.start(spec.clone()).await.unwrap();
    let worker = fixture.worker().await;
    fixture
        .store
        .end(
            &spec.team,
            &spec.phase,
            Some(&spec.name),
            fixture.clock.now().into(),
        )
        .unwrap();
    let mut new_spec = spec;
    new_spec.cron_expr = "@hourly".into();
    fixture.jitter.values.lock().unwrap().push_back(600);
    let parks = fixture.clock.parks();
    let restarted = fixture.cron.start(new_spec).await.unwrap();
    fixture.clock.wait_parks_above(parks).await;
    fixture.reply(HookReply::PassThrough);
    fixture.step(1800).await;
    assert_eq!(fixture.calls(), 0);
    fixture.step(600).await;
    shutdown(&worker).await;
    assert_eq!(row.id, restarted.id);
    assert_eq!(restarted.jitter_s, 600);
    assert_eq!(fixture.calls(), 1);
}

#[tokio::test]
async fn stale_run_never_writes_the_restarted_row() {
    let fixture = Fixture::new();
    let spec = fixture.spec("restart");
    let row = fixture.cron.start(spec.clone()).await.unwrap();
    let (release, mut entered) = fixture.hold();
    fixture.reply(HookReply::PassThrough);
    let worker = fixture.worker().await;
    fixture.step(60).await;
    wait_for_entered(&mut entered, 1).await;
    fixture
        .store
        .end(
            &spec.team,
            &spec.phase,
            Some(&spec.name),
            fixture.clock.now().into(),
        )
        .unwrap();
    let parks = fixture.clock.parks();
    let restarted = fixture.cron.start(spec).await.unwrap();
    fixture.clock.wait_parks_above(parks).await;
    release.send_replace(true);
    shutdown(&worker).await;
    assert_eq!(restarted.generation, row.generation + 1);
    assert_eq!(fixture.row().last_outcome, None);
    assert_eq!(fixture.store.writes.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn restart_during_tick_does_not_replay_after_reload() {
    let fixture = Fixture::new();
    let spec = fixture.spec("restart");
    fixture.cron.start(spec.clone()).await.unwrap();
    let (release, mut entered) = fixture.hold();
    let worker = fixture.worker().await;
    fixture.step(60).await;
    wait_for_entered(&mut entered, 1).await;
    fixture
        .store
        .end(
            &spec.team,
            &spec.phase,
            Some(&spec.name),
            fixture.clock.now().into(),
        )
        .unwrap();
    let parks = fixture.clock.parks();
    fixture.cron.start(spec).await.unwrap();
    fixture.clock.wait_parks_above(parks).await;
    release.send_replace(true);
    shutdown(&worker).await;
    assert_eq!(fixture.calls(), 1);
    assert_eq!(fixture.row().last_run_at, None);
}

#[tokio::test]
async fn record_run_after_end_mid_run_writes_nothing() {
    let fixture = Fixture::new();
    let spec = fixture.spec("ended");
    fixture.reply(HookReply::PassThrough);
    fixture.cron.start(spec.clone()).await.unwrap();
    let (release, mut entered) = fixture.hold();
    let worker = fixture.worker().await;
    fixture.step(60).await;
    wait_for_entered(&mut entered, 1).await;
    fixture
        .store
        .end(&spec.team, &spec.phase, None, fixture.clock.now().into())
        .unwrap();
    release.send_replace(true);
    shutdown(&worker).await;
    assert_eq!(fixture.row().last_outcome, None);
    assert_eq!(fixture.store.writes.load(Ordering::SeqCst), 2);
}

fn errors() -> Vec<(AtmError, &'static str)> {
    vec![
        (
            AtmError::hook_exit(atm_core::boundary::HookExitCause::Missing("test".into())),
            "exit",
        ),
        (
            AtmError::hook_timeout(&PhaseId::parse("bk").unwrap(), "test"),
            "timeout",
        ),
        (AtmError::hook_malformed("test"), "malformed"),
    ]
}

#[tokio::test]
async fn runner_errors_record_and_dispatch_nothing() {
    for (error, expected) in errors() {
        let fixture = Fixture::new();
        fixture.runner.replies.lock().unwrap().push_back(Err(error));
        fixture.cron.start(fixture.spec("error")).await.unwrap();
        let worker = fixture.worker().await;
        fixture.step(60).await;
        shutdown(&worker).await;
        assert_eq!(fixture.row().last_outcome.as_deref(), Some(expected));
        assert!(fixture.dispatcher.calls.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn stop_hook_failure_skips_the_tick_and_keeps_the_row_active() {
    for (error, _) in errors() {
        let fixture = Fixture::new();
        let code = error.code();
        fixture
            .runner
            .stop_replies
            .lock()
            .unwrap()
            .push_back(Err(error));
        let mut spec = fixture.spec("error");
        spec.stop = StopCondition::Hook;
        fixture.cron.start(spec).await.unwrap();
        let worker = fixture.worker().await;
        fixture.step(60).await;
        shutdown(&worker).await;
        assert!(fixture.row().active);
        assert_eq!(fixture.calls(), 1);
        assert_eq!(
            fixture.row().last_outcome,
            Some(format!("stop_check_failed: {code}"))
        );
    }
}

#[tokio::test]
async fn two_schedules_same_module_different_custom_get_their_own() {
    let fixture = Fixture::new();
    for name in ["first", "second"] {
        let mut spec = fixture.spec(name);
        spec.custom.insert("name".into(), name.into());
        fixture.cron.start(spec).await.unwrap();
        fixture.reply(HookReply::PassThrough);
    }
    let worker = fixture.worker().await;
    fixture.step(90).await;
    shutdown(&worker).await;
    let calls = fixture.runner.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    let mut names: Vec<_> = calls
        .iter()
        .map(|(_, call, _)| {
            let HookCall::Run(request) = call else {
                panic!("not run")
            };
            request.custom["name"].as_str().unwrap().to_owned()
        })
        .collect();
    names.sort();
    assert_eq!(names, vec!["first", "second"]);
}

#[tokio::test]
async fn loaded_row_without_future_occurrence_is_ended() {
    let fixture = Fixture::new();
    fixture.clock.set("2019-01-01T00:00:00Z".parse().unwrap());
    let mut spec = fixture.spec("expired");
    spec.cron_expr = "0 0 0 1 1 * 2020".into();
    spec.stop = StopCondition::Hook;
    fixture.cron.start(spec).await.unwrap();
    fixture.clock.set("2026-10-19T00:00:00Z".parse().unwrap());
    let worker = fixture.worker().await;
    shutdown(&worker).await;
    assert!(!fixture.row().active);
    assert_eq!(
        fixture.row().last_outcome.as_deref(),
        Some("stopped: no future occurrence")
    );
}

#[tokio::test]
async fn last_finite_occurrence_ends_the_schedule() {
    let fixture = Fixture::new();
    let mut spec = fixture.spec("last");
    spec.cron_expr = "0 0 0 20 10 * 2026".into();
    spec.stop = StopCondition::Until {
        at: fixture.clock.now() + chrono::Duration::days(2),
    };
    fixture.cron.start(spec).await.unwrap();
    fixture.reply(HookReply::PassThrough);
    let (release, mut entered) = fixture.hold();
    let worker = fixture.worker().await;

    fixture.step(86_400).await;
    wait_for_entered(&mut entered, 1).await;
    let parks = fixture.clock.parks();
    release.send(true).unwrap();
    fixture.clock.wait_parks_above(parks).await;

    assert_eq!(fixture.calls(), 1);
    assert!(!fixture.row().active);
    assert_eq!(
        fixture.row().last_outcome.as_deref(),
        Some("stopped: no future occurrence")
    );
    shutdown(&worker).await;
}

#[tokio::test]
async fn shutdown_joins_within_deadline_with_run_in_flight() {
    let fixture = Fixture::new();
    fixture.cron.start(fixture.spec("held")).await.unwrap();
    let (_release, mut entered) = fixture.hold();
    let worker = fixture.worker().await;
    fixture.step(60).await;
    wait_for_entered(&mut entered, 1).await;
    worker.shutdown(tokio::time::Instant::now()).await;
    assert!(fixture.cron.list(None, None).await.is_err());
    assert_eq!(fixture.row().last_outcome, None);
}

#[tokio::test]
async fn shutdown_waits_for_cancelled_run_cleanup() {
    struct Dropped(watch::Sender<bool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.send_replace(true);
        }
    }
    let fixture = Fixture::new();
    let (entered, mut received) = watch::channel(false);
    let (dropped, finished) = watch::channel(false);
    *fixture.runner.before_reply.lock().unwrap() = Some(Arc::new(move |_| {
        let guard = Dropped(dropped.clone());
        let entered = entered.clone();
        Box::pin(async move {
            let _guard = guard;
            entered.send_replace(true);
            std::future::pending::<()>().await;
        })
    }));
    fixture.cron.start(fixture.spec("cleanup")).await.unwrap();
    let worker = fixture.worker().await;
    fixture.step(60).await;
    wait_for_release(&mut received).await;
    worker.shutdown(tokio::time::Instant::now()).await;
    assert!(*finished.borrow());
    assert_eq!(fixture.row().last_outcome, None);
}

fn actions() -> Vec<atm_core::hook::HookAction> {
    serde_json::from_value(serde_json::json!([
        {"command":"task.move","request":{"caller_identity":"other","caller_team":"test-team","task_id":"first","target":{"to":"end"}}},
        {"command":"task.move","request":{"caller_identity":"other","caller_team":"test-team","task_id":"second","target":{"to":"end"}}},
        {"command":"task.move","request":{"caller_identity":"other","caller_team":"test-team","task_id":"third","target":{"to":"end"}}}
    ]))
    .unwrap()
}

#[tokio::test]
async fn three_actions_dispatch_once_in_order_as_created_by() {
    let fixture = Fixture::new();
    let expected = actions();
    fixture.reply(HookReply::Handled(expected.clone()));
    fixture.cron.start(fixture.spec("actions")).await.unwrap();
    let worker = fixture.worker().await;
    fixture.step(60).await;
    shutdown(&worker).await;
    let calls = fixture.dispatcher.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].actions, expected);
    assert_eq!(calls[0].spec.default_as.as_str(), TEST_SENDER);
    let run_deadlines: Vec<_> = fixture
        .runner
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(_, call, deadline)| matches!(call, HookCall::Run(_)).then_some(*deadline))
        .collect();
    assert_eq!(run_deadlines.len(), calls.len());
    for (dispatch, run_deadline) in calls.iter().zip(run_deadlines) {
        assert_eq!(dispatch.spec.deadline, Some(run_deadline));
    }
    assert!(!calls[0].spec.dry_run);
    assert_eq!(fixture.row().last_outcome.as_deref(), Some("actions: 3"));
}

#[tokio::test]
async fn dispatcher_error_records_action_err() {
    let fixture = Fixture::with_dispatcher(RecordingHookActionDispatcher {
        fail_at: Some(1),
        ..Default::default()
    });
    fixture.reply(HookReply::Handled(actions()));
    fixture.cron.start(fixture.spec("actions")).await.unwrap();
    let worker = fixture.worker().await;
    fixture.step(60).await;
    shutdown(&worker).await;
    assert_eq!(fixture.dispatcher.calls.lock().unwrap().len(), 1);
    assert_eq!(
        fixture.row().last_outcome.as_deref(),
        Some("action_err: ATM_HOOK_MALFORMED")
    );
}

#[tokio::test]
async fn panicking_run_records_exit_and_worker_keeps_ticking() {
    let fixture = Fixture::new();
    fixture.runner.panic_on_run.store(true, Ordering::SeqCst);
    fixture.cron.start(fixture.spec("panic")).await.unwrap();
    let (release, mut entered) = fixture.hold();
    let worker = fixture.worker().await;
    fixture.step(60).await;
    wait_for_entered(&mut entered, 1).await;
    let parks = fixture.clock.parks();
    release.send_replace(true);
    fixture.clock.wait_parks_above(parks).await;
    assert!(
        fixture
            .row()
            .last_outcome
            .unwrap()
            .starts_with("exit: hook runner panicked: ")
    );
    fixture.runner.panic_on_run.store(false, Ordering::SeqCst);
    fixture.reply(HookReply::PassThrough);
    fixture.step(60).await;
    shutdown(&worker).await;
    assert_eq!(fixture.calls(), 2);
    assert_eq!(fixture.row().last_outcome.as_deref(), Some("ok"));
}

#[tokio::test]
async fn cancelled_run_records_nothing() {
    let fixture = Fixture::new();
    let logs = capture_logs();
    let spec = fixture.spec("cancelled");
    fixture.cron.start(spec.clone()).await.unwrap();
    let (_release, mut entered) = fixture.hold();
    let worker = fixture.worker().await;
    fixture.step(60).await;
    wait_for_entered(&mut entered, 1).await;
    let parks = fixture.clock.parks();
    fixture
        .cron
        .end(&spec.team, &spec.phase, Some(&spec.name))
        .await
        .unwrap();
    fixture.clock.wait_parks_above(parks).await;
    shutdown(&worker).await;
    assert_eq!(fixture.row().last_outcome, None);
    assert_eq!(
        logs.lock()
            .unwrap()
            .iter()
            .filter(|entry| entry.contains("action=\"cancelled\""))
            .count(),
        1
    );
}

#[tokio::test]
async fn record_failure_is_logged() {
    let fixture = Fixture::new();
    let logs = capture_logs();
    fixture.reply(HookReply::PassThrough);
    fixture.cron.start(fixture.spec("record")).await.unwrap();
    let (release, mut entered) = fixture.hold();
    let worker = fixture.worker().await;
    fixture.step(60).await;
    wait_for_entered(&mut entered, 1).await;
    fixture.store.fail_writes.store(true, Ordering::SeqCst);
    let parks = fixture.clock.parks();
    release.send_replace(true);
    fixture.clock.wait_parks_above(parks).await;
    fixture.store.fail_writes.store(false, Ordering::SeqCst);
    fixture.reply(HookReply::PassThrough);
    fixture.step(60).await;
    shutdown(&worker).await;
    assert_eq!(fixture.calls(), 2);
    assert_eq!(fixture.row().last_outcome.as_deref(), Some("ok"));
    assert!(
        logs.lock()
            .unwrap()
            .iter()
            .any(|entry| entry.contains("action=\"record_failed\"") && entry.contains("error="))
    );
}

#[tokio::test]
async fn reload_failure_keeps_slots_and_retries() {
    let fixture = Fixture::new();
    let logs = capture_logs();
    let spec = fixture.spec("reload");
    fixture.cron.start(spec.clone()).await.unwrap();
    fixture.reply(HookReply::PassThrough);
    let (release, mut entered) = fixture.hold();
    let worker = fixture.worker().await;
    fixture.store.fail_active_all.store(true, Ordering::SeqCst);
    let parks = fixture.clock.parks();
    fixture
        .cron
        .end(&spec.team, &spec.phase, Some("absent"))
        .await
        .unwrap();
    fixture.clock.wait_parks_above(parks).await;
    fixture.step(60).await;
    wait_for_entered(&mut entered, 1).await;
    fixture.store.fail_active_all.store(false, Ordering::SeqCst);
    let parks = fixture.clock.parks();
    fixture
        .cron
        .start(fixture.spec("after-clear"))
        .await
        .unwrap();
    fixture.clock.wait_parks_above(parks).await;
    fixture.reply(HookReply::PassThrough);
    fixture.step(61).await;
    wait_for_entered(&mut entered, 2).await;
    release.send_replace(true);
    shutdown(&worker).await;
    assert_eq!(fixture.calls(), 2);
    assert!(
        logs.lock()
            .unwrap()
            .iter()
            .any(|entry| entry.contains("action=\"reload_failed\"") && entry.contains("error="))
    );
}

fn reload_failures(logs: &Arc<std::sync::Mutex<Vec<String>>>) -> usize {
    logs.lock()
        .unwrap()
        .iter()
        .filter(|entry| entry.contains("action=\"reload_failed\""))
        .count()
}

#[tokio::test]
async fn failed_reload_retries_on_backoff_before_the_next_occurrence() {
    let fixture = Fixture::new();
    let logs = capture_logs();
    let spec = fixture.spec("reload-backoff");
    fixture.cron.start(spec.clone()).await.unwrap();
    let worker = fixture.worker().await;
    fixture.store.fail_active_all.store(true, Ordering::SeqCst);
    let parks = fixture.clock.parks();
    fixture
        .cron
        .end(&spec.team, &spec.phase, Some("absent"))
        .await
        .unwrap();
    fixture.clock.wait_parks_above(parks).await;
    assert_eq!(reload_failures(&logs), 1);
    // The next occurrence is a minute away, so each retry below comes from
    // the backoff alone: 1s after the failure, then 2s, then 4s.
    fixture.step(1).await;
    assert_eq!(reload_failures(&logs), 2);
    fixture.step(1).await;
    assert_eq!(reload_failures(&logs), 2);
    fixture.step(1).await;
    assert_eq!(reload_failures(&logs), 3);
    shutdown(&worker).await;
}

#[tokio::test]
async fn tick_and_stop_check_log_entries() {
    let fixture = Fixture::new();
    let logs = capture_logs();
    let mut spec = fixture.spec("logs");
    spec.stop = StopCondition::Hook;
    fixture
        .runner
        .stop_replies
        .lock()
        .unwrap()
        .push_back(Ok(StopReply {
            stop: false,
            reason: None,
        }));
    fixture.reply(HookReply::PassThrough);
    fixture.cron.start(spec).await.unwrap();
    let worker = fixture.worker().await;
    fixture.step(60).await;
    shutdown(&worker).await;
    let logs = logs.lock().unwrap();
    for action in ["stop_check", "tick"] {
        let events: Vec<_> = logs
            .iter()
            .filter(|entry| entry.contains(&format!("action=\"{action}\"")))
            .collect();
        assert_eq!(events.len(), 1);
        for key in [
            "subsystem=\"atm_cron\"",
            "team=test-team",
            "phase=bk",
            "schedule=1",
            "hook=",
            "outcome=\"ok\"",
            "elapsed_ms=",
        ] {
            assert!(events[0].contains(key), "missing {key} in {}", events[0]);
        }
    }
}

/// Parks a run by blocking its runtime thread until the returned sender fires.
/// A run that cannot yield is not dropped when the worker is aborted, so its
/// count stays up until the thread is released.
fn hold_blocking(fixture: &Fixture) -> (std::sync::mpsc::Sender<()>, watch::Receiver<usize>) {
    let (release, parked) = std::sync::mpsc::channel::<()>();
    let parked = Arc::new(std::sync::Mutex::new(parked));
    let (entered, calls) = watch::channel(0);
    *fixture.runner.before_reply.lock().unwrap() = Some(Arc::new(move |index| {
        let parked = parked.clone();
        let entered = entered.clone();
        Box::pin(async move {
            entered.send_replace(index + 1);
            let _ = parked.lock().unwrap().recv();
        })
    }));
    (release, calls)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_returns_at_its_deadline_when_a_run_cannot_yield() {
    let fixture = Fixture::new();
    fixture.reply(HookReply::PassThrough);
    fixture.cron.start(fixture.spec("parked")).await.unwrap();
    let (release, mut entered) = hold_blocking(&fixture);
    let worker = fixture.worker().await;
    fixture.step(60).await;
    wait_for_entered(&mut entered, 1).await;
    let outcome = tokio::time::timeout(
        TEST_WAIT_TIMEOUT,
        worker.shutdown(tokio::time::Instant::now()),
    )
    .await;
    release.send(()).unwrap();
    outcome.expect("shutdown must return at its deadline, not wait for a run that cannot yield");
}
