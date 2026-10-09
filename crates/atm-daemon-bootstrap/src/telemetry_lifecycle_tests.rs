//! BD6 daemon lifecycle export proofs.
//!
//! Each test composes the production daemon path (`DaemonObservability`
//! bootstrap, `compose_daemon_assembly`, `build_replacement_handler`, the
//! maintained HTTP runtime and `shutdown_replacement_daemon`) over an isolated
//! SQLite database and an in-process OTLP gRPC collector. Requests arrive over
//! the daemon's capability-authenticated loopback listener, as from `atm`. Two seams are test-owned: the
//! received-hook selector accepts every prompt (so a real prompt handoff is
//! recorded without a tmux pane), and Herdr is the trait-boundary fake.
#![cfg(test)]

mod exit;
mod queue_wake;
pub(crate) mod receiver;
mod shutdown_loss;
mod stalled_shutdown;

use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use atm_core::api::{ApiRequest, RequestDeadline};
use atm_core::boundary::{
    AsyncMessageReceivedHookEmitter, BuiltInNudgeTemplateKind, BuiltInPostSendDispatch,
    MessageReceivedHookSelector, PostSendEmissionPath, PromptTrigger, RosterEntry, TaskActor,
};
use atm_core::doctor::{DoctorQuery, DoctorReport};
use atm_core::error::AtmError;
use atm_core::observability::{
    AtmTelemetryExportFailure, AtmTelemetryExportHealth, AtmTelemetryExportState,
};
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, SendResponseEnvelope};
use atm_core::send::{SendMessageSource, TemplateSendSource, WriteRequest};
use atm_core::test_support::FakeEnvSource;
use atm_core::types::{AgentName, IsoTimestamp, ModelName, TeamName};
use atm_core::{DaemonApiClient, TaskHandoffFacts, TaskTelemetryKind, TaskTelemetryRecord};
use atm_core::{LocalServiceRuntime, SweepConfig};
use atm_http_runtime::{
    DirectPeerTcpConfig, LoopbackTcpConfig, PeerPoolConfig, RuntimeHealth, StorageAndNudgeRouter,
};
use atm_storage::{RosterHarness, RosterMemberKind, RosterSnapshot, TaskCloseOutcome, TaskOp};
use atm_storage_rusqlite::SqliteStorageFactory;
use opentelemetry_proto::tonic::common::v1::KeyValue;
use opentelemetry_proto::tonic::common::v1::any_value::Value as AnyValue;
use opentelemetry_proto::tonic::metrics::v1::metric::Data;
use opentelemetry_proto::tonic::metrics::v1::number_data_point::Value as Number;
use receiver::{Capture, Receiver};
use serde_json::Map;
use tokio::time::Instant;

use super::atm_temp_sweeper_runtime::AtmTempSweeperRuntime;
use super::daemon_observability::DaemonObservability;
use super::shutdown_probe::{Probe, ShutdownStep, observe};
use super::{
    DaemonLaunchIdentity, DaemonWorkers, ReplacementHandlerConfig, SelectedPeerAdapterSelection,
    build_replacement_handler, compose_daemon_assembly,
    replacement_runtime_config_with_direct_peer, shutdown_replacement_daemon,
    start_replacement_runtime_for_test,
};

const TEAM: &str = "bd6-team";
/// First scheduled export is at 2x the 1s SDK interval; leave load headroom.
const EXPORT_WAIT: Duration = Duration::from_secs(15);

/// Accepts every prompt so the router records a real prompt handoff.
struct AcceptingHook;

impl atm_core::boundary::sealed::Sealed for AcceptingHook {}

impl AsyncMessageReceivedHookEmitter for AcceptingHook {
    fn emit_received_message(
        &self,
        _dispatch: BuiltInPostSendDispatch,
        _deadline: RequestDeadline,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<PostSendEmissionPath, AtmError>> + Send + '_>>
    {
        Box::pin(async { Ok(PostSendEmissionPath::QueuePull) })
    }
}

struct AcceptingSelector(AcceptingHook);

impl atm_core::boundary::sealed::Sealed for AcceptingSelector {}

impl MessageReceivedHookSelector for AcceptingSelector {
    fn select_emitter(
        &self,
        _dispatch: &BuiltInPostSendDispatch,
    ) -> Option<&dyn AsyncMessageReceivedHookEmitter> {
        Some(&self.0)
    }
}

fn endpoint_env(endpoint: &str) -> FakeEnvSource {
    FakeEnvSource::new([("ATM_OTEL_ENDPOINT", Some(endpoint))])
}

fn endpoint_env_with_auth(endpoint: &str, auth_header: &str) -> FakeEnvSource {
    FakeEnvSource::new([
        ("ATM_OTEL_ENDPOINT", Some(endpoint)),
        ("ATM_OTEL_AUTH_HEADER", Some(auth_header)),
    ])
}

pub(super) struct Daemon {
    root: tempfile::TempDir,
    pub observability: DaemonObservability,
    running: atm_http_runtime::HttpRuntime<atm_http_runtime::Running>,
    handler: Arc<StorageAndNudgeRouter>,
    workers: DaemonWorkers,
    client: Arc<dyn DaemonApiClient>,
    runtime: LocalServiceRuntime,
    herdr: Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
    /// The composed queue-wake pump, when composed with a test clock.
    pump: Option<atm_http_runtime::HerdrQueueWakePump>,
}

impl Daemon {
    pub async fn start(env: FakeEnvSource) -> Self {
        let (root, observability) = Self::bootstrap(env).await;
        Self::compose(root, observability).await
    }

    /// The daemon's observability bootstrap over an isolated root.
    pub async fn bootstrap(env: FakeEnvSource) -> (tempfile::TempDir, DaemonObservability) {
        let root = tempfile::tempdir().expect("daemon root");
        let observability = DaemonObservability::bootstrap_from(&env, root.path().join("logs"))
            .await
            .expect("daemon observability bootstraps for any export configuration");
        (root, observability)
    }

    /// Composes and starts the daemon on a bootstrapped observability owner.
    pub async fn compose(root: tempfile::TempDir, observability: DaemonObservability) -> Self {
        Self::compose_with(root, observability, None).await
    }

    /// [`Self::compose`]; a `clock` is installed on the composed queue-wake
    /// pump, which is kept, and Herdr worker plus lead members are seeded.
    pub async fn compose_with(
        root: tempfile::TempDir,
        observability: DaemonObservability,
        clock: Option<crate::replacement_handler::queue_wake_probe::Clock>,
    ) -> Self {
        let selector = Arc::new(AcceptingSelector(AcceptingHook));
        Self::compose_with_selector(root, observability, clock, selector).await
    }

    /// [`Self::compose_with`] with the received-hook `selector` the router
    /// and queue workers use.
    pub async fn compose_with_selector(
        root: tempfile::TempDir,
        observability: DaemonObservability,
        clock: Option<crate::replacement_handler::queue_wake_probe::Clock>,
        selector: Arc<dyn MessageReceivedHookSelector>,
    ) -> Self {
        let assembly = compose_daemon_assembly(
            SqliteStorageFactory::at_path(root.path().join("runtime").join("mail.sqlite3")),
            Some(&observability),
        )
        .expect("compose daemon runtime");
        seed_roster(&assembly.service_runtime, clock.is_some());
        let herdr = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
        let probed = clock.is_some();
        if let Some(clock) = clock {
            crate::replacement_handler::queue_wake_probe::arm(clock);
        }
        let runtime = assembly.service_runtime.clone();
        let task_telemetry = assembly.task_telemetry.clone();
        let runtime_health = RuntimeHealth::with_owner(std::process::id());
        let (handler, recovery_sweep) = build_replacement_handler(
            assembly,
            ReplacementHandlerConfig {
                observability: Arc::new(observability.clone()),
                selector_factory: move |_, _, _, _| selector,
                daemon_launch_identity: DaemonLaunchIdentity::default(),
                peer_wire_mode: atm_core::peer_wire::PeerWireMode::plaintext_test(),
                peer_adapter_selection: SelectedPeerAdapterSelection {
                    adapter: None,
                    pool_config: PeerPoolConfig::default(),
                },
                runtime_health: runtime_health.clone(),
                diagnostic_counters: None,
                bare_cli: Default::default(),
                herdr_config: crate::herdr_config::DaemonHerdrConfig::default(),
                herdr_process: Some(herdr.clone()),
                daemon_home: root.path().join("home"),
            },
        )
        .expect("compose the replacement daemon handler");
        // Taken in the same synchronous step that armed it, on this thread.
        let pump = probed.then(|| {
            crate::replacement_handler::queue_wake_probe::take()
                .expect("composition installed the test clock on its pump")
        });
        let instance = ulid::Ulid::new();
        // The loopback client admits only the recorded owning instance.
        std::fs::write(
            root.path()
                .join(atm_core::home::HOST_RUNTIME_OWNER_LOCK_FILE),
            format!("{}:bd6-test-owner:{instance}\n", std::process::id()),
        )
        .expect("write daemon owner record");
        let config = replacement_runtime_config_with_direct_peer(
            LoopbackTcpConfig::new(
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                root.path().join("local-http.json"),
                instance,
            ),
            None,
            DirectPeerTcpConfig::ephemeral_for_test(),
            &None,
            PeerPoolConfig::default(),
        );
        let running = start_replacement_runtime_for_test(config, handler.clone(), runtime_health)
            .await
            .expect("start the daemon runtime");
        // The same capability-authenticated loopback client `atm` uses.
        let client = atm_http_runtime::loopback_tcp_client(
            root.path().join("local-http.json"),
            Duration::from_secs(10),
        )
        .expect("loopback daemon client");
        let atm_temp_sweeper = AtmTempSweeperRuntime::start(
            root.path().join("atm-temp"),
            SweepConfig {
                interval: Duration::from_secs(3600),
                ttl: Duration::from_secs(3600),
            },
            Arc::new(atm_core::observability::NullObservability),
            DaemonLaunchIdentity::default(),
        );
        Self {
            observability: observability.clone(),
            running,
            handler,
            workers: DaemonWorkers {
                task_telemetry,
                recovery_sweep,
                atm_temp_sweeper,
                observability: Some(observability),
            },
            client,
            runtime,
            root,
            herdr,
            pump,
        }
    }

    pub async fn write(&self, request: WriteRequest) -> Result<ResponseEnvelope, AtmError> {
        self.client
            .execute(ApiRequest::new(RequestEnvelope::Write(Box::new(request))))
            .await
            .map(atm_core::ApiResponse::into_inner)
    }

    pub fn request(&self, from: &str, to: &str, task_id: &str, op: Option<TaskOp>) -> WriteRequest {
        let mut request = WriteRequest::new(
            self.root.path().join("home"),
            self.root.path().join("workspace"),
            from.parse::<AgentName>().expect("caller"),
            &format!("{to}@{TEAM}"),
            TEAM.parse().expect("team"),
            SendMessageSource::Inline(format!("bd6 {task_id}")),
            None,
            false,
            Some(task_id.parse().expect("task id")),
            false,
        )
        .expect("task request");
        request.task_op = op;
        request
    }

    /// The daemon's `doctor --json` report, through its API, as the exact
    /// text `atm doctor --json` prints: `print_doctor_result` emits
    /// `serde_json::to_string_pretty(report)` (crates/atm/src/output.rs), so
    /// the report is rendered to that text and parsed back.
    pub async fn doctor_json(&self) -> serde_json::Value {
        let query = DoctorQuery {
            home_dir: self.root.path().join("home"),
            current_dir: self.root.path().join("workspace"),
            ..DoctorQuery::default()
        };
        let response = self
            .client
            .execute(ApiRequest::new(RequestEnvelope::Doctor(query)))
            .await
            .expect("daemon doctor")
            .into_inner();
        let ResponseEnvelope::Doctor(report) = response else {
            panic!("doctor must return a report: {response:?}");
        };
        let printed =
            serde_json::to_string_pretty::<DoctorReport>(&report).expect("doctor --json text");
        serde_json::from_str(&printed).expect("doctor --json text parses")
    }

    /// Every committed ledger row and inserted handoff of `tasks`, as
    /// `(kind, task id, seq, unix nanos)`.
    pub async fn committed(&self, tasks: &[&str]) -> Vec<EventIdentity> {
        let store = self.runtime.task_store().expect("task store");
        let ledger = self.runtime.async_task_ledger_reader().expect("ledger");
        let team: TeamName = TEAM.parse().expect("team");
        let mut identities = Vec::new();
        for task in tasks {
            let task_id = task.parse().expect("task id");
            for row in store
                .list_task_events(&team, &task_id, None)
                .expect("task events")
            {
                identities.push(EventIdentity {
                    kind: row.event.as_str().to_owned(),
                    task_id: row.task_id.to_string(),
                    seq: Some(row.seq.to_string()),
                    at_unix_nanos: unix_nanos(row.at),
                });
            }
            for handoff in ledger
                .list_prompt_handoffs(
                    team.clone(),
                    task_id,
                    atm_storage::ReadDeadline::new(Duration::from_secs(2)).expect("deadline"),
                )
                .await
                .expect("prompt handoffs")
            {
                identities.push(EventIdentity {
                    kind: "prompt_handoff".to_owned(),
                    task_id: handoff.task_id.to_string(),
                    seq: None,
                    at_unix_nanos: unix_nanos(handoff.at),
                });
            }
        }
        identities.sort();
        identities
    }

    pub async fn shutdown(self) -> Result<(), AtmError> {
        shutdown_replacement_daemon(self.running, &self.handler, self.workers).await
    }
}

fn seed_roster(runtime: &LocalServiceRuntime, with_herdr: bool) {
    let team: TeamName = TEAM.parse().expect("team");
    let member = |agent: &str| RosterEntry {
        team_name: team.clone(),
        agent_name: agent.parse().expect("agent"),
        member_kind: RosterMemberKind::Permanent,
        harness: RosterHarness::PythonGraft,
        agent_type: atm_core::schema::AgentType::default(),
        model: ModelName::default(),
        recipient_pane_id: None,
        metadata_json: Map::new(),
    };
    let mut members: Vec<RosterEntry> = ["sender", "recipient", "third"]
        .into_iter()
        .map(member)
        .collect();
    if with_herdr {
        let mut worker = member("worker");
        worker.harness = RosterHarness::CodexCli;
        worker.metadata_json = atm_core::delivery_channel::test_backend_type_metadata("herdr");
        let mut lead = member("lead");
        lead.agent_type = atm_storage::AgentType::Lead;
        members.extend([worker, lead]);
    }
    runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: team.clone(),
            members,
            refreshed_at: None,
        })
        .expect("seed roster");
}

fn unix_nanos(at: atm_core::types::IsoTimestamp) -> u64 {
    u64::try_from(
        at.into_inner()
            .timestamp_nanos_opt()
            .expect("timestamp in range"),
    )
    .expect("post-epoch timestamp")
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct EventIdentity {
    kind: String,
    task_id: String,
    seq: Option<String>,
    at_unix_nanos: u64,
}

/// A synthetic assignment record for queue-pressure tests.
fn task_record(seq: u64) -> TaskTelemetryRecord {
    TaskTelemetryRecord {
        kind: TaskTelemetryKind::Assigned,
        team: TEAM.parse().expect("team"),
        task_id: "BD6-Q".parse().expect("task id"),
        assignee: "recipient".parse().expect("agent"),
        actor: TaskActor::Daemon,
        seq: Some(seq),
        at: "2026-10-08T00:00:00Z"
            .parse::<IsoTimestamp>()
            .expect("timestamp"),
        from_state: None,
        to_state: None,
        close_outcome: None,
        message_id: None,
        reminder_outcome: None,
        marker: None,
        handoff: None,
    }
}

fn attribute(attributes: &[KeyValue], key: &str) -> Option<String> {
    attributes
        .iter()
        .find(|attribute| attribute.key == key)
        .and_then(|attribute| attribute.value.as_ref())
        .and_then(|value| match &value.value {
            Some(AnyValue::StringValue(text)) => Some(text.clone()),
            Some(AnyValue::IntValue(number)) => Some(number.to_string()),
            _ => None,
        })
}

/// Exported `atm.task.event` spans, with start == end == the durable `at`.
fn exported_events(capture: &Capture) -> Vec<EventIdentity> {
    let mut events: Vec<EventIdentity> = capture
        .spans
        .lock()
        .unwrap()
        .iter()
        .filter(|span| span.name == "atm.task.event")
        .map(|span| {
            assert_eq!(span.start_time_unix_nano, span.end_time_unix_nano);
            EventIdentity {
                kind: attribute(&span.attributes, "atm.task.kind").expect("kind"),
                task_id: attribute(&span.attributes, "atm.task.id").expect("task id"),
                seq: attribute(&span.attributes, "atm.task.seq"),
                at_unix_nanos: span.start_time_unix_nano,
            }
        })
        .collect();
    events.sort();
    events
}

/// Latest cumulative `atm.task.events` value per `kind`.
fn exported_counts(capture: &Capture) -> BTreeMap<String, i64> {
    let mut counts = BTreeMap::new();
    for metric in capture.metrics.lock().unwrap().iter() {
        let (true, Some(Data::Sum(sum))) = (metric.name == "atm.task.events", &metric.data) else {
            continue;
        };
        for point in &sum.data_points {
            if let (Some(kind), Some(Number::AsInt(value))) =
                (attribute(&point.attributes, "kind"), point.value)
            {
                let entry = counts.entry(kind).or_insert(0);
                *entry = value.max(*entry);
            }
        }
    }
    counts
}

fn histogram_count(capture: &Capture, name: &str) -> u64 {
    capture
        .metrics
        .lock()
        .unwrap()
        .iter()
        .filter(|metric| metric.name == name)
        .filter_map(|metric| match &metric.data {
            Some(Data::Histogram(histogram)) => {
                histogram.data_points.iter().map(|point| point.count).max()
            }
            _ => None,
        })
        .max()
        .unwrap_or(0)
}

/// Returns the wire-format debug rendering of every payload the in-process
/// collector accepted. This deliberately covers span attributes/events, log
/// bodies/attributes, and metric data points instead of only task spans.
fn captured_export_text(capture: &Capture) -> String {
    format!(
        "{:?}\n{:?}\n{:?}",
        capture.spans.lock().unwrap(),
        capture.logs.lock().unwrap(),
        capture.metrics.lock().unwrap(),
    )
}

fn assert_export_excludes(capture: &Capture, sentinels: &[&str]) {
    let exported = captured_export_text(capture);
    for sentinel in sentinels {
        assert!(
            !exported.contains(sentinel),
            "collector export leaked sentinel {sentinel:?}: {exported}"
        );
    }
}

fn prompt_handoff_record(task_id: &str) -> TaskTelemetryRecord {
    TaskTelemetryRecord {
        kind: TaskTelemetryKind::PromptHandoff,
        team: TEAM.parse().expect("team"),
        task_id: task_id.parse().expect("task id"),
        assignee: "recipient".parse().expect("assignee"),
        actor: TaskActor::Daemon,
        seq: None,
        at: "2026-10-08T00:00:00Z".parse().expect("timestamp"),
        from_state: None,
        to_state: None,
        close_outcome: None,
        message_id: None,
        reminder_outcome: None,
        marker: None,
        handoff: Some(TaskHandoffFacts {
            attempt: 1,
            trigger: PromptTrigger::TaskPass,
            template_kind: BuiltInNudgeTemplateKind::TaskReminder,
        }),
    }
}

fn export_health(doctor: &serde_json::Value) -> AtmTelemetryExportHealth {
    serde_json::from_value(doctor["observability"]["export"].clone())
        .expect("doctor JSON carries export health")
}

/// The daemon shutdown steps, in call order, each given the shared deadline.
const SHUTDOWN_STEPS: [&str; 8] = [
    "entry",
    "listener",
    "recovery_sweep",
    "peer_connections",
    "task_telemetry",
    "atm_temp_sweeper",
    "export",
    "timeline_flush_worker",
];

/// Proves, from the recorded steps, that shutdown met one shared deadline:
/// [`assert_shutdown_deadline_passed_through`], and every top-level step
/// returned at or before that deadline, with no slack. A step that ran on a
/// fresh budget past the deadline fails here even when it was handed the
/// shared deadline. Returns the shared deadline.
fn assert_one_shutdown_deadline(steps: &[ShutdownStep]) -> Instant {
    let deadline = assert_shutdown_deadline_passed_through(steps);
    for step in steps
        .iter()
        .filter(|step| SHUTDOWN_STEPS.contains(&step.step))
    {
        assert!(
            step.returned <= deadline,
            "step {} returned after the shared deadline: {steps:#?}",
            step.step
        );
    }
    deadline
}

/// Proves the shutdown deadline is passed through unchanged, from the
/// recorded steps: it is fixed once at entry `REPLACEMENT_DRAIN_DEADLINE`
/// ahead, every step receives exactly that deadline and starts only after the
/// previous one returned, and the logger and exporter waits compute bounds no
/// later than it and no later than their own 1 s bound from their start. It
/// does not prove a step returned in time. Only a shutdown whose stalled step
/// is ended by the shared deadline itself calls it alone, since that step
/// returns after the deadline by its timer's wake latency; every other caller
/// uses [`assert_one_shutdown_deadline`]. Returns the shared deadline.
fn assert_shutdown_deadline_passed_through(steps: &[ShutdownStep]) -> Instant {
    let entry = steps.first().expect("the shutdown recorded its entry");
    assert_eq!(entry.step, "entry", "{steps:#?}");
    assert_eq!(
        entry.deadline,
        entry.started + super::REPLACEMENT_DRAIN_DEADLINE,
        "{steps:#?}"
    );
    let called: Vec<&ShutdownStep> = steps
        .iter()
        .filter(|step| SHUTDOWN_STEPS.contains(&step.step))
        .collect();
    assert_eq!(
        called.iter().map(|step| step.step).collect::<Vec<_>>(),
        SHUTDOWN_STEPS,
        "{steps:#?}"
    );
    for pair in called.windows(2) {
        assert_eq!(pair[1].deadline, entry.deadline, "{steps:#?}");
        assert!(pair[1].started >= pair[0].returned, "{steps:#?}");
    }
    for inner in steps
        .iter()
        .filter(|step| !SHUTDOWN_STEPS.contains(&step.step))
    {
        let own = match inner.step {
            "export.providers" => super::daemon_observability::EXPORT_SHUTDOWN_BOUND,
            "logger.flush" | "logger.flush.skipped" => {
                super::daemon_observability::RETAINED_LOG_WRITER_SHUTDOWN_TIMEOUT
            }
            other => panic!("unexpected shutdown step {other}: {steps:#?}"),
        };
        assert_eq!(
            inner.deadline,
            entry.deadline.min(inner.started + own),
            "{steps:#?}"
        );
    }
    entry.deadline
}

/// Synthetic steps for one shutdown that meets its deadline: the entry fixes
/// it, then each top-level step runs 1ms after the previous one returned.
fn steps_meeting_the_deadline() -> Vec<ShutdownStep> {
    let entry = Instant::now();
    let deadline = entry + super::REPLACEMENT_DRAIN_DEADLINE;
    let mut at = entry;
    SHUTDOWN_STEPS
        .iter()
        .map(|&step| {
            let started = at;
            at = started + Duration::from_millis(1);
            ShutdownStep {
                step,
                started,
                deadline,
                returned: if step == "entry" { started } else { at },
            }
        })
        .collect()
}

/// Control: a shutdown whose steps all returned by the shared deadline passes.
#[test]
fn one_shutdown_deadline_accepts_steps_that_returned_by_it() {
    let steps = steps_meeting_the_deadline();
    assert_eq!(assert_one_shutdown_deadline(&steps), steps[0].deadline);
}

/// Negative: a top-level step handed the shared deadline but run on a fresh
/// 20s budget returns past the deadline, and the assertion fails, although
/// every label and the step order are still correct.
#[test]
#[should_panic(expected = "step task_telemetry returned after the shared deadline")]
fn one_shutdown_deadline_rejects_a_step_on_a_fresh_budget() {
    let mut steps = steps_meeting_the_deadline();
    let fresh = Duration::from_secs(20);
    let index = steps
        .iter()
        .position(|step| step.step == "task_telemetry")
        .expect("task telemetry step");
    let shift = steps[index].started + fresh - steps[index].returned;
    steps[index].returned += shift;
    for later in &mut steps[index + 1..] {
        later.started += shift;
        later.returned += shift;
    }
    assert_shutdown_deadline_passed_through(&steps);
    assert_one_shutdown_deadline(&steps);
}

fn sent_message_id(response: ResponseEnvelope) -> atm_core::schema::AtmMessageId {
    let ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) = response else {
        panic!("task write must send: {response:?}")
    };
    outcome.message_id
}

/// Positive: composition consumes the telemetry setups of the owner it is
/// handed. Negative: another bootstrapped owner in the same process keeps its
/// setups, so no process-wide owner stands in for the supplied one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn composition_uses_only_the_supplied_observability_owner() {
    let (_other_root, other) = Daemon::bootstrap(endpoint_env("http://127.0.0.1:9")).await;
    let (root, supplied) = Daemon::bootstrap(endpoint_env("http://127.0.0.1:9")).await;

    let _assembly = compose_daemon_assembly(
        SqliteStorageFactory::at_path(root.path().join("runtime").join("mail.sqlite3")),
        Some(&supplied),
    )
    .expect("compose daemon runtime");

    assert!(
        supplied.take_telemetry_setup().is_none(),
        "composition took the supplied owner's setup"
    );
    assert!(
        other.take_telemetry_setup().is_some(),
        "an owner composition was not handed keeps its setup"
    );
}

/// Positive: every router producer reachable over the daemon API (assign,
/// reassign, start, terminal close, reopen, committed rejection audit and the
/// newly inserted prompt handoff) reaches the collector as exactly its
/// durable identity, kind, seq and timestamp, together with the counter and
/// both histograms, while the daemon is still serving.
/// Negative: the acknowledgement adds neither a ledger row nor a span.
/// Reminder, lead notification and reminder reset come from the composed
/// queue-wake pump and are proven in `queue_wake.rs`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::parallel(slo)]
async fn running_daemon_exports_each_committed_task_event_to_the_collector() {
    let receiver = Receiver::start(false).await;
    let daemon = Daemon::start(endpoint_env(&receiver.endpoint)).await;
    let health = export_health(&daemon.doctor_json().await);
    assert_eq!(health.state, AtmTelemetryExportState::Healthy);
    assert_eq!(health.endpoint.as_deref(), Some(receiver.endpoint.as_str()));

    daemon
        .write(daemon.request("sender", "recipient", "BD6-T1", None))
        .await
        .expect("assign");
    daemon
        .write(daemon.request("sender", "third", "BD6-T1", None))
        .await
        .expect("reassign");
    daemon
        .write(daemon.request("third", "sender", "BD6-T1", Some(TaskOp::Start)))
        .await
        .expect("start");
    let close = || TaskOp::Close {
        outcome: TaskCloseOutcome::Completed,
        reason: None,
    };
    daemon
        .write(daemon.request("third", "sender", "BD6-T1", Some(close())))
        .await
        .expect("close");
    daemon
        .write(daemon.request("sender", "third", "BD6-T1", None))
        .await
        .expect("reopen");
    let rejected = daemon
        .write(daemon.request("third", "sender", "BD6-MISSING", Some(close())))
        .await;
    let (Err(rejected) | Ok(ResponseEnvelope::Error(rejected))) = rejected else {
        panic!("closing an unknown task is refused: {rejected:?}");
    };
    assert_eq!(rejected.code(), atm_core::error::AtmErrorCode::TaskNotFound);
    let mut acked = daemon.request("sender", "recipient", "BD6-T2", None);
    acked.requires_ack = true;
    let message_id = sent_message_id(daemon.write(acked).await.expect("assign T2"));
    let before_ack = daemon.committed(&["BD6-T2"]).await;
    let acknowledgement = atm_core::ack::AckRequest {
        home_dir: daemon.root.path().join("home"),
        current_dir: daemon.root.path().join("workspace"),
        caller_identity: "recipient".parse().expect("recipient"),
        caller_chat_id: None,
        caller_team: TEAM.parse().expect("team"),
        activity_observation: None,
        message_id,
        reply_body: "on it".to_owned(),
    }
    .into_write_request();
    daemon.write(acknowledgement).await.expect("acknowledge T2");
    assert_eq!(daemon.committed(&["BD6-T2"]).await, before_ack);

    let tasks = ["BD6-T1", "BD6-MISSING", "BD6-T2"];
    let expected = daemon.committed(&tasks).await;
    let kinds: BTreeSet<&str> = expected.iter().map(|event| event.kind.as_str()).collect();
    for kind in [
        "assigned",
        "reassigned",
        "started",
        "completed",
        "reopened",
        "rejected",
        "prompt_handoff",
    ] {
        assert!(kinds.contains(kind), "ledger lacks {kind}: {expected:?}");
    }
    let mut expected_counts = BTreeMap::new();
    for event in &expected {
        *expected_counts.entry(event.kind.clone()).or_insert(0_i64) += 1;
    }
    receiver
        .capture
        .wait(EXPORT_WAIT, "every committed task event span", || {
            exported_events(&receiver.capture) == expected
        })
        .await;
    receiver
        .capture
        .wait(EXPORT_WAIT, "the task counter and both histograms", || {
            exported_counts(&receiver.capture) == expected_counts
                && histogram_count(&receiver.capture, "atm.task.time_to_start_ms") >= 1
                && histogram_count(&receiver.capture, "atm.task.time_to_close_ms") >= 1
        })
        .await;
    // Still serving: the export happened before shutdown began.
    let health = export_health(&daemon.doctor_json().await);
    assert_eq!(health.state, AtmTelemetryExportState::Healthy);
    assert_eq!(health.emitted, expected.len() as u64);
    assert_eq!(health.last_failure, None);
    assert_eq!(
        exported_events(&receiver.capture),
        expected,
        "no extra spans"
    );

    daemon.shutdown().await.expect("clean daemon shutdown");
    receiver.stop().await;
}

/// A real template write crosses the daemon's HTTP boundary and reaches the
/// live collector, but D1/D5 must project only typed ledger facts. The lower
/// layer pins the record field set in `task_telemetry::record_field_set_is_pinned_and_payload_free`;
/// this daemon proof catches a future composition path that bypasses it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn running_daemon_export_excludes_message_template_and_auth_sentinels() {
    const BODY: &str = "BD6-F2-MESSAGE-BODY-SENTINEL";
    const VARIABLE: &str = "BD6-F2-TEMPLATE-VARIABLE-SENTINEL";
    const AUTH: &str = "Bearer BD6-F2-AUTH-SENTINEL";

    let receiver = Receiver::start(false).await;
    let daemon = Daemon::start(endpoint_env_with_auth(&receiver.endpoint, AUTH)).await;
    let template_path = daemon.root.path().join("private-message.j2");
    std::fs::write(&template_path, format!("{BODY}: {{{{ private_value }}}}"))
        .expect("write private template fixture");
    let mut request = daemon.request("sender", "recipient", "BD6-REDACT", None);
    request.message_source = SendMessageSource::Template(TemplateSendSource {
        canonical_template_path: std::fs::canonicalize(&template_path)
            .expect("canonical template path"),
        canonical_template_root: std::fs::canonicalize(daemon.root.path())
            .expect("canonical template root"),
        raw_file_bytes: std::fs::read(&template_path).expect("read template fixture"),
        input_defaults: Map::new(),
        var_file_values: Map::new(),
        explicit_values: Map::from_iter([(
            "private_value".to_owned(),
            serde_json::Value::String(VARIABLE.to_owned()),
        )]),
        environment_values: Map::new(),
    });
    daemon.write(request).await.expect("template assignment");

    receiver
        .capture
        .wait(EXPORT_WAIT, "the template assignment export", || {
            exported_events(&receiver.capture)
                .iter()
                .any(|event| event.task_id == "BD6-REDACT" && event.kind == "assigned")
        })
        .await;
    daemon.shutdown().await.expect("clean daemon shutdown");
    // Negative control: each distinctive value was present at daemon ingress;
    // any body, merged-var, or exporter-auth projection makes this fail.
    assert_export_excludes(&receiver.capture, &[BODY, VARIABLE, AUTH]);

    receiver.stop().await;
}

/// The router's SQLite replay proof lives in
/// `storage_and_nudge_router/tests/bd3_task_telemetry.rs`; this composed
/// daemon/exporter check proves the same immutable handoff cannot produce two
/// OTLP task-event spans if it reaches the runtime twice.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn running_daemon_exports_duplicate_handoff_once() {
    let receiver = Receiver::start(false).await;
    let daemon = Daemon::start(endpoint_env(&receiver.endpoint)).await;
    let handoff = prompt_handoff_record("BD6-DUPLICATE-HANDOFF");
    daemon.workers.task_telemetry.try_emit(handoff.clone());
    daemon.workers.task_telemetry.try_emit(handoff);

    receiver
        .capture
        .wait(EXPORT_WAIT, "one deduplicated handoff export", || {
            exported_events(&receiver.capture)
                .iter()
                .filter(|event| {
                    event.task_id == "BD6-DUPLICATE-HANDOFF" && event.kind == "prompt_handoff"
                })
                .count()
                == 1
        })
        .await;
    // Drain the composed runtime before counting so a second queued projection
    // cannot arrive after the first export made the wait condition true.
    daemon.shutdown().await.expect("clean daemon shutdown");
    assert_eq!(
        exported_events(&receiver.capture)
            .iter()
            .filter(|event| {
                event.task_id == "BD6-DUPLICATE-HANDOFF" && event.kind == "prompt_handoff"
            })
            .count(),
        1,
        "a duplicate handoff must not fabricate a second event"
    );

    receiver.stop().await;
}

/// Positive: an assignment that was acked and migrated and then completed
/// records exactly one time-to-close sample, measured from its assignment.
/// Negative: acked and migrated are state-neutral facts, so no time-to-start
/// sample is invented when the task never started.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn running_daemon_export_has_close_duration_but_no_start_for_acked_or_migrated() {
    let receiver = Receiver::start(false).await;
    let daemon = Daemon::start(endpoint_env(&receiver.endpoint)).await;
    let mut assigned = task_record(1);
    assigned.task_id = "BD6-STATE-NEUTRAL".parse().expect("task id");
    let mut acked = assigned.clone();
    acked.kind = TaskTelemetryKind::Acked;
    acked.seq = Some(2);
    let mut migrated = assigned.clone();
    migrated.kind = TaskTelemetryKind::Migrated;
    migrated.seq = Some(3);
    let mut completed = assigned.clone();
    completed.kind = TaskTelemetryKind::Completed;
    completed.seq = Some(4);
    completed.at = "2026-10-08T00:00:05Z"
        .parse::<IsoTimestamp>()
        .expect("timestamp");
    for record in [assigned, acked, migrated, completed] {
        daemon.workers.task_telemetry.try_emit(record);
    }

    receiver
        .capture
        .wait(
            EXPORT_WAIT,
            "the four exports and the close duration",
            || {
                let events = exported_events(&receiver.capture);
                ["assigned", "acked", "migrated", "completed"]
                    .into_iter()
                    .all(|kind| {
                        events
                            .iter()
                            .any(|event| event.task_id == "BD6-STATE-NEUTRAL" && event.kind == kind)
                    })
                    && histogram_count(&receiver.capture, "atm.task.time_to_close_ms") >= 1
            },
        )
        .await;
    daemon.shutdown().await.expect("clean daemon shutdown");
    assert_eq!(
        histogram_count(&receiver.capture, "atm.task.time_to_start_ms"),
        0,
        "Acked/Migrated must not invent a start duration"
    );
    assert_eq!(
        histogram_count(&receiver.capture, "atm.task.time_to_close_ms"),
        1,
        "the completed assignment records exactly one close duration"
    );

    receiver.stop().await;
}

/// Positive: an absent endpoint composes no SDK provider, leaves the task
/// telemetry runtime disabled and reports `Inert`.
/// Negative: task writes still succeed and nothing is admitted for export.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::parallel(slo)]
async fn absent_endpoint_composes_no_exporter_and_reports_inert() {
    let daemon = Daemon::start(FakeEnvSource::empty()).await;
    assert!(!daemon.observability.export_providers_present_for_test());
    daemon
        .write(daemon.request("sender", "recipient", "BD6-INERT", None))
        .await
        .expect("assignment succeeds without an exporter");
    let doctor = daemon.doctor_json().await;
    let health = export_health(&doctor);
    assert_eq!(health.state, AtmTelemetryExportState::Inert);
    assert_eq!((health.endpoint, health.emitted), (None, 0));
    assert_eq!(
        daemon
            .workers
            .task_telemetry
            .diagnostics()
            .snapshot()
            .emitted,
        0
    );
    daemon.shutdown().await.expect("clean daemon shutdown");
}

/// Positive: invalid export configuration (a non-gRPC protocol, a credential
/// in the endpoint, an unknown log destination) keeps the daemon serving with
/// file logging and `Unavailable`/`ConfigInvalid` health in doctor JSON.
/// Negative: the rejected values never appear in the doctor report.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::parallel(slo)]
async fn invalid_export_configuration_keeps_the_daemon_operational() {
    let cases = [
        FakeEnvSource::new([
            ("ATM_OTEL_ENDPOINT", Some("http://127.0.0.1:4318")),
            ("ATM_OTEL_PROTOCOL", Some("http/json")),
        ]),
        FakeEnvSource::new([(
            "ATM_OTEL_ENDPOINT",
            Some("http://user:s3cret@127.0.0.1:4317"),
        )]),
        FakeEnvSource::new([("ATM_LOG_DESTINATION", Some("s3cret-sink"))]),
    ];
    for env in cases {
        let daemon = Daemon::start(env).await;
        assert!(!daemon.observability.export_providers_present_for_test());
        daemon
            .write(daemon.request("sender", "recipient", "BD6-INVALID", None))
            .await
            .expect("assignment succeeds with invalid export configuration");
        let doctor = daemon.doctor_json().await;
        let health = export_health(&doctor);
        assert_eq!(health.state, AtmTelemetryExportState::Unavailable);
        assert_eq!(
            health.last_failure,
            Some(AtmTelemetryExportFailure::ConfigInvalid)
        );
        assert_eq!(health.endpoint, None);
        let text = doctor.to_string();
        assert!(!text.contains("s3cret") && !text.contains("http/json"));
        assert_export_remediation(&doctor);
        daemon.shutdown().await.expect("clean daemon shutdown");
    }
}

/// Positive: a configuration that parses (it checks scheme and host, not full
/// URI syntax) but that the SDK transport rejects during setup leaves
/// the daemon serving with no exporter and reports `Unavailable`/
/// `ConfigInvalid` health in doctor JSON, with the same remediation finding.
/// Negative: this is the `setup_telemetry` failure branch, not the
/// configuration-rejected one: the endpoint stays reported, and the daemon's
/// task writes still succeed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::parallel(slo)]
async fn valid_configuration_with_failed_sdk_setup_reports_config_invalid() {
    let endpoint = "http://bad<host:4317";
    assert!(
        atm_core::task_telemetry::TelemetryExportConfig::from_env(&endpoint_env(endpoint))
            .is_ok_and(|config| config.is_some()),
        "the configuration itself is valid"
    );
    let daemon = Daemon::start(endpoint_env(endpoint)).await;
    assert!(!daemon.observability.export_providers_present_for_test());
    daemon
        .write(daemon.request("sender", "recipient", "BD6-SETUP-FAIL", None))
        .await
        .expect("assignment succeeds when SDK setup failed");
    let doctor = daemon.doctor_json().await;
    let health = export_health(&doctor);
    assert_eq!(health.state, AtmTelemetryExportState::Unavailable);
    assert_eq!(
        health.last_failure,
        Some(AtmTelemetryExportFailure::ConfigInvalid)
    );
    assert_eq!(health.endpoint.as_deref(), Some(endpoint));
    assert_eq!(health.emitted, 0);
    assert_export_remediation(&doctor);
    daemon.shutdown().await.expect("clean daemon shutdown");
}

/// Exactly one export-related finding, raised above info.
fn assert_export_remediation(doctor: &serde_json::Value) {
    let findings = doctor["findings"].as_array().expect("doctor findings");
    let export: Vec<&serde_json::Value> = findings
        .iter()
        .filter(|finding| {
            finding["remediation"]
                .as_str()
                .is_some_and(|text| text.contains("ATM_OTEL_ENDPOINT"))
        })
        .collect();
    assert_eq!(
        export.len(),
        1,
        "doctor raises exactly one export finding: {doctor:#}"
    );
    assert_ne!(
        export[0]["severity"], "info",
        "export failure is above info: {doctor:#}"
    );
}

/// A task runtime whose sink stalls until the returned release is dropped,
/// with a full production queue, so a burst leaves real `dropped_full` counts.
/// Attach it before composing the daemon, and drop the release before
/// shutting the runtime down.
async fn lossy_task_runtime() -> (
    atm_runtime::TaskTelemetryRuntime,
    atm_runtime_test_support::StallRelease,
) {
    let (entered_tx, mut entered) = tokio::sync::mpsc::unbounded_channel();
    let (sink, release) =
        atm_runtime_test_support::StalledTaskTelemetrySink::reporting(move || {
            let _ = entered_tx.send(());
        });
    let runtime = atm_runtime::TaskTelemetryRuntime::start(Arc::new(sink));
    runtime.try_emit(task_record(1));
    tokio::time::timeout(EXPORT_WAIT, entered.recv())
        .await
        .expect("the worker never reached the stalled sink")
        .expect("the stalled sink is alive");
    // One record is in the stalled sink and the queue fills; the rest drop
    // full, synchronously, so no counter moves after this point.
    for seq in 2..=(2 + atm_runtime::TASK_TELEMETRY_QUEUE_CAPACITY as u64 + 3) {
        runtime.try_emit(task_record(seq));
    }
    assert_eq!(runtime.diagnostics().snapshot().dropped_full, 4);
    (runtime, release)
}

/// Doctor reports exactly the runtime's known losses, disjoint per cause.
fn assert_doctor_reports_losses(
    doctor: &serde_json::Value,
    losses: atm_runtime::TaskTelemetryDiagnosticsSnapshot,
) {
    let health = export_health(doctor);
    assert!(losses.dropped_full > 0, "{losses:?}");
    assert_eq!(health.dropped_full, losses.dropped_full);
    assert_eq!(
        health.dropped_timeout, 0,
        "the governed field stays and the synchronous sink never times out"
    );
    assert_eq!(
        (health.dropped_failure, health.dropped_shutdown),
        (0, 0),
        "no loss is invented for an SDK failure or shutdown"
    );
}

/// Polls the daemon's doctor JSON on a fixed cadence until `condition`.
async fn doctor_until(
    daemon: &Daemon,
    what: &str,
    condition: impl Fn(&AtmTelemetryExportHealth) -> bool,
) -> serde_json::Value {
    let mut cadence = tokio::time::interval(Duration::from_millis(100));
    let last = std::sync::Mutex::new(None);
    let reached = tokio::time::timeout(EXPORT_WAIT, async {
        loop {
            cadence.tick().await;
            let doctor = daemon.doctor_json().await;
            let health = export_health(&doctor);
            if condition(&health) {
                return doctor;
            }
            *last.lock().unwrap() = Some(health);
        }
    })
    .await;
    reached.unwrap_or_else(|_| {
        panic!(
            "doctor never reported {what} within {EXPORT_WAIT:?}; last export health {:?}",
            last.lock().unwrap()
        )
    })
}

/// Positive: SDK transport failures against an unreachable collector reach
/// doctor JSON as `Unavailable` through the process-global tracing bridge,
/// while the daemon keeps serving and every task write keeps its result.
/// Negative: no SDK loss quantity is invented (`dropped_failure` stays 0).
/// The scenario runs in a child process because the bridge is process-global
/// in production; a thread-scoped test dispatcher races other tests' scoped
/// dispatchers in tracing's per-callsite interest cache and can miss the SDK
/// failure event entirely.
#[test]
#[serial_test::serial(slo)]
fn unreachable_collector_degrades_health_without_changing_task_results() {
    exit::run_child_scenario(UNREACHABLE_CHILD);
}

const UNREACHABLE_CHILD: &str = "telemetry_lifecycle_tests::unreachable_collector_child";

/// Child half of [`unreachable_collector_degrades_health_without_changing_task_results`];
/// a no-op unless launched by [`exit::run_child_scenario`].
#[test]
fn unreachable_collector_child() {
    if !exit::is_child_scenario(UNREACHABLE_CHILD) {
        return;
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("child runtime");
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let unreachable = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        // Production order: bootstrap, install the global bridge, compose.
        let (root, observability) = Daemon::bootstrap(endpoint_env(&unreachable)).await;
        observability
            .install_tracing_bridge()
            .expect("the child process owns the global tracing bridge");
        let (lossy, _release) = lossy_task_runtime().await;
        observability.attach_runtime_telemetry(lossy.diagnostics());
        let daemon = Daemon::compose(root, observability).await;
        for task in ["BD6-U1", "BD6-U2", "BD6-U3"] {
            sent_message_id(
                daemon
                    .write(daemon.request("sender", "recipient", task, None))
                    .await
                    .expect("assignment result is independent of the collector"),
            );
        }
        let doctor = doctor_until(&daemon, "Unavailable export", |health| {
            health.state == AtmTelemetryExportState::Unavailable
        })
        .await;
        let health = export_health(&doctor);
        assert_eq!(
            health.last_failure,
            Some(AtmTelemetryExportFailure::Unavailable)
        );
        assert_eq!(health.dropped_failure, 0);
        assert_export_remediation(&doctor);
        // Runtime losses and the SDK failure are reported together, disjointly.
        assert_doctor_reports_losses(&doctor, lossy.diagnostics().snapshot());
        let probe = Probe::new();
        observe(&probe, daemon.shutdown())
            .await
            .expect("shutdown result is the listener's");
        // Every step shares the one deadline fixed at shutdown entry and
        // returned by it.
        assert_one_shutdown_deadline(&probe.steps());
    });
    drop(runtime);
    println!("{}", exit::CHILD_SCENARIO_SENTINEL);
}

/// Positive: known runtime losses (a full task queue behind a stalled sink)
/// reach `doctor --json` as `Degraded` with exactly the runtime's
/// `dropped_full`, as one export finding, with no SDK failure and no invented
/// timeout, failure or shutdown loss.
/// Negative: the daemon keeps serving and the task write keeps its result.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn runtime_losses_reach_doctor_json_as_one_degraded_finding() {
    let collector = Receiver::start(false).await;
    let (root, observability) = Daemon::bootstrap(endpoint_env(&collector.endpoint)).await;
    let (lossy, release) = lossy_task_runtime().await;
    observability.attach_runtime_telemetry(lossy.diagnostics());
    let daemon = Daemon::compose(root, observability).await;
    sent_message_id(
        daemon
            .write(daemon.request("sender", "recipient", "BD6-LOSS", None))
            .await
            .expect("assignment result is independent of runtime losses"),
    );
    let doctor = daemon.doctor_json().await;
    let health = export_health(&doctor);
    assert_eq!(
        health.state,
        AtmTelemetryExportState::Degraded,
        "{health:?}"
    );
    assert_eq!(health.last_failure, None, "{health:?}");
    assert_doctor_reports_losses(&doctor, lossy.diagnostics().snapshot());
    assert_export_remediation(&doctor);
    daemon.shutdown().await.expect("clean daemon shutdown");
    drop(release);
    lossy
        .shutdown(Instant::now() + Duration::from_secs(5))
        .await;
    collector.stop().await;
}

/// Positive: a collector that accepts connections but never answers leaves
/// task results unchanged, and daemon shutdown with a stalled exporter gives
/// every step the one deadline fixed at entry, retaining the terminal export
/// failure.
/// Negative: no elapsed time is compared.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial(slo)]
async fn stalled_collector_never_changes_task_results_or_blocks_shutdown() {
    let stalled = Receiver::start(true).await;
    let daemon = Daemon::start(endpoint_env(&stalled.endpoint)).await;
    for task in ["BD6-S1", "BD6-S2", "BD6-S3"] {
        sent_message_id(
            daemon
                .write(daemon.request("sender", "recipient", task, None))
                .await
                .expect("assignment result is independent of the collector"),
        );
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
    let observability = daemon.observability.clone();
    let probe = Probe::new();
    observe(&probe, daemon.shutdown())
        .await
        .expect("shutdown result is the listener's");
    // The stalled export ends on its own 1s bound, so every step still
    // returns by the shared deadline.
    assert_one_shutdown_deadline(&probe.steps());
    // The SDK's own 400ms export timeout or the 1s wait bound ends the step;
    // either way the terminal failure is retained.
    let health = observability.export_health_for_test();
    assert!(health.last_failure.is_some(), "{health:?}");
    assert_ne!(health.state, AtmTelemetryExportState::Healthy, "{health:?}");
    stalled.stop().await;
}

/// A stalled collector with a non-empty export in flight, and the task
/// runtime already stopped, so only the exporter step remains.
async fn stalled_export_in_flight() -> (Receiver, tempfile::TempDir, DaemonObservability) {
    let stalled = Receiver::start(true).await;
    let root = tempfile::tempdir().expect("root");
    let observability =
        DaemonObservability::bootstrap_from(&endpoint_env(&stalled.endpoint), root.path().into())
            .await
            .expect("bootstrap");
    let task = observability
        .take_telemetry_setup()
        .expect("configured task setup");
    let runtime = atm_runtime::TaskTelemetryRuntime::start(task.sink);
    for seq in 1..=64 {
        runtime.try_emit(task_record(seq));
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
    runtime
        .shutdown(Instant::now() + Duration::from_millis(500))
        .await;
    (stalled, root, observability)
}

/// Positive: concurrent and cancelled `shutdown_export` callers share one
/// outcome. Proven by ordering and state, not elapsed time: the test holds
/// the shared step open with an explicit gate, so a caller whose deadline has
/// passed returns while the step is still running, a caller with a long
/// deadline returns only after the release, the first (cancelled) caller's
/// outcome survives, and a later caller finds the stored outcome and
/// completes on its first poll without new work.
/// Negative: no wall-clock margin is asserted and no timer decides an
/// ordering; the long caller's 30s deadline is a failure-only hang bound.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::parallel(slo)]
async fn concurrent_and_cancelled_export_shutdown_share_one_ordered_outcome() {
    let (stalled, _root, observability) = stalled_export_in_flight().await;
    let probe = Probe::new();
    let release = probe.hold_export_shutdown();

    // The first caller is cancelled right after it starts the shared work;
    // the gate keeps the step open, and the outcome survives the caller.
    let cancelled = tokio::time::timeout(
        Duration::from_millis(1),
        observe(
            &probe,
            observability.shutdown_export(Instant::now() + Duration::from_secs(5)),
        ),
    )
    .await;
    assert!(
        cancelled.is_err(),
        "the gated export outlives the first caller"
    );

    let order = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let expired = Instant::now();
    let long = Instant::now() + Duration::from_secs(30);
    let (a, b) = (observability.clone(), observability.clone());
    let (order_a, order_b) = (Arc::clone(&order), Arc::clone(&order));
    let short_done = tokio::spawn(async move {
        a.shutdown_export(expired).await;
        order_a.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    });
    let long_done = tokio::spawn(async move {
        b.shutdown_export(long).await;
        order_b.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    });
    let short_position = short_done.await.expect("short caller");
    assert!(
        !long_done.is_finished(),
        "the expired caller returned while the gated step was still running"
    );
    release.send(()).expect("the shared step awaits the gate");
    let long_position = long_done.await.expect("long caller");
    assert!(
        Instant::now() < long,
        "the long caller returned at its own deadline, not at the step's release"
    );
    assert_eq!(
        (short_position, long_position),
        (0, 1),
        "the expired caller returns before the caller that waits for the shared step"
    );

    // The SDK's own export timeout or the exporter step bound ends the
    // provider shutdowns; either way the terminal failure is retained.
    let health = observability.export_health_for_test();
    assert!(health.last_failure.is_some(), "{health:?}");
    assert_ne!(health.state, AtmTelemetryExportState::Healthy, "{health:?}");

    // A later caller finds the stored outcome: it is ready on its first poll
    // (a zero timeout polls the future before its timer), so no work is done.
    tokio::time::timeout(
        Duration::ZERO,
        observability.shutdown_export(Instant::now() + Duration::from_secs(5)),
    )
    .await
    .expect("a later caller completes on its first poll");
    stalled.stop().await;
}

/// Positive: with the collector stalled, a caller that waits for the shared
/// exporter step returns once the step ends, and the step's wait was bounded
/// by construction at the 1 s exporter bound from its start, well inside the
/// daemon's 5 s clean-stop SLO, never at the caller's later deadline.
/// Negative: no elapsed time is compared; the caller's 30 s deadline is a
/// failure-only hang bound.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial(slo)]
async fn shutdown_export_is_bounded_by_the_clean_stop_slo() {
    let (stalled, _root, observability) = stalled_export_in_flight().await;
    let deadline = Instant::now() + Duration::from_secs(30);
    let probe = Probe::new();
    observe(&probe, observability.shutdown_export(deadline)).await;
    assert!(
        Instant::now() < deadline,
        "the caller returned at its own deadline, not at the step's end"
    );
    let steps = probe.steps();
    let [step] = steps.as_slice() else {
        panic!("one exporter step: {steps:#?}");
    };
    assert_eq!(step.step, "export.providers", "{steps:#?}");
    assert_eq!(
        step.deadline,
        step.started + super::daemon_observability::EXPORT_SHUTDOWN_BOUND,
        "{steps:#?}"
    );
    stalled.stop().await;
}
