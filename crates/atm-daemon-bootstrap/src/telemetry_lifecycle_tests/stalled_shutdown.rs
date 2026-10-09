//! BD6 cumulative shutdown deadline over a composed daemon with stalled
//! steps.
//!
//! Every step that can stall without changing a production API stalls: a
//! local write held open by its received hook (listener drain), a recovery
//! sweep and transition drain that never finish, a peer write whose detached
//! hook never finishes (peer-connection drain), task and workflow telemetry
//! sinks that never answer, a `$ATM_TEMP` sweep pass blocked in its
//! observability emit, an OTLP export to a collector that never answers
//! (exporter shutdown), and a retained-logger flush held on the blocking
//! pool (both logger flush steps). The clock stays real: the exporter step shuts
//! the SDK providers down in `spawn_blocking`, which stops a paused clock from
//! advancing while their exports wait on its timers. Exact virtual-time
//! deadline tests of the recovery sweep and the peer pool live beside those
//! types.
#![cfg(test)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use atm_core::AuthenticatedIngress;
use atm_core::api::{ApiRequest, RequestDeadline};
use atm_core::boundary::{
    AsyncMessageReceivedHookEmitter, BuiltInPostSendDispatch, MessageReceivedHookSelector,
    PostSendEmissionPath,
};
use atm_core::error::AtmError;
use atm_core::observability::{
    AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, CommandEvent, LogTailSession,
    NullObservability, ObservabilityPort,
};
use atm_core::protocol::RequestEnvelope;
use atm_core::{SweepConfig, WorkflowTelemetryRecord};
use atm_http_runtime::{CanonicalWriteHandler, RuntimeHealth};
use atm_runtime::{
    TaskTelemetryConfig, TaskTelemetryRuntime, WorkflowTelemetryConfig, WorkflowTelemetryRuntime,
};
use atm_runtime_test_support::task_telemetry::ReportingStalledTelemetrySink;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::time::Instant;

use super::{
    Daemon, EXPORT_WAIT, Receiver, assert_one_shutdown_deadline, endpoint_env, task_record,
};
use crate::DaemonLaunchIdentity;
use crate::atm_temp_sweeper_runtime::AtmTempSweeperRuntime;
use crate::daemon_observability::RETAINED_LOG_WRITER_SHUTDOWN_TIMEOUT;
use crate::queue_drain::RecoverySweepHandle;
use crate::shutdown_probe::{Probe, observe};

/// The longest emit and drain a telemetry runtime accepts, so only the shared
/// shutdown deadline can end a stalled drain.
const STALL_LIMIT: Duration = Duration::from_secs(30);

/// Reports each hook start, then never finishes.
struct StallingHook(UnboundedSender<()>);

impl atm_core::boundary::sealed::Sealed for StallingHook {}

impl AsyncMessageReceivedHookEmitter for StallingHook {
    fn emit_received_message(
        &self,
        _dispatch: BuiltInPostSendDispatch,
        _deadline: RequestDeadline,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<PostSendEmissionPath, AtmError>> + Send + '_>>
    {
        let _ = self.0.send(());
        Box::pin(std::future::pending())
    }
}

struct StallingSelector(StallingHook);

impl atm_core::boundary::sealed::Sealed for StallingSelector {}

impl MessageReceivedHookSelector for StallingSelector {
    fn select_emitter(
        &self,
        _dispatch: &BuiltInPostSendDispatch,
    ) -> Option<&dyn AsyncMessageReceivedHookEmitter> {
        Some(&self.0)
    }
}

/// A telemetry sink that reports each emit on `entered`, then never finishes it.
fn reporting_stalled_sink(entered: UnboundedSender<()>) -> ReportingStalledTelemetrySink {
    ReportingStalledTelemetrySink::new(move || {
        let _ = entered.send(());
    })
}

/// Blocks the calling thread until the paired [`Release`] is dropped.
type Blocked = Arc<Mutex<std::sync::mpsc::Receiver<()>>>;

fn block_until_released(blocked: &Blocked) {
    let _ = blocked
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .recv();
}

/// An observability port whose `emit` reports entry, then blocks its thread
/// until the paired [`Release`] is dropped.
struct BlockingEmit {
    entered: UnboundedSender<()>,
    release: Blocked,
}

/// Unblocks every blocked thread when dropped, so the test runtime's
/// shutdown can join the blocked sweep and flush threads.
struct Release(#[allow(dead_code)] std::sync::mpsc::Sender<()>);

impl atm_core::boundary::sealed::Sealed for BlockingEmit {}

impl ObservabilityPort for BlockingEmit {
    fn emit(&self, _event: CommandEvent) -> Result<(), AtmError> {
        let _ = self.entered.send(());
        block_until_released(&self.release);
        Ok(())
    }

    fn query(&self, req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
        NullObservability.query(req)
    }

    fn follow(&self, req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
        NullObservability.follow(req)
    }

    fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
        NullObservability.health()
    }
}

fn workflow_record() -> WorkflowTelemetryRecord {
    WorkflowTelemetryRecord {
        observation: atm_core::WorkflowTelemetryObservation::Incomplete,
        scope_kind: atm_storage::WorkflowScopeKind::new("sprint").expect("kind"),
        scope_id: atm_storage::WorkflowScopeId::new("bd-6").expect("scope"),
        state: atm_storage::WorkflowState::new("opened").expect("state"),
        stage: atm_storage::WorkflowStage::new("dev").expect("stage"),
        transition: atm_storage::WorkflowTransition::new("start").expect("transition"),
        iteration: None,
        start_message_id: atm_storage::AtmMessageId::new(),
        start_timestamp: atm_storage::IsoTimestamp::now(),
        end_message_id: None,
        end_timestamp: None,
        duration_millis: None,
    }
}

async fn hook_started(started: &mut UnboundedReceiver<()>, what: &str) {
    tokio::time::timeout(Duration::from_secs(10), started.recv())
        .await
        .unwrap_or_else(|_| panic!("{what} hook never started"))
        .expect("stalling hook is alive");
}

/// Positive: with every stallable step stalled, the production shutdown
/// sequence gives every step the one deadline fixed at entry, and the logger
/// and exporter waits compute no later one. The forever-stalled recovery
/// sweep ends through that deadline, so every later step starts past it and
/// neither logger flush is attempted.
/// Negative: no elapsed time is compared; the recorded deadlines prove the
/// bound by construction.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_stalled_shutdown_step_shares_one_cumulative_deadline() {
    let stalled = Receiver::start(true).await;
    let (root, observability) = Daemon::bootstrap(endpoint_env(&stalled.endpoint)).await;
    let (started_tx, mut started) = unbounded_channel();
    let selector: Arc<dyn MessageReceivedHookSelector> =
        Arc::new(StallingSelector(StallingHook(started_tx)));
    let mut daemon = Daemon::compose_with_selector(root, observability, None, selector).await;

    for seq in 1..=64 {
        daemon.workers.task_telemetry.try_emit(task_record(seq));
    }
    stalled
        .capture
        .wait(EXPORT_WAIT, "a stalled export", || {
            stalled
                .capture
                .started
                .load(std::sync::atomic::Ordering::SeqCst)
                >= 1
        })
        .await;
    daemon
        .handler
        .write(
            daemon.request("sender", "recipient", "BD6-F1-PEER", None),
            AuthenticatedIngress::Peer,
            RequestDeadline::after(Duration::from_secs(10)),
        )
        .await
        .expect("a peer write is acknowledged before its detached hook");
    hook_started(&mut started, "detached peer").await;
    let client = Arc::clone(&daemon.client);
    let request = daemon.request("sender", "recipient", "BD6-F1-LOCAL", None);
    let in_flight = tokio::spawn(async move {
        client
            .execute(ApiRequest::new(RequestEnvelope::Write(Box::new(request))))
            .await
    });
    hook_started(&mut started, "in-flight local").await;

    // Recovery sweep: the sweep and one transition drain never finish. The
    // composed handle it replaces aborts on drop.
    daemon.workers.recovery_sweep = RecoverySweepHandle::stalled_for_test(RuntimeHealth::default());

    // Task and workflow drains: one record each sits in a sink that never
    // answers. The composed runtimes it replaces stop at once.
    let (sink_tx, mut sink_entered) = unbounded_channel();
    let task_telemetry = TaskTelemetryRuntime::start(
        TaskTelemetryConfig {
            emit_timeout: STALL_LIMIT,
            drain_timeout: STALL_LIMIT,
            ..TaskTelemetryConfig::default()
        },
        Arc::new(reporting_stalled_sink(sink_tx.clone())),
    );
    let workflow_telemetry = WorkflowTelemetryRuntime::start(
        WorkflowTelemetryConfig {
            emit_timeout: STALL_LIMIT,
            drain_timeout: STALL_LIMIT,
            ..WorkflowTelemetryConfig::default()
        },
        Arc::new(reporting_stalled_sink(sink_tx)),
    );
    task_telemetry.try_emit(task_record(65));
    workflow_telemetry.try_emit(workflow_record());
    hook_started(&mut sink_entered, "task telemetry sink").await;
    hook_started(&mut sink_entered, "workflow telemetry sink").await;
    std::mem::replace(&mut daemon.workers.task_telemetry, task_telemetry)
        .shutdown(Instant::now())
        .await;
    std::mem::replace(&mut daemon.workers.workflow_telemetry, workflow_telemetry)
        .shutdown(Instant::now())
        .await;

    // Temp sweeper: its first pass blocks in the observability emit, which
    // it makes only with a launch team and identity.
    let (entered, mut emit_entered) = unbounded_channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let _release = Release(release);
    let blocked: Blocked = Arc::new(Mutex::new(blocked));
    let sweep_root = daemon.root.path().join("atm-temp-stalled");
    std::fs::create_dir_all(&sweep_root).expect("sweep root");
    daemon.workers.atm_temp_sweeper = AtmTempSweeperRuntime::start(
        sweep_root,
        SweepConfig {
            interval: Duration::from_secs(3600),
            ttl: Duration::from_secs(3600),
        },
        Arc::new(BlockingEmit {
            entered,
            release: Arc::clone(&blocked),
        }),
        DaemonLaunchIdentity {
            team: Some(super::TEAM.parse().expect("team")),
            identity: Some("atm-daemon".parse().expect("identity")),
        },
    );
    hook_started(&mut emit_entered, "temp sweep emit").await;

    // Retained logger: every flush blocks until released. Earlier steps use
    // the whole deadline, so a flush that honours it is never attempted.
    let probe = Probe::new();
    probe.stall_logger_flush(move || block_until_released(&blocked));

    let stopped = observe(&probe, daemon.shutdown()).await;

    let steps = probe.steps();
    let deadline = assert_one_shutdown_deadline(&steps);
    let sweep = steps
        .iter()
        .find(|step| step.step == "recovery_sweep")
        .expect("recovery sweep step");
    assert!(sweep.returned >= deadline, "{steps:#?}");
    assert_eq!(
        steps
            .iter()
            .filter(|step| step.step.starts_with("logger.flush"))
            .map(|step| step.step)
            .collect::<Vec<_>>(),
        ["logger.flush.skipped", "logger.flush.skipped"],
        "{steps:#?}"
    );
    // The held request ends at its own server budget, inside the drain; the
    // detached hook then gets only what the listener left.
    stopped.expect("the listener drained the held request within its budget");
    // The client task may not have been polled yet on a busy runtime, so await
    // it under a failure-only bound instead of sampling `is_finished()`.
    tokio::time::timeout(Duration::from_secs(60), in_flight)
        .await
        .expect("the listener drain never ended the held request")
        .expect("the held request's client task joins without panicking")
        .expect("the held local write completes once the drain releases it");
    stalled.stop().await;
}

/// Positive: a stalled retained-logger flush waits until the caller's
/// deadline when that comes first, and otherwise until the 1s logger bound
/// from its start: the recorded wait bound is exactly the earlier of the two
/// and the flush returns no earlier than it.
/// Negative: the clock is paused and only this test advances it, after the
/// stalled flush has started, so no deadline can pass before `flush_logger`
/// runs and no elapsed time is compared.
#[tokio::test(start_paused = true)]
async fn a_stalled_logger_flush_ends_at_the_earlier_of_deadline_and_bound() {
    let (_root, observability) =
        Daemon::bootstrap(atm_core::test_support::FakeEnvSource::new([])).await;
    let (entered, mut flush_entered) = unbounded_channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let _release = Release(release);
    let blocked: Blocked = Arc::new(Mutex::new(blocked));
    let probe = Probe::new();
    probe.stall_logger_flush(move || {
        let _ = entered.send(());
        block_until_released(&blocked);
    });

    // A held blocking flush stops the paused clock advancing on its own; each
    // wait ends only when the test advances past its bound.
    let early_by = Duration::from_millis(300);
    let early = Instant::now() + early_by;
    tokio::join!(observe(&probe, observability.flush_logger(early)), async {
        hook_started(&mut flush_entered, "first logger flush").await;
        tokio::time::advance(early_by).await;
    });
    let late = Instant::now() + Duration::from_secs(5);
    tokio::join!(observe(&probe, observability.flush_logger(late)), async {
        hook_started(&mut flush_entered, "second logger flush").await;
        tokio::time::advance(RETAINED_LOG_WRITER_SHUTDOWN_TIMEOUT).await;
    });

    let steps = probe.steps();
    let [first, second] = steps.as_slice() else {
        panic!("two logger flushes: {steps:#?}");
    };
    for (step, deadline) in [(first, early), (second, late)] {
        assert_eq!(step.step, "logger.flush", "{steps:#?}");
        assert_eq!(
            step.deadline,
            deadline.min(step.started + RETAINED_LOG_WRITER_SHUTDOWN_TIMEOUT),
            "{steps:#?}"
        );
        assert!(step.returned >= step.deadline, "{steps:#?}");
    }
    assert_eq!(first.deadline, early, "{steps:#?}");
    assert_eq!(
        second.deadline,
        second.started + RETAINED_LOG_WRITER_SHUTDOWN_TIMEOUT,
        "{steps:#?}"
    );
}
