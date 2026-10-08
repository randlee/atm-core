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

use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use atm_core::api::{ApiRequest, RequestDeadline};
use atm_core::boundary::{
    AsyncMessageReceivedHookEmitter, BuiltInPostSendDispatch, MessageReceivedHookSelector,
    PostSendEmissionPath, RosterEntry, TaskActor,
};
use atm_core::doctor::{DoctorQuery, DoctorReport};
use atm_core::error::AtmError;
use atm_core::observability::{
    AtmTelemetryExportFailure, AtmTelemetryExportHealth, AtmTelemetryExportState,
};
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, SendResponseEnvelope};
use atm_core::send::{SendMessageSource, WriteRequest};
use atm_core::test_support::FakeEnvSource;
use atm_core::types::{AgentName, IsoTimestamp, ModelName, TeamName};
use atm_core::{DaemonApiClient, TaskTelemetryKind, TaskTelemetryRecord};
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
        let workflow_telemetry = assembly.workflow_telemetry.clone();
        let runtime_health = RuntimeHealth::with_owner(std::process::id());
        let (handler, recovery_sweep) = build_replacement_handler(
            assembly,
            ReplacementHandlerConfig {
                observability: Arc::new(observability.clone()),
                selector_factory: |_, _, _, _| {
                    Arc::new(AcceptingSelector(AcceptingHook))
                        as Arc<dyn MessageReceivedHookSelector>
                },
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
                workflow_telemetry,
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

    /// The daemon's own `doctor --json` report, through its API.
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
        serde_json::to_value::<&DoctorReport>(&*report).expect("doctor report json")
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

fn export_health(doctor: &serde_json::Value) -> AtmTelemetryExportHealth {
    serde_json::from_value(doctor["observability"]["export"].clone())
        .expect("doctor JSON carries export health")
}

fn sent_message_id(response: ResponseEnvelope) -> atm_core::schema::AtmMessageId {
    let ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) = response else {
        panic!("task write must send: {response:?}")
    };
    outcome.message_id
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

/// Positive: an absent endpoint composes no SDK provider, leaves both task
/// and workflow runtimes disabled and reports `Inert`.
/// Negative: task writes still succeed and nothing is admitted for export.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
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

fn assert_export_remediation(doctor: &serde_json::Value) {
    let findings = doctor["findings"].as_array().expect("doctor findings");
    assert!(
        findings.iter().any(|finding| finding["severity"] != "info"
            && finding["remediation"]
                .as_str()
                .is_some_and(|text| text.contains("ATM_OTEL_ENDPOINT"))),
        "doctor raises export failure above info: {doctor:#}"
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
        let started = Instant::now();
        daemon
            .shutdown()
            .await
            .expect("shutdown result is the listener's");
        assert!(
            started.elapsed() <= Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    });
}

/// Positive: a collector that accepts connections but never answers leaves
/// task results unchanged, and daemon shutdown with a stalled exporter still
/// returns within the clean-stop SLO, retaining the terminal export failure.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
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
    let started = Instant::now();
    daemon
        .shutdown()
        .await
        .expect("shutdown result is the listener's");
    let elapsed = started.elapsed();
    assert!(elapsed <= Duration::from_secs(5), "{elapsed:?}");
    // The SDK's own 400ms export timeout or the 1s wait bound ends the step;
    // either way the terminal failure is retained.
    let health = observability.export_health_for_test();
    assert!(health.last_failure.is_some(), "{health:?}");
    assert_ne!(health.state, AtmTelemetryExportState::Healthy, "{health:?}");
    stalled.stop().await;
}

/// Positive: concurrent and cancelled `shutdown_export` callers share one
/// outcome; every caller returns by its own deadline even when the
/// collector never answers, and a later caller sees the retained failure.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_and_cancelled_export_shutdown_obey_their_deadlines() {
    let stalled = Receiver::start(true).await;
    let root = tempfile::tempdir().expect("root");
    let observability =
        DaemonObservability::bootstrap_from(&endpoint_env(&stalled.endpoint), root.path().into())
            .await
            .expect("bootstrap");
    let (task, _workflow) = observability.take_telemetry_setups();
    let task = task.expect("configured task setup");
    let runtime = atm_runtime::TaskTelemetryRuntime::start(task.config, task.sink);
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

    // The first caller is cancelled right after it starts the shared work;
    // the outcome survives it.
    let cancelled = tokio::time::timeout(
        Duration::from_millis(1),
        observability.shutdown_export(Instant::now() + Duration::from_secs(5)),
    )
    .await;
    assert!(
        cancelled.is_err(),
        "the stalled export outlives the first caller"
    );
    let started = Instant::now();
    let short = Instant::now() + Duration::from_millis(200);
    let long = Instant::now() + Duration::from_secs(3);
    let (a, b) = (observability.clone(), observability.clone());
    let short_elapsed = tokio::spawn(async move {
        a.shutdown_export(short).await;
        started.elapsed()
    });
    let long_elapsed = tokio::spawn(async move {
        b.shutdown_export(long).await;
        started.elapsed()
    });
    let short_elapsed = short_elapsed.await.unwrap();
    let long_elapsed = long_elapsed.await.unwrap();
    assert!(
        short_elapsed < Duration::from_millis(400),
        "{short_elapsed:?}"
    );
    // The exporter step is bounded by 1s from its first caller.
    assert!(
        long_elapsed < Duration::from_millis(1500),
        "{long_elapsed:?}"
    );
    // The SDK's own 400ms export timeout or the 1s wait bound ends the step;
    // either way the terminal failure is retained.
    let health = observability.export_health_for_test();
    assert!(health.last_failure.is_some(), "{health:?}");
    assert_ne!(health.state, AtmTelemetryExportState::Healthy, "{health:?}");
    let again = Instant::now();
    observability
        .shutdown_export(Instant::now() + Duration::from_secs(5))
        .await;
    assert!(
        again.elapsed() < Duration::from_millis(100),
        "outcome is stored"
    );
    stalled.stop().await;
}
