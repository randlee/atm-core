#![cfg(test)]

mod bb5_closure;
mod herdr_nudge_invariant;
mod herdr_queue_ephemeral;
mod herdr_queue_no_delivery;

use super::task_pass::runtime_state;
use super::{
    BoundedBlockingBridge, HERDR_MAX_CONSECUTIVE_RELEASES, HERDR_MAX_PROMPTS_PER_TICK,
    HERDR_POLL_INTERVAL_MS, HERDR_REQUEST_BUDGET, HerdrQueueWakePump, ReleasePendingOnDrop,
    RuntimeHealth, herdr_request_deadline, log_herdr_list_failure,
};
use atm_core::LocalServiceRuntime;
use atm_core::ack::{AckRequest, ack_mail_with_runtime};
use atm_core::api::RequestDeadline;
use atm_core::boundary::{
    AsyncMessageReceivedHookEmitter, BuiltInPostSendDispatch, MessageReceivedHookSelector,
    PostSendEmissionPath, PromptTrigger, ReadDeadline, RosterEntry, RosterHarness,
    RosterMemberKind,
};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::observability::NullObservability;
use atm_core::protocol::{RuntimeMemberState, RuntimeObservationAvailability};
use atm_core::schema::AtmMessageId;
use atm_core::send::{NudgeMode, SendMessageSource, WriteRequest, write_mail_with_runtime};
use atm_core::test_support as atm_storage;
use atm_core::types::{IsoTimestamp, ModelName, TaskId, TeamName};
use atm_herdr::{
    AgentSnapshot, HerdrAgentStatus, HerdrListOutcome, HerdrProcessAdapter, HerdrPromptOutcome,
};
use atm_observability::{
    DiagnosticSink, RetainedEvent, RetainedLogPolicy, SinkOffer, TracingBridgeLayer,
    build_retained_logger,
};
use atm_runtime_test_support::open_isolated_sqlite_boundary;
use atm_storage::{RosterSnapshot, TaskRow, TaskState};
use serde_json::json;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;

use tracing_subscriber::prelude::*;

#[derive(Default)]
struct RecordingDiagnosticSink {
    codes: Mutex<Vec<String>>,
}

impl DiagnosticSink for RecordingDiagnosticSink {
    fn offer(&self, event: &RetainedEvent<'_>) -> SinkOffer {
        if let Some(code) = event.code {
            self.codes.lock().expect("codes").push(code.to_owned());
        }
        SinkOffer::Accepted
    }
}

#[tokio::test(start_paused = true)]
async fn blocking_work_is_bounded_by_the_existing_request_deadline() {
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let release = Arc::new(AtomicBool::new(false));
    let worker_release = Arc::clone(&release);
    let bridge = BoundedBlockingBridge::new(
        std::num::NonZeroUsize::new(1).expect("non-zero test bridge capacity"),
        RuntimeHealth::default(),
    );
    let operation = tokio::spawn(async move {
        bridge
            .run(herdr_request_deadline(), move || {
                started_tx.send(()).expect("signal blocking work started");
                while !worker_release.load(Ordering::Acquire) {
                    std::thread::yield_now();
                }
                Ok(())
            })
            .await
    });

    started_rx.await.expect("blocking work starts");
    tokio::time::advance(HERDR_REQUEST_BUDGET).await;
    let error = operation
        .await
        .expect("timeout task joins")
        .expect_err("blocking work must time out");
    release.store(true, Ordering::Release);

    assert_eq!(error.code(), AtmErrorCode::InternalError);
    assert!(error.detail().contains("timed out"));
}

#[test]
fn herdr_list_failure_reaches_the_tracing_bridge_with_its_error_code() {
    let root = tempfile::tempdir().expect("tempdir");
    let logger = Arc::new(
        build_retained_logger(
            "atm",
            &root.path().join("logs"),
            RetainedLogPolicy {
                rotation_max_bytes: 1_024 * 1_024,
                rotation_max_files: 2,
                retention_max_age: Duration::from_secs(60),
                maintenance_cadence: Duration::from_secs(60),
                writer_shutdown_timeout: Duration::from_secs(1),
                maintenance_max_work_per_pass: Some(2),
            },
            None,
        )
        .expect("logger"),
    );
    let bridge = TracingBridgeLayer::new(logger);
    let sink = Arc::new(RecordingDiagnosticSink::default());
    bridge.set_diagnostic_sink(sink.clone());

    tracing::subscriber::with_default(tracing_subscriber::Registry::default().with(bridge), || {
        log_herdr_list_failure(&None, &atm_herdr::HerdrError::ServerNotRunning);
    });

    assert_eq!(
        sink.codes.lock().expect("codes").as_slice(),
        ["ATM_HERDR_UNAVAILABLE"]
    );
}

struct FakeSelector {
    emitter: FakeEmitter,
}

struct FakeEmitter {
    process: Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
}

struct NoEmitterSelector;

impl atm_core::boundary::sealed::Sealed for FakeSelector {}
impl atm_core::boundary::sealed::Sealed for FakeEmitter {}
impl atm_core::boundary::sealed::Sealed for NoEmitterSelector {}

impl MessageReceivedHookSelector for FakeSelector {
    fn select_emitter(
        &self,
        _dispatch: &BuiltInPostSendDispatch,
    ) -> Option<&dyn AsyncMessageReceivedHookEmitter> {
        Some(&self.emitter)
    }
}

impl MessageReceivedHookSelector for NoEmitterSelector {
    fn select_emitter(
        &self,
        _dispatch: &BuiltInPostSendDispatch,
    ) -> Option<&dyn AsyncMessageReceivedHookEmitter> {
        None
    }
}

impl AsyncMessageReceivedHookEmitter for FakeEmitter {
    fn emit_received_message(
        &self,
        dispatch: BuiltInPostSendDispatch,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<PostSendEmissionPath, AtmError>> + Send + '_>> {
        let process = Arc::clone(&self.process);
        Box::pin(async move {
            let target = match dispatch.target {
                atm_core::boundary::PostSendBuiltInTarget::LocalSteer(
                    atm_core::boundary::LocalSteerTarget::Herdr(target),
                ) => target,
                _ => {
                    return Err(AtmError::new(
                        AtmErrorCode::InternalError,
                        "test dispatch was not Herdr",
                    ));
                }
            };
            process
                .prompt(
                    &target.agent,
                    target.session.as_ref(),
                    &target.rendered_nudge,
                    deadline,
                )
                .await
                .map(|HerdrPromptOutcome::Accepted(_)| PostSendEmissionPath::LocalHerdr)
                .map_err(Into::into)
        })
    }
}

fn herdr_member_with_session(team: &TeamName, agent: &str, session: &str) -> RosterEntry {
    RosterEntry {
        team_name: team.clone(),
        agent_name: agent.parse().expect("agent"),
        member_kind: RosterMemberKind::Permanent,
        harness: RosterHarness::CodexCli,
        agent_type: atm_storage::AgentType::default(),
        model: ModelName::default(),
        recipient_pane_id: None,
        metadata_json: {
            let mut metadata = atm_core::delivery_channel::test_backend_type_metadata("herdr");
            metadata.insert("herdrSession".to_owned(), json!(session));
            metadata
        },
    }
}

fn herdr_member(team: &TeamName, agent: &str) -> RosterEntry {
    herdr_member_with_session(team, agent, "aq27-test")
}

fn herdr_member_with_alias(team: &TeamName, agent: &str, alias: &str) -> RosterEntry {
    let mut member = herdr_member(team, agent);
    member
        .metadata_json
        .insert("alias".to_string(), json!(alias));
    member
}

fn queue_message(
    root: &std::path::Path,
    runtime: &LocalServiceRuntime,
    team: &TeamName,
    agent: &str,
) -> AtmMessageId {
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let recipient = format!("{agent}@{team}");
    write_mail_with_runtime(
        WriteRequest::new(
            home.clone(),
            home,
            "sender".parse().expect("sender"),
            &recipient,
            team.clone(),
            SendMessageSource::Inline("AQ2.7 test message".to_owned()),
            None,
            false,
            None,
            false,
        )
        .expect("write request")
        .with_nudge_mode(NudgeMode::Deferred),
        &NullObservability,
        runtime,
    )
    .expect("queue write")
    .persisted_message_id()
}

fn queue_task_message(
    root: &std::path::Path,
    runtime: &LocalServiceRuntime,
    team: &TeamName,
    agent: &str,
    task_id: TaskId,
) -> AtmMessageId {
    queue_task_message_with_nudge(root, runtime, team, agent, task_id, NudgeMode::Deferred)
}

fn queue_task_message_with_nudge(
    root: &std::path::Path,
    runtime: &LocalServiceRuntime,
    team: &TeamName,
    agent: &str,
    task_id: TaskId,
    nudge_mode: NudgeMode,
) -> AtmMessageId {
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let recipient = format!("{agent}@{team}");
    let mut request = WriteRequest::new(
        home.clone(),
        home,
        "sender".parse().expect("sender"),
        &recipient,
        team.clone(),
        SendMessageSource::Inline("AX5 reminder task".to_owned()),
        None,
        false,
        None,
        false,
    )
    .expect("task write request")
    .with_nudge_mode(nudge_mode);
    request.task_id = Some(task_id);
    write_mail_with_runtime(request, &NullObservability, runtime)
        .expect("queue task write")
        .persisted_message_id()
}

fn queue_requires_ack_message(
    root: &std::path::Path,
    runtime: &LocalServiceRuntime,
    team: &TeamName,
    agent: &str,
) -> AtmMessageId {
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let recipient = format!("{agent}@{team}");
    write_mail_with_runtime(
        WriteRequest::new(
            home.clone(),
            home,
            "sender".parse().expect("sender"),
            &recipient,
            team.clone(),
            SendMessageSource::Inline("requires acknowledgement".to_owned()),
            None,
            true,
            None,
            false,
        )
        .expect("requires-ack write request")
        .with_nudge_mode(NudgeMode::Deferred),
        &NullObservability,
        runtime,
    )
    .expect("queue requires-ack message")
    .persisted_message_id()
}

fn immediate_message(
    root: &std::path::Path,
    runtime: &LocalServiceRuntime,
    team: &TeamName,
    agent: &str,
) -> AtmMessageId {
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let request = WriteRequest::new(
        home.clone(),
        home,
        "sender".parse().expect("sender"),
        &format!("{agent}@{team}"),
        team.clone(),
        SendMessageSource::Inline("immediate test message".to_owned()),
        None,
        false,
        None,
        false,
    )
    .expect("immediate write request")
    .with_nudge_mode(NudgeMode::Immediate);
    write_mail_with_runtime(request, &NullObservability, runtime)
        .expect("immediate write")
        .persisted_message_id()
}

fn pending_state(
    root: &std::path::Path,
    key: &atm_core::boundary::MemberKey,
    message_id: AtmMessageId,
) -> (Option<String>, u32) {
    atm_runtime_test_support::inspect_pending_marker_state_for_test(
        root.join("runtime/mail.sqlite3"),
        key.team().as_str(),
        key.agent().as_str(),
        atm_storage::MessageKey::from(message_id).as_str(),
    )
    .expect("read pending marker state")
}

fn acknowledgement_is_pending(
    root: &std::path::Path,
    key: &atm_core::boundary::MemberKey,
    message_id: AtmMessageId,
) -> bool {
    atm_runtime_test_support::inspect_message_ack_state_for_test(
        root.join("runtime/mail.sqlite3"),
        key.team().as_str(),
        key.agent().as_str(),
        atm_storage::MessageKey::from(message_id).as_str(),
    )
    .expect("read acknowledgement state")
}

fn pump_with_clock(
    runtime: LocalServiceRuntime,
    fake: Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
    health: super::RuntimeHealth,
    now: Arc<Mutex<IsoTimestamp>>,
) -> HerdrQueueWakePump {
    let selector = Arc::new(FakeSelector {
        emitter: FakeEmitter {
            process: Arc::clone(&fake),
        },
    });
    pump_with_selector(runtime, fake, health, now, selector)
}

fn pump_with_selector(
    runtime: LocalServiceRuntime,
    fake: Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
    health: super::RuntimeHealth,
    now: Arc<Mutex<IsoTimestamp>>,
    selector: Arc<dyn MessageReceivedHookSelector>,
) -> HerdrQueueWakePump {
    let process: Arc<dyn HerdrProcessAdapter> = fake;
    let clock_now = Arc::clone(&now);
    HerdrQueueWakePump::new(runtime, selector, health, process).with_clock(Arc::new(move || {
        *clock_now.lock().expect("test clock lock")
    }))
}

fn queue_idle_result(
    fake: &atm_herdr::testing::FakeHerdrProcessAdapter,
    key: &atm_core::boundary::MemberKey,
) {
    fake.queue_list_result(Ok(HerdrListOutcome {
        agents: vec![AgentSnapshot {
            name: Some(key.agent().to_string()),
            pane_id: None,
            status: HerdrAgentStatus::Idle,
            workspace_id: None,
        }],
    }));
}

fn queue_status_result(
    fake: &atm_herdr::testing::FakeHerdrProcessAdapter,
    keys: &[atm_storage::MemberKey],
    status: HerdrAgentStatus,
) {
    fake.queue_list_result(Ok(HerdrListOutcome {
        agents: keys
            .iter()
            .map(|key| AgentSnapshot {
                name: Some(key.agent().to_string()),
                pane_id: None,
                status,
                workspace_id: None,
            })
            .collect(),
    }));
}

fn prompt_texts(fake: &atm_herdr::testing::FakeHerdrProcessAdapter) -> Vec<String> {
    fake.calls()
        .into_iter()
        .filter_map(|call| match call {
            atm_herdr::testing::FakeHerdrCall::Prompt { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

fn clear_pending_markers(
    root: &std::path::Path,
    runtime: &LocalServiceRuntime,
    key: &atm_core::boundary::MemberKey,
) {
    let store = runtime.pending_nudge_store().expect("pending store");
    while let Some(claim) = store.claim_next_pending(key).expect("claim pending marker") {
        let message_id = claim.msg.to_string();
        let query = atm_core::read::ReadQuery::new(
            root.join("home"),
            root.join("home"),
            key.agent().clone(),
            Some(&format!("{}@{}", key.agent(), key.team())),
            key.team().clone(),
            atm_core::types::ReadSelection::All,
            false,
            true,
            Some(&message_id),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("read pending marker query");
        atm_core::read::read_mail_with_runtime(query, &NullObservability, runtime)
            .expect("close pending marker message");
    }
}

fn make_failed_attempt_due(
    store: &dyn atm_core::boundary::PendingNudgeStore,
    key: &atm_core::boundary::MemberKey,
    message_id: &AtmMessageId,
) {
    store
        .rearm_pending_after_handoff(
            key,
            message_id,
            IsoTimestamp::from_str("2020-01-01T00:00:00Z").expect("test timestamp"),
        )
        .expect("make the next failed claim due");
}

fn close_message(
    root: &std::path::Path,
    runtime: &LocalServiceRuntime,
    key: &atm_core::boundary::MemberKey,
    message_id: AtmMessageId,
) {
    let message_id = message_id.to_string();
    let query = atm_core::read::ReadQuery::new(
        root.join("home"),
        root.join("home"),
        key.agent().clone(),
        Some(&format!("{}@{}", key.agent(), key.team())),
        key.team().clone(),
        atm_core::types::ReadSelection::All,
        false,
        true,
        Some(&message_id),
        None,
        None,
        None,
        None,
        None,
    )
    .expect("read message query");
    atm_core::read::read_mail_with_runtime(query, &NullObservability, runtime)
        .expect("close message");
}

fn add_roster_member(runtime: &LocalServiceRuntime, team: &TeamName, agent: &str) {
    let mut roster = runtime
        .shared_roster_store_arc()
        .load_roster(team)
        .expect("load roster");
    roster.members.push(herdr_member(team, agent));
    runtime
        .shared_roster_store_arc()
        .save_roster(&roster)
        .expect("save roster member");
}

fn add_lead_roster_member(runtime: &LocalServiceRuntime, team: &TeamName, agent: &str) {
    let mut roster = runtime
        .shared_roster_store_arc()
        .load_roster(team)
        .expect("load roster");
    let mut member = herdr_member(team, agent);
    member.agent_type = atm_core::schema::AgentType::Lead;
    roster.members.push(member);
    runtime
        .shared_roster_store_arc()
        .save_roster(&roster)
        .expect("save lead roster member");
}

fn ack_message(
    root: &std::path::Path,
    runtime: &LocalServiceRuntime,
    team: &TeamName,
    message_id: AtmMessageId,
) {
    let home = root.join("home");
    ack_mail_with_runtime(
        AckRequest {
            home_dir: home.clone(),
            current_dir: home,
            caller_identity: "aq27-agent".parse().expect("agent"),
            caller_chat_id: None,
            caller_team: team.clone(),
            activity_observation: None,
            message_id,
            reply_body: "acknowledged".to_owned(),
        },
        &NullObservability,
        runtime,
    )
    .expect("message acknowledgement");
}

/// Starts an assigned task the way `atm task start` does: the assignee writes
/// a `TaskOp::Start` message back to the assigner
/// (`atm-storage-rusqlite/src/writer/task_start.rs:17`).
fn start_task(
    root: &std::path::Path,
    runtime: &LocalServiceRuntime,
    key: &atm_core::boundary::MemberKey,
    task_id: TaskId,
) {
    let home = root.join("home");
    let mut request = WriteRequest::new(
        home.clone(),
        home,
        key.agent().clone(),
        &format!("sender@{}", key.team()),
        key.team().clone(),
        SendMessageSource::Inline("task started".to_owned()),
        None,
        false,
        Some(task_id),
        false,
    )
    .expect("task start request");
    request.task_op = Some(atm_storage::TaskOp::Start);
    write_mail_with_runtime(request, &NullObservability, runtime).expect("task start");
}

fn complete_task(
    root: &std::path::Path,
    runtime: &LocalServiceRuntime,
    team: &TeamName,
    task_id: TaskId,
) {
    let home = root.join("home");
    let request = WriteRequest::new(
        home.clone(),
        home,
        "sender".parse().expect("sender"),
        &format!("aq27-agent@{team}"),
        team.clone(),
        SendMessageSource::Inline("task completed".to_owned()),
        None,
        false,
        None,
        false,
    )
    .expect("completion request")
    .with_nudge_mode(NudgeMode::Immediate)
    .with_task_complete(task_id);
    write_mail_with_runtime(request, &NullObservability, runtime).expect("task completion");
}

fn build_test_pump_with_agents(
    agents: Vec<AgentSnapshot>,
) -> (
    tempfile::TempDir,
    LocalServiceRuntime,
    Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
    Arc<HerdrQueueWakePump>,
    super::RuntimeHealth,
    atm_core::boundary::MemberKey,
) {
    let root = tempfile::tempdir().expect("temporary root");
    let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
    let team: TeamName = "aq27-team".parse().expect("team");
    let roster_agents: Vec<String> = if agents.is_empty() {
        vec!["aq27-agent".to_owned()]
    } else {
        agents
            .iter()
            .filter_map(|snapshot| snapshot.name.clone())
            .collect()
    };
    let members = roster_agents
        .iter()
        .map(|agent| herdr_member(&team, agent))
        .collect();
    assembly
        .service_runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: team.clone(),
            members,
            refreshed_at: None,
        })
        .expect("roster");
    for agent in &roster_agents {
        queue_message(root.path(), &assembly.service_runtime, &team, agent);
    }
    let key = atm_core::boundary::MemberKey::new(team, roster_agents[0].parse().expect("agent"));
    let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
    fake.queue_list_result(Ok(HerdrListOutcome { agents }));
    let selector = Arc::new(FakeSelector {
        emitter: FakeEmitter {
            process: Arc::clone(&fake),
        },
    });
    let process: Arc<dyn HerdrProcessAdapter> = fake.clone();
    let health = super::RuntimeHealth::default();
    let pump = Arc::new(HerdrQueueWakePump::new(
        assembly.service_runtime.clone(),
        selector,
        health.clone(),
        process,
    ));
    (root, assembly.service_runtime, fake, pump, health, key)
}

fn build_test_pump() -> (
    tempfile::TempDir,
    LocalServiceRuntime,
    Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
    Arc<HerdrQueueWakePump>,
    super::RuntimeHealth,
    atm_core::boundary::MemberKey,
) {
    build_test_pump_with_agents(vec![AgentSnapshot {
        name: Some("aq27-agent".to_owned()),
        pane_id: None,
        status: HerdrAgentStatus::Idle,
        workspace_id: None,
    }])
}

#[tokio::test]
async fn shared_herdr_server_prompts_each_team_by_its_roster_alias() {
    let root = tempfile::tempdir().expect("temporary root");
    let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
    let team_a: TeamName = "a-team".parse().expect("team");
    let team_b: TeamName = "b-team".parse().expect("team");
    assembly
        .service_runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: team_a.clone(),
            members: vec![herdr_member_with_alias(
                &team_a,
                atm_core::roles::ROLE_TEAM_LEAD,
                "team-lead_a-team",
            )],
            refreshed_at: None,
        })
        .expect("team a roster");
    assembly
        .service_runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: team_b.clone(),
            members: vec![herdr_member_with_alias(
                &team_b,
                atm_core::roles::ROLE_TEAM_LEAD,
                "team-lead_b-team",
            )],
            refreshed_at: None,
        })
        .expect("team b roster");
    queue_message(
        root.path(),
        &assembly.service_runtime,
        &team_a,
        atm_core::roles::ROLE_TEAM_LEAD,
    );
    queue_message(
        root.path(),
        &assembly.service_runtime,
        &team_b,
        atm_core::roles::ROLE_TEAM_LEAD,
    );

    let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
    fake.queue_list_result(Ok(HerdrListOutcome {
        agents: vec![
            AgentSnapshot {
                name: Some("team-lead_a-team".to_owned()),
                pane_id: None,
                status: HerdrAgentStatus::Idle,
                workspace_id: None,
            },
            AgentSnapshot {
                name: Some("team-lead_b-team".to_owned()),
                pane_id: None,
                status: HerdrAgentStatus::Idle,
                workspace_id: None,
            },
        ],
    }));
    let selector = Arc::new(FakeSelector {
        emitter: FakeEmitter {
            process: Arc::clone(&fake),
        },
    });
    let process: Arc<dyn HerdrProcessAdapter> = fake.clone();
    let runtime = assembly.service_runtime.clone();
    let pump = HerdrQueueWakePump::new(
        runtime.clone(),
        selector,
        super::RuntimeHealth::default(),
        process,
    );

    pump.tick_once().await;

    let prompted: Vec<String> = fake
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            atm_herdr::testing::FakeHerdrCall::Prompt { agent, .. } => Some(agent),
            _ => None,
        })
        .collect();
    assert_eq!(prompted.len(), 2);
    assert!(prompted.contains(&"team-lead_a-team".to_owned()));
    assert!(prompted.contains(&"team-lead_b-team".to_owned()));
}

type TaskOnlyPumpFixture = (
    tempfile::TempDir,
    LocalServiceRuntime,
    Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
    HerdrQueueWakePump,
    Arc<atm_storage::DummyTaskStore>,
    Vec<atm_storage::MemberKey>,
    Arc<Mutex<IsoTimestamp>>,
);

fn build_task_only_pump(
    statuses: Vec<HerdrAgentStatus>,
    fail_reminders: bool,
) -> TaskOnlyPumpFixture {
    build_task_only_pump_with_template(statuses, fail_reminders, None)
}

fn build_task_only_pump_without_delivery_channel() -> TaskOnlyPumpFixture {
    build_task_only_pump_with_channel(vec![HerdrAgentStatus::Idle], false, None, false, None)
}

fn build_task_only_pump_with_template(
    statuses: Vec<HerdrAgentStatus>,
    fail_reminders: bool,
    task_template: Option<&str>,
) -> TaskOnlyPumpFixture {
    build_task_only_pump_with_channel(statuses, fail_reminders, task_template, true, None)
}

fn build_task_only_pump_with_refusal_error(
    error: atm_storage::ReadLaneError,
) -> TaskOnlyPumpFixture {
    build_task_only_pump_with_channel(vec![HerdrAgentStatus::Idle], false, None, true, Some(error))
}

fn task_only_reader(
    rows: Vec<TaskRow>,
    task_store: &Arc<atm_storage::DummyTaskStore>,
    refusal_error: Option<atm_storage::ReadLaneError>,
) -> Arc<dyn atm_core::boundary::AsyncTaskLedgerReader + Send + Sync> {
    match refusal_error {
        Some(error) => Arc::new(
            atm_storage::testing::InMemoryTaskLedgerReader::with_rows(rows, Vec::new())
                .with_refusal_error(error),
        ),
        None => task_store.clone(),
    }
}

fn build_task_only_pump_with_channel(
    statuses: Vec<HerdrAgentStatus>,
    fail_reminders: bool,
    task_template: Option<&str>,
    task_channel_available: bool,
    refusal_error: Option<atm_storage::ReadLaneError>,
) -> TaskOnlyPumpFixture {
    let root = tempfile::tempdir().expect("temporary root");
    let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
    let team: TeamName = "ax5-task-only".parse().expect("team");
    let agents: Vec<String> = (0..statuses.len())
        .map(|index| format!("ax5-agent-{index:02}"))
        .collect();
    let members: Vec<RosterEntry> = agents
        .iter()
        .map(|agent| herdr_member(&team, agent))
        .collect();
    assembly
        .service_runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: team.clone(),
            members,
            refreshed_at: None,
        })
        .expect("roster");
    let assigned_at = IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("timestamp");
    let rows = task_rows(&team, &agents, assigned_at);
    if task_channel_available {
        if let Some(template) = task_template {
            assembly
                .nudge_template_override_store
                .save_template_override(
                    &team,
                    atm_storage::BuiltInNudgeTemplateKind::TaskReminder,
                    template,
                )
                .expect("task template override");
        }
    } else {
        for kind in [
            atm_storage::BuiltInNudgeTemplateKind::TaskReady,
            atm_storage::BuiltInNudgeTemplateKind::TaskReminder,
        ] {
            assembly
                .nudge_template_override_store
                .disable_template_override(&team, kind)
                .expect("disable task template override");
        }
    }
    let task_store = Arc::new(atm_storage::DummyTaskStore::with_rows(
        rows.clone(),
        fail_reminders,
    ));
    let reader = task_only_reader(rows, &task_store, refusal_error);
    let runtime = assembly
        .service_runtime
        .with_task_store(task_store.clone())
        .with_async_task_ledger_reader(reader);
    let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
    fake.queue_list_result(Ok(HerdrListOutcome {
        agents: agents
            .iter()
            .zip(statuses)
            .map(|(name, status)| AgentSnapshot {
                name: Some(name.clone()),
                pane_id: None,
                status,
                workspace_id: None,
            })
            .collect(),
    }));
    let now = Arc::new(Mutex::new(assigned_at));
    let health = super::RuntimeHealth::default();
    let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now))
        .with_daemon_home(root.path().join("home"));
    let keys = agents
        .into_iter()
        .map(|agent| atm_storage::MemberKey::new(team.clone(), agent.parse().expect("agent")))
        .collect();
    (root, runtime, fake, pump, task_store, keys, now)
}

fn build_task_handoff_pump() -> (
    tempfile::TempDir,
    LocalServiceRuntime,
    Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
    HerdrQueueWakePump,
    atm_core::boundary::MemberKey,
    TaskId,
    Arc<Mutex<IsoTimestamp>>,
) {
    let root = tempfile::tempdir().expect("temporary root");
    let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
    let team: TeamName = "ax5-handoff".parse().expect("team");
    let key =
        atm_core::boundary::MemberKey::new(team.clone(), "ax5-agent-00".parse().expect("agent"));
    assembly
        .service_runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: team.clone(),
            members: vec![
                herdr_member(&team, key.agent().as_str()),
                herdr_member(&team, "sender"),
            ],
            refreshed_at: None,
        })
        .expect("roster");
    let task_id: TaskId = "AX5-HANDOFF".parse().expect("task");
    queue_task_message(
        root.path(),
        &assembly.service_runtime,
        &team,
        key.agent().as_str(),
        task_id.clone(),
    );
    let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
    fake.queue_list_result(Ok(HerdrListOutcome {
        agents: vec![AgentSnapshot {
            name: Some(key.agent().to_string()),
            pane_id: None,
            status: HerdrAgentStatus::Idle,
            workspace_id: None,
        }],
    }));
    let now = Arc::new(Mutex::new(
        IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("timestamp"),
    ));
    let pump = pump_with_clock(
        assembly.service_runtime.clone(),
        fake.clone(),
        super::RuntimeHealth::default(),
        Arc::clone(&now),
    )
    .with_daemon_home(root.path().join("home"));
    (
        root,
        assembly.service_runtime,
        fake,
        pump,
        key,
        task_id,
        now,
    )
}

fn task_rows(team: &TeamName, agents: &[String], assigned_at: IsoTimestamp) -> Vec<TaskRow> {
    agents
        .iter()
        .enumerate()
        .map(|(index, agent)| TaskRow {
            position: None,
            team: team.clone(),
            task_id: format!("AX5-TASK-{index:02}").parse().expect("task id"),
            assignee: agent.parse().expect("agent"),
            assigner: "sender".parse().expect("assigner"),
            state: TaskState::Assigned,
            assignment_message_id: AtmMessageId::new(),
            description: format!("reminder body {index}"),
            assigned_at,
            updated_at: assigned_at,
            last_reminded_at: None,
            reminder_count: 0,
            lead_notified_count: 0,
        })
        .collect()
}

fn build_test_pump_with_two_sessions() -> (
    tempfile::TempDir,
    LocalServiceRuntime,
    Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
    Arc<HerdrQueueWakePump>,
    atm_core::boundary::MemberKey,
) {
    let root = tempfile::tempdir().expect("temporary root");
    let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
    let team: TeamName = "aq27-team".parse().expect("team");
    let members = vec![
        herdr_member_with_session(&team, "aq27-agent", "aq27-session-a"),
        herdr_member_with_session(&team, "aq27-agent-b", "aq27-session-b"),
    ];
    assembly
        .service_runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: team.clone(),
            members,
            refreshed_at: None,
        })
        .expect("roster");
    queue_message(root.path(), &assembly.service_runtime, &team, "aq27-agent");
    queue_message(
        root.path(),
        &assembly.service_runtime,
        &team,
        "aq27-agent-b",
    );
    let agents = vec![
        AgentSnapshot {
            name: Some("aq27-agent".to_owned()),
            pane_id: None,
            status: HerdrAgentStatus::Idle,
            workspace_id: None,
        },
        AgentSnapshot {
            name: Some("aq27-agent-b".to_owned()),
            pane_id: None,
            status: HerdrAgentStatus::Idle,
            workspace_id: None,
        },
    ];
    let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
    for _ in 0..2 {
        fake.queue_list_result(Ok(HerdrListOutcome {
            agents: agents.clone(),
        }));
    }
    let selector = Arc::new(FakeSelector {
        emitter: FakeEmitter {
            process: Arc::clone(&fake),
        },
    });
    let process: Arc<dyn HerdrProcessAdapter> = fake.clone();
    let pump = Arc::new(HerdrQueueWakePump::new(
        assembly.service_runtime.clone(),
        selector,
        super::RuntimeHealth::default(),
        process,
    ));
    let key = atm_core::boundary::MemberKey::new(team, "aq27-agent".parse().expect("agent"));
    (root, assembly.service_runtime, fake, pump, key)
}

async fn cancel_inflight_prompt() -> (
    tempfile::TempDir,
    LocalServiceRuntime,
    Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
    atm_core::boundary::MemberKey,
) {
    let (root, runtime, fake, pump, _health, key) = build_test_pump();
    let prompt_gate = fake.block_next_prompt();
    let prompt_started = pump.install_prompt_started_test_gate();
    let (shutdown_tx, shutdown_rx) = watch::channel(());
    let sender_clone = shutdown_tx.clone();
    let task = pump.clone().start(shutdown_rx);
    tokio::time::timeout(Duration::from_secs(1), prompt_started.notified())
        .await
        .expect("the fake prompt is in flight before shutdown");
    shutdown_tx.send(()).expect("shutdown notification");
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("pump joins after shutdown notification")
        .expect("poll task join");
    drop(prompt_gate);
    drop(sender_clone);
    (root, runtime, fake, key)
}

async fn test_pump() -> (
    tempfile::TempDir,
    LocalServiceRuntime,
    Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
    atm_core::boundary::MemberKey,
) {
    let (root, runtime, fake, pump, _health, key) = build_test_pump();
    pump.tick_once().await;
    assert_eq!(pump.stats().prompted, 1, "one queued message prompted");
    (root, runtime, fake, key)
}

#[test]
fn poll_contract_uses_fixed_cadence_and_cap() {
    assert_eq!(HERDR_POLL_INTERVAL_MS, 5_000);
    assert_eq!(HERDR_MAX_PROMPTS_PER_TICK, 16);
    assert_eq!(atm_core::boundary::TASK_REMINDER_INTERVAL_MS, 60_000);
}

#[tokio::test]
async fn ax5_01_assigned_task_is_reminded_without_a_state_transition() {
    let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
    let task_id: TaskId = "AX5-ASSIGNED".parse().expect("task id");
    queue_task_message(
        root.path(),
        &runtime,
        key.team(),
        key.agent().as_str(),
        task_id.clone(),
    );
    let now = Arc::new(Mutex::new(
        IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
    ));
    let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now));

    pump.tick_once().await;
    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:02:00Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;

    assert_eq!(
        runtime
            .task_store()
            .expect("task store")
            .load_task(key.team(), &task_id)
            .expect("load task")
            .expect("task row")
            .state,
        atm_storage::TaskState::Assigned,
        "a reminder never acknowledges the assignment"
    );
    assert_eq!(pump.stats().task_reminders, 0);
    let prompts = prompt_texts(&fake);
    assert!(prompts.iter().any(|text| text.starts_with("<atm from=\"")));
    assert!(prompts.iter().all(|text| !text.contains(task_id.as_str())));
}

#[tokio::test]
async fn ac01_ack_and_completion_advance_to_the_next_task_reminder() {
    let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
    clear_pending_markers(root.path(), &runtime, &key);
    let first: TaskId = "AX5-AC1-FIRST".parse().expect("task id");
    let second: TaskId = "AX5-AC1-SECOND".parse().expect("task id");
    let first_message_id = queue_task_message(
        root.path(),
        &runtime,
        key.team(),
        key.agent().as_str(),
        first.clone(),
    );
    let second_message_id = queue_task_message(
        root.path(),
        &runtime,
        key.team(),
        key.agent().as_str(),
        second.clone(),
    );
    let mut roster = runtime
        .shared_roster_store_arc()
        .load_roster(key.team())
        .expect("load roster");
    roster.members.push(herdr_member(key.team(), "sender"));
    // `shared_roster_store_arc()` is the write-through roster boundary:
    // this save already updated the RAM roster mirror in the same
    // operation, so no separate cache invalidation is needed here.
    runtime
        .shared_roster_store_arc()
        .save_roster(&roster)
        .expect("add task sender to roster");
    let now = Arc::new(Mutex::new(
        IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
    ));
    let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now));

    pump.tick_once().await;
    let prompts = prompt_texts(&fake);
    // BB.5: the first prompt is the task pass `task_ready` naming the head
    // task, not a queue drain of the assignment message
    // (nudge_dispatch.rs:24-31, send/nudge_template.rs:138).
    assert!(prompts.iter().any(|text| {
        text.starts_with(&format!(
            "<atm task=\"{}\" ready message=\"{first_message_id}\"",
            first.as_str()
        ))
    }));
    let prompts_after_first = prompts.len();
    assert_eq!(
        runtime
            .task_store()
            .expect("task store")
            .load_task(key.team(), &first)
            .expect("load first task")
            .expect("first task")
            .state,
        TaskState::Assigned,
        "prompting does not start the first task"
    );
    assert_eq!(
        runtime
            .task_store()
            .expect("task store")
            .load_task(key.team(), &first)
            .expect("load first task")
            .expect("first task")
            .reminder_count,
        1
    );
    complete_task(root.path(), &runtime, key.team(), first);
    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    let prompts = prompt_texts(&fake);
    assert!(prompts.len() > prompts_after_first);
    // BB.5: completing the head advances the pass to the next task, whose own
    // first prompt is `task_ready` again because its reminder count is still
    // zero (nudge_dispatch.rs:24-31).
    assert!(prompts[prompts_after_first..].iter().any(|text| {
        text.starts_with(&format!(
            "<atm task=\"{}\" ready message=\"{second_message_id}\"",
            second.as_str()
        ))
    }));
    assert_eq!(
        runtime
            .task_store()
            .expect("task store")
            .load_task(key.team(), &second)
            .expect("load second task")
            .expect("second task")
            .state,
        TaskState::Assigned,
        "prompting does not start the next task after queue advancement"
    );
}

#[tokio::test]
async fn ax5_02_drain_prompt_consumes_the_shared_reminder_budget() {
    let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
    // BB.5: an assignment writes no pending marker, so the fixture's own
    // ordinary queue message is the only drain candidate. Clear it first, or
    // the task pass holds on pending mail (herdr_task_disposition.rs:69) and
    // the cadence below is never reached.
    clear_pending_markers(root.path(), &runtime, &key);
    let task_id: TaskId = "AX5-BUDGET".parse().expect("task id");
    queue_task_message(
        root.path(),
        &runtime,
        key.team(),
        key.agent().as_str(),
        task_id.clone(),
    );
    let now = Arc::new(Mutex::new(
        IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
    ));
    let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now));
    pump.tick_once().await;
    // BB.5: the task pass is the only source of the first prompt; it spends
    // one unit of the shared per-tick prompt budget (task_pass.rs:287,424).
    assert_eq!(pump.stats().prompted, 1);
    assert_eq!(pump.stats().task_reminders, 1);
    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(pump.stats().task_reminders, 1);

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:02:00Z").expect("test timestamp");
    let _ = queue_message(root.path(), &runtime, key.team(), key.agent().as_str());
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(
        pump.stats().prompted,
        1,
        "only the fresh queue nudge is emitted"
    );
    // BB.5: the drain prompt still takes the member's prompt for this tick,
    // but it no longer counts as a task reminder; the retired queue-wide
    // reminder recorder is gone and the pass holds while mail is pending
    // (herdr_task_disposition.rs:69).
    assert_eq!(pump.stats().task_reminders, 0);

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:03:00Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(pump.stats().task_reminders, 0);
    let prompts = prompt_texts(&fake);
    assert_eq!(
        prompts.len(),
        3,
        "two task prompts on the reminder cadence, then one queue drain"
    );
    // BB.5: first prompt `task_ready`, later prompts `task_reminder` with a
    // rising attempt; the drain prompt is an ordinary queue nudge that names
    // no task (nudge_dispatch.rs:24-31, send/nudge_template.rs:138-143).
    assert!(prompts[0].starts_with(&format!("<atm task=\"{}\" ready ", task_id.as_str())));
    assert!(prompts[1].starts_with(&format!(
        "<atm task=\"{}\" reminder=\"1\" ",
        task_id.as_str()
    )));
    assert!(prompts[2].starts_with("<atm from=\""));
    assert!(!prompts[2].contains(task_id.as_str()));
}

#[tokio::test]
async fn ac02_seventeen_due_reminders_split_across_ticks_at_sixteen() {
    let statuses = vec![HerdrAgentStatus::Idle; 17];
    let (_root, _runtime, fake, pump, store, keys, _now) =
        build_task_only_pump(statuses.clone(), false);
    pump.tick_once().await;
    assert_eq!(pump.stats().prompted, 16);
    assert_eq!(pump.stats().task_reminders, 16);
    assert_eq!(prompt_texts(&fake).len(), 16);

    queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
    pump.tick_once().await;
    assert_eq!(pump.stats().prompted, 1);
    assert_eq!(pump.stats().task_reminders, 1);
    assert_eq!(prompt_texts(&fake).len(), 17);
    assert_eq!(
        store
            .row(&keys[16], &"AX5-TASK-16".parse().expect("task id"))
            .reminder_count,
        1
    );
}

#[tokio::test]
async fn ax5_05_emitted_prompts_consume_budget_when_audit_writes_fail() {
    let statuses = vec![HerdrAgentStatus::Idle; 17];
    let (_root, _runtime, fake, pump, store, _keys, _now) = build_task_only_pump(statuses, true);
    pump.tick_once().await;

    assert_eq!(pump.stats().prompted, 16);
    assert_eq!(pump.stats().task_reminders, 16);
    assert_eq!(prompt_texts(&fake).len(), 16);
    assert_eq!(
        store
            .row(
                &atm_storage::MemberKey::new(
                    "ax5-task-only".parse().expect("team"),
                    "ax5-agent-00".parse().expect("agent"),
                ),
                &"AX5-TASK-00".parse().expect("task id"),
            )
            .reminder_count,
        0
    );
}

#[tokio::test]
async fn task_pass_records_handoff_with_kind_and_attempt() {
    let (_root, _runtime, fake, pump, store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Idle], false);

    pump.tick_once().await;
    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("test timestamp");
    queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
    pump.tick_once().await;

    let task_id: TaskId = "AX5-TASK-00".parse().expect("task id");
    let handoffs = atm_core::boundary::AsyncTaskLedgerReader::list_prompt_handoffs(
        store.as_ref(),
        keys[0].team().clone(),
        task_id,
        ReadDeadline::new(Duration::from_secs(1)).expect("read deadline"),
    )
    .await
    .expect("list task-pass handoffs");
    assert_eq!(
        handoffs.len(),
        2,
        "each successful prompt writes one handoff"
    );
    assert_eq!(handoffs[0].kind.as_str(), "task_ready");
    assert_eq!(handoffs[0].attempt, 0);
    assert_eq!(handoffs[0].trigger, PromptTrigger::TaskPass);
    assert_eq!(handoffs[1].kind.as_str(), "task_reminder");
    assert_eq!(handoffs[1].attempt, 1);
    assert_eq!(handoffs[1].trigger, PromptTrigger::TaskPass);
}

#[tokio::test]
async fn ax5_09_generic_emit_failure_counts_and_respects_cooldown() {
    let (_root, _runtime, fake, pump, store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
    fake.queue_prompt_result(Err(atm_herdr::HerdrError::AgentPromptStalled));
    pump.tick_once().await;

    let task_id = "AX5-TASK-00".parse().expect("task id");
    assert_eq!(pump.stats().task_reminders_failed, 1);
    assert_eq!(pump.stats().task_reminders, 0);
    assert_eq!(store.row(&keys[0], &task_id).reminder_count, 0);
    assert_eq!(prompt_texts(&fake).len(), 1);

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:00:05Z").expect("test timestamp");
    fake.queue_prompt_result(Err(atm_herdr::HerdrError::AgentPromptStalled));
    queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
    pump.tick_once().await;
    assert_eq!(
        prompt_texts(&fake).len(),
        2,
        "failed emit retries next tick"
    );
    assert_eq!(pump.stats().task_reminders_failed, 1);

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("test timestamp");
    queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
    pump.tick_once().await;
    assert_eq!(pump.stats().task_reminders, 1);
    assert_eq!(
        prompt_texts(&fake).len(),
        3,
        "the next successful emit is recorded"
    );
    assert_eq!(store.row(&keys[0], &task_id).reminder_count, 1);

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:01:05Z").expect("test timestamp");
    queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
    pump.tick_once().await;
    assert_eq!(prompt_texts(&fake).len(), 3, "durable reminder rate-limits");
}

#[tokio::test]
async fn failed_task_pass_sink_records_no_handoff() {
    let (_root, _runtime, fake, pump, store, keys, _now) =
        build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
    fake.queue_prompt_result(Err(atm_herdr::HerdrError::AgentPromptStalled));

    pump.tick_once().await;

    let handoffs = atm_core::boundary::AsyncTaskLedgerReader::list_prompt_handoffs(
        store.as_ref(),
        keys[0].team().clone(),
        "AX5-TASK-00".parse().expect("task id"),
        ReadDeadline::new(Duration::from_secs(1)).expect("read deadline"),
    )
    .await
    .expect("list failed task-pass handoffs");
    assert!(handoffs.is_empty(), "a failed task-pass sink writes no row");
}

#[tokio::test]
async fn ax5_03_active_task_wins_over_a_newer_assigned_task() {
    let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
    // BB.5: an assignment writes no pending marker, so the fixture's ordinary
    // queue message is cleared first; otherwise the pass holds on pending mail
    // (herdr_task_disposition.rs:69) and no task is ever prompted.
    clear_pending_markers(root.path(), &runtime, &key);
    let first: TaskId = "AX5-ACTIVE".parse().expect("task id");
    let second: TaskId = "AX5-ASSIGNED-2".parse().expect("task id");
    let first_message = queue_task_message(
        root.path(),
        &runtime,
        key.team(),
        key.agent().as_str(),
        first.clone(),
    );
    queue_task_message(
        root.path(),
        &runtime,
        key.team(),
        key.agent().as_str(),
        second.clone(),
    );
    let mut roster = runtime
        .shared_roster_store_arc()
        .load_roster(key.team())
        .expect("load roster");
    roster.members.push(herdr_member(key.team(), "sender"));
    // `shared_roster_store_arc()` is the write-through roster boundary:
    // this save already updated the RAM roster mirror in the same
    // operation, so no separate cache invalidation is needed here.
    runtime
        .shared_roster_store_arc()
        .save_roster(&roster)
        .expect("add task sender to roster");
    // BB.5: acknowledging an assignment no longer starts its task, so the
    // active head is established through the `atm task start` write path
    // (writer/task_start.rs:17), which also moves it to position 1.
    start_task(root.path(), &runtime, &key, first.clone());
    let now = Arc::new(Mutex::new(
        IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
    ));
    let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now));

    pump.tick_once().await;
    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:02:00Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;

    let store = runtime.task_store().expect("task store");
    let active = store
        .load_task(key.team(), &first)
        .expect("load task")
        .expect("first task");
    assert_eq!(active.state, atm_storage::TaskState::Active);
    let reminders = prompt_texts(&fake);
    // BB.5: every prompt comes from the task pass and names the active head;
    // the newer assigned task is never prompted and never reminded
    // (task_pass.rs:311-343 keeps one head per member).
    assert_eq!(reminders.len(), 3);
    assert!(
        reminders
            .iter()
            .all(|text| text.contains(first.as_str()) && !text.contains(second.as_str()))
    );
    assert!(reminders[0].starts_with(&format!(
        "<atm task=\"{}\" ready message=\"{first_message}\"",
        first.as_str()
    )));
    assert_eq!(active.reminder_count, 3);
    let queued = store
        .load_task(key.team(), &second)
        .expect("load task")
        .expect("second task");
    assert_eq!(queued.state, atm_storage::TaskState::Assigned);
    assert_eq!(
        queued.reminder_count, 0,
        "a reminder is recorded only against the task the prompt was rendered for"
    );
}

#[tokio::test]
async fn ax5_04_emit_failure_retries_until_durable_reminder_rate_limits() {
    let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
    // BB.5: an assignment writes no pending marker, so the fixture's ordinary
    // queue message is cleared first; otherwise the pass holds on pending mail
    // (herdr_task_disposition.rs:69) and no prompt is ever attempted.
    clear_pending_markers(root.path(), &runtime, &key);
    let task_id: TaskId = "AX5-RETRY".parse().expect("task id");
    queue_task_message(
        root.path(),
        &runtime,
        key.team(),
        key.agent().as_str(),
        task_id.clone(),
    );
    let now = Arc::new(Mutex::new(
        IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
    ));
    let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now));
    pump.tick_once().await;

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:02:00Z").expect("test timestamp");
    fake.queue_prompt_result(Err(atm_herdr::HerdrError::AgentNotReady));
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    // BB.5: the two task-pass prompts at 00:00 and 00:01 are the only recorded
    // reminders; the failed emit at 00:02 records nothing and is counted as a
    // task-reminder failure instead (task_pass.rs:395-408).
    assert_eq!(
        runtime
            .task_store()
            .expect("task store")
            .load_task(key.team(), &task_id)
            .expect("load task")
            .expect("task row")
            .reminder_count,
        2
    );
    assert_eq!(pump.stats().task_reminders_failed, 1);
    assert_eq!(pump.stats().task_reminders, 0);

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:02:05Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    // BB.5: the failed emit left `last_reminded_at` at 00:01, so the very next
    // tick is still due and retries the prompt (herdr_task_disposition.rs:76).
    assert_eq!(
        pump.stats().task_reminders,
        1,
        "the failed emit is retried on the next tick"
    );
    assert_eq!(
        runtime
            .task_store()
            .expect("task store")
            .load_task(key.team(), &task_id)
            .expect("load task")
            .expect("task row")
            .reminder_count,
        3
    );

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:03:00Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(
        pump.stats().task_reminders,
        0,
        "the durable reminder timestamp rate-limits the later tick"
    );
    assert_eq!(
        prompt_texts(&fake).len(),
        4,
        "two prompts on cadence, one failed emit, one retry"
    );
}

#[tokio::test]
async fn ac04_breaker_and_absent_emitter_leave_no_reminder_audit() {
    let (_root, _runtime, fake, pump, store, keys, _now) =
        build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
    fake.queue_prompt_result(Err(atm_herdr::HerdrError::ServerUnavailable {
        message: String::new(),
        retry_after: None,
        io_error_kind: None,
    }));
    pump.tick_once().await;
    let task_id = "AX5-TASK-00".parse().expect("task id");
    assert_eq!(pump.stats().prompted, 0);
    assert_eq!(pump.stats().breaker_open, 1);
    assert_eq!(store.row(&keys[0], &task_id).reminder_count, 0);

    queue_status_result(&fake, &keys, HerdrAgentStatus::Idle);
    pump.tick_once().await;
    assert_eq!(
        pump.stats().task_reminders,
        1,
        "closed breaker resumes next tick"
    );
    assert_eq!(store.row(&keys[0], &task_id).reminder_count, 1);

    let (_root, runtime, fake, _pump, store, keys, now) =
        build_task_only_pump(vec![HerdrAgentStatus::Idle], false);
    let no_emitter = pump_with_selector(
        runtime.clone(),
        fake.clone(),
        super::RuntimeHealth::default(),
        now,
        Arc::new(NoEmitterSelector),
    );
    no_emitter.tick_once().await;
    assert_eq!(no_emitter.stats().prompted, 0);
    assert_eq!(no_emitter.stats().task_reminders, 0);
    assert_eq!(store.row(&keys[0], &task_id).reminder_count, 0);
}

#[tokio::test]
async fn ax5_06_task_reminder_only_appends_audit_bookkeeping() {
    let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
    // BB.5: an assignment writes no pending marker, so the fixture's ordinary
    // queue message is cleared first; otherwise the pass holds on pending mail
    // (herdr_task_disposition.rs:69) and records nothing at all.
    clear_pending_markers(root.path(), &runtime, &key);
    let task_id: TaskId = "AX5-AUDIT-ONLY".parse().expect("task id");
    queue_task_message(
        root.path(),
        &runtime,
        key.team(),
        key.agent().as_str(),
        task_id.clone(),
    );
    let now = Arc::new(Mutex::new(
        IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("test timestamp"),
    ));
    let pump = pump_with_clock(runtime.clone(), fake.clone(), health, Arc::clone(&now));
    pump.tick_once().await;
    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:01:00Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:02:00Z").expect("test timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;

    let row = runtime
        .task_store()
        .expect("task store")
        .load_task(key.team(), &task_id)
        .expect("load task")
        .expect("task row");
    let events = runtime
        .task_store()
        .expect("task store")
        .list_task_events(key.team(), &task_id, Some(key.agent()))
        .expect("task events");
    assert_eq!(row.state, atm_storage::TaskState::Assigned);
    // BB.5: every prompt now comes from the task pass, so all three ticks on
    // the 60s cadence record a reminder (task_pass.rs:419, task_store.rs:305);
    // before BB.5 the first two ticks were queue drains of the assignment and
    // the third found no due marker.
    assert_eq!(row.reminder_count, 3);
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event == atm_storage::TaskEventKind::Reminded)
            .count(),
        3,
        "one `reminded` event per emitted prompt, against the prompted task"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event == atm_storage::TaskEventKind::Assigned)
            .count(),
        1
    );
    // BB.5: an assignment is never acknowledged and a prompt never starts the
    // task, so the audit trail holds no `acked` or `started` row (D1, D3).
    assert!(events.iter().all(|event| {
        event.event != atm_storage::TaskEventKind::Acked
            && event.event != atm_storage::TaskEventKind::Started
    }));
}

#[tokio::test]
async fn ax5_05_drain_precedes_task_reminder_and_clock_controls_cadence() {
    let (root, runtime, fake, _old_pump, health, key) = build_test_pump();
    // BB.5: the assignment is written immediately and never enters the drain
    // (send/mod.rs:376-385), so the drain half of this test is driven by an
    // ordinary queue message whose id the test owns.
    clear_pending_markers(root.path(), &runtime, &key);
    let task_id: TaskId = "AX5-REMINDER".parse().expect("task id");
    queue_task_message(
        root.path(),
        &runtime,
        key.team(),
        key.agent().as_str(),
        task_id.clone(),
    );
    let queue_message_id = queue_message(root.path(), &runtime, key.team(), key.agent().as_str());
    let now = Arc::new(Mutex::new(IsoTimestamp::now()));
    let clock_now = Arc::clone(&now);
    let selector = Arc::new(FakeSelector {
        emitter: FakeEmitter {
            process: Arc::clone(&fake),
        },
    });
    let process: Arc<dyn HerdrProcessAdapter> = fake.clone();
    let pump = HerdrQueueWakePump::new(runtime.clone(), selector, health, process).with_clock(
        Arc::new(move || *clock_now.lock().expect("test clock lock")),
    );

    // The drain runs before the task pass and the pass holds while that queue
    // item is open, so the tick emits exactly one prompt.
    pump.tick_once().await;
    assert_eq!(pump.stats().prompted, 1);
    // BB.5: a drained queue prompt never records a reminder against the task;
    // the retired queue-wide recorder was deleted from task_pass.rs.
    assert_eq!(pump.stats().task_reminders, 0, "drain counts no reminder");
    close_message(root.path(), &runtime, &key, queue_message_id);

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:00:00Z").expect("future timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(
        pump.stats().task_reminders,
        1,
        "the task pass owns this tick"
    );

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:00:30Z").expect("future timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(
        pump.stats().task_reminders,
        0,
        "the clock rate-limits inside the reminder interval"
    );

    *now.lock().expect("test clock lock") =
        IsoTimestamp::from_str("2030-01-01T00:01:05Z").expect("future timestamp");
    queue_idle_result(&fake, &key);
    pump.tick_once().await;
    assert_eq!(pump.stats().task_reminders, 1);

    let row = runtime
        .task_store()
        .expect("task store")
        .load_task(key.team(), &task_id)
        .expect("load task")
        .expect("task row");
    assert_eq!(row.reminder_count, 2);
    let prompts = prompt_texts(&fake);
    assert_eq!(prompts.len(), 3, "one drain, then two task-pass prompts");
    assert!(prompts[0].starts_with("<atm from=\"") && !prompts[0].contains(task_id.as_str()));
    assert!(prompts[1].starts_with(&format!("<atm task=\"{}\" ready ", task_id.as_str())));
    assert!(prompts[2].starts_with(&format!(
        "<atm task=\"{}\" reminder=\"1\" ",
        task_id.as_str()
    )));
}

#[tokio::test]
async fn ax5_08_missing_task_store_skips_only_the_task_step() {
    let root = tempfile::tempdir().expect("temporary root");
    let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
    let team: TeamName = "aq27-team".parse().expect("team");
    assembly
        .service_runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: team.clone(),
            members: vec![herdr_member(&team, "aq27-agent")],
            refreshed_at: None,
        })
        .expect("roster");
    let _message = queue_message(root.path(), &assembly.service_runtime, &team, "aq27-agent");
    let pending = assembly
        .service_runtime
        .pending_nudge_store()
        .expect("pending store");
    let async_reader = assembly
        .service_runtime
        .async_task_ledger_reader()
        .expect("task reader");
    let roster = atm_runtime_test_support::build_write_through_roster_for_test(
        assembly.shared_roster_store_arc(),
    )
    .expect("write-through roster fixture hydrates from the isolated sqlite assembly");
    let runtime = LocalServiceRuntime::new_with_delivery_boundaries(
        assembly.message_store_arc(),
        roster,
        assembly.nudge_template_override_store.clone(),
        Arc::new(atm_core::LocalFileNonClaudeOutbound::new()),
    )
    .with_pending_nudge_store(pending)
    .with_async_task_ledger_reader(async_reader);
    let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
    fake.queue_list_result(Ok(HerdrListOutcome {
        agents: vec![AgentSnapshot {
            name: Some("aq27-agent".to_owned()),
            pane_id: None,
            status: HerdrAgentStatus::Idle,
            workspace_id: None,
        }],
    }));
    let health = super::RuntimeHealth::default();
    let selector = Arc::new(FakeSelector {
        emitter: FakeEmitter {
            process: Arc::clone(&fake),
        },
    });
    let process: Arc<dyn HerdrProcessAdapter> = fake.clone();
    let pump = HerdrQueueWakePump::new(runtime, selector, health, process);

    pump.tick_once().await;
    assert_eq!(pump.stats().prompted, 1, "queue drain remains active");
    assert_eq!(pump.stats().task_reminders, 0);
    assert!(pump.stats().task_step_skipped);
}

#[tokio::test]
async fn ac01_fifo_per_member_via_claim() {
    let (root, runtime, fake, pump, _health, key) = build_test_pump();
    queue_message(root.path(), &runtime, key.team(), key.agent().as_str());
    queue_message(root.path(), &runtime, key.team(), key.agent().as_str());
    pump.tick_once().await;
    assert_eq!(pump.stats().prompted, 1, "FIFO claims one message per tick");
    let prompt_text = fake
        .calls()
        .into_iter()
        .find_map(|call| match call {
            atm_herdr::testing::FakeHerdrCall::Prompt { text, .. } => Some(text),
            _ => None,
        })
        .expect("queue tick prompt");
    let prompted_message_id = prompt_text
        .split("message-id=\"")
        .nth(1)
        .and_then(|value| value.split('"').next())
        .expect("message id in rendered prompt");
    assert_eq!(
        prompt_text,
        format!(
            "<atm from=\"sender@aq27-team\" message-id=\"{prompted_message_id}\">\n  <action>atm read --message-id {prompted_message_id}</action>\n  <description>AQ2.7 test message</description>\n  <action>execute the assigned task</action>\n  <console announce=\"concise\" pause=\"false\"/>\n</atm>"
        )
    );
    assert!(
        runtime
            .pending_nudge_store()
            .expect("pending store")
            .list_pending_members()
            .expect("pending members")
            .contains(&key)
    );
}

/// Proves the ephemeral roster wake-pending flag actually mutates on a
/// real Herdr wake attempt (FTQ-AW finding 4 on PR #1240): it is unset
/// before any attempt, set while a claim's prompt is in flight, and
/// clears again once the attempt concludes -- regardless of the claim
/// eventually succeeding.
#[tokio::test]
async fn ac13_herdr_wake_pending_ephemeral_state_tracks_the_in_flight_claim() {
    let (root, runtime, fake, pump, _health, key) = build_test_pump();
    assert_eq!(
        runtime.roster_ephemeral_state(key.team(), key.agent()),
        Some(atm_storage::RosterMemberEphemeralState::default()),
        "no wake attempt is in flight before the first tick"
    );

    let prompt_gate = fake.block_next_prompt();
    let (shutdown_tx, shutdown_rx) = watch::channel(());
    let task = pump.start(shutdown_rx);
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if runtime
                .roster_ephemeral_state(key.team(), key.agent())
                .expect("member present in RAM roster")
                .herdr_wake_pending
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("wake-pending ephemeral state must be set while a claim is in flight");

    drop(prompt_gate);
    shutdown_tx.send(()).expect("shutdown notification");
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("pump joins after shutdown notification")
        .expect("poll task join");

    assert!(
        !runtime
            .roster_ephemeral_state(key.team(), key.agent())
            .expect("member present in RAM roster")
            .herdr_wake_pending,
        "wake-pending must clear once the claim attempt concludes"
    );
    let _ = root;
}

#[tokio::test]
async fn ac02_burst_cap_is_sixteen_successful_prompts() {
    let agents: Vec<AgentSnapshot> = (0..17)
        .map(|index| AgentSnapshot {
            name: Some(if index == 0 {
                "aq27-agent".to_owned()
            } else {
                format!("aq27-agent-{index:02}")
            }),
            pane_id: None,
            status: HerdrAgentStatus::Idle,
            workspace_id: None,
        })
        .collect();
    let (_root, runtime, fake, pump, _health, key) = build_test_pump_with_agents(agents);
    pump.tick_once().await;
    assert_eq!(pump.stats().prompted, HERDR_MAX_PROMPTS_PER_TICK);
    assert_eq!(
        fake.calls()
            .iter()
            .filter(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
            .count(),
        HERDR_MAX_PROMPTS_PER_TICK
    );
    assert_eq!(
        runtime
            .pending_nudge_store()
            .expect("pending store")
            .list_pending_members()
            .expect("pending members")
            .len(),
        HERDR_MAX_PROMPTS_PER_TICK + 1
    );
    let remaining = atm_core::boundary::MemberKey::new(
        key.team().clone(),
        "aq27-agent-16".parse().expect("agent"),
    );
    let store = runtime.pending_nudge_store().expect("pending store");
    let claim = store
        .claim_next_pending(&remaining)
        .expect("remaining claim")
        .expect("cap leaves remaining marker");
    assert_eq!(claim.attempt, 0, "the capped member was never claimed");
    store
        .release_pending(&remaining, &claim)
        .expect("restore cap assertion claim");
}

#[tokio::test]
async fn ac03_session_grouping_is_part_of_the_poll_contract() {
    let (_root, _runtime, fake, pump, _key) = build_test_pump_with_two_sessions();
    pump.tick_once().await;
    let list_sessions: Vec<_> = fake
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            atm_herdr::testing::FakeHerdrCall::List { session } => session,
            _ => None,
        })
        .collect();
    assert_eq!(list_sessions.len(), 2);
    assert!(
        list_sessions
            .iter()
            .any(|session| session.as_str() == "aq27-session-a")
    );
    assert!(
        list_sessions
            .iter()
            .any(|session| session.as_str() == "aq27-session-b")
    );
}

#[tokio::test]
async fn ac04_shutdown_send_stops_pump_before_drain_completes() {
    let (_root, runtime, fake, key) = cancel_inflight_prompt().await;
    let calls = fake.calls();
    assert_eq!(
        calls
            .iter()
            .filter(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
            .count(),
        1,
        "shutdown leaves no second prompt"
    );
    assert!(
        runtime
            .pending_nudge_store()
            .expect("pending store")
            .list_pending_members()
            .expect("pending members")
            .contains(&key)
    );
}

#[tokio::test]
async fn ac05_fake_adapter_breaker_error_does_not_prompt() {
    let root = tempfile::tempdir().expect("temporary root");
    let assembly = open_isolated_sqlite_boundary(root.path()).expect("runtime");
    let team: TeamName = "aq27-team".parse().expect("team");
    assembly
        .service_runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: team.clone(),
            members: vec![herdr_member(&team, "aq27-agent")],
            refreshed_at: None,
        })
        .expect("roster");
    let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
    fake.queue_list_result(Err(atm_herdr::HerdrError::ServerUnavailable {
        message: String::new(),
        retry_after: None,
        io_error_kind: None,
    }));
    let selector = Arc::new(FakeSelector {
        emitter: FakeEmitter {
            process: Arc::clone(&fake),
        },
    });
    let process: Arc<dyn HerdrProcessAdapter> = fake.clone();
    let runtime = assembly.service_runtime.clone();
    let pump = HerdrQueueWakePump::new(
        runtime.clone(),
        selector,
        super::RuntimeHealth::default(),
        process,
    );
    pump.tick_once().await;
    assert_eq!(pump.stats().prompted, 0);
    assert!(pump.stats().breaker_open > 0);
    let observation = runtime
        .roster_ephemeral_state(&team, &"aq27-agent".parse().expect("agent"))
        .expect("canonical roster member")
        .runtime;
    assert_eq!(observation.state, RuntimeMemberState::Unknown);
    assert_eq!(observation.revision.get(), 0);
    assert_eq!(
        observation.availability,
        RuntimeObservationAvailability::Unavailable
    );
}

#[tokio::test]
async fn ac06_blocked_race_releases_pending_with_zero_injected_bytes() {
    let (_root, runtime, fake, pump, _health, key) = build_test_pump();
    fake.queue_prompt_result(Err(atm_herdr::HerdrError::AgentBlocked));

    pump.tick_once().await;

    assert_eq!(pump.stats().prompted, 0, "blocked prompt injected no bytes");
    assert_eq!(pump.stats().released, 1);
    assert_eq!(pump.release_streak_for(&key), 1);
    assert_eq!(
        fake.calls()
            .iter()
            .filter(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
            .count(),
        1,
        "the post-claim prompt was attempted exactly once"
    );

    let store = runtime.pending_nudge_store().expect("pending store");
    let claim = store
        .claim_next_pending(&key)
        .expect("claim released message")
        .expect("blocked claim remains pending");
    assert_eq!(claim.attempt, 0, "blocked input consumes no retry debt");
    store.release_pending(&key, &claim).expect("restore claim");
}

#[tokio::test]
async fn ac06_not_found_family_releases_without_input() {
    for error in [
        atm_herdr::HerdrError::AgentNotFound,
        atm_herdr::HerdrError::AgentTargetAmbiguous,
        atm_herdr::HerdrError::AgentNotReady,
    ] {
        let (_root, runtime, fake, pump, _health, key) = build_test_pump();
        fake.queue_prompt_result(Err(error));

        pump.tick_once().await;

        assert_eq!(pump.stats().prompted, 0);
        assert_eq!(pump.stats().released, 1);
        assert_eq!(pump.release_streak_for(&key), 1);
        let prompt_calls = fake
            .calls()
            .iter()
            .filter(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
            .count();
        assert_eq!(
            prompt_calls, 1,
            "each lifecycle error reaches one prompt call"
        );

        let store = runtime.pending_nudge_store().expect("pending store");
        let claim = store
            .claim_next_pending(&key)
            .expect("claim released message")
            .expect("not-found-family claim remains pending");
        assert_eq!(claim.attempt, 0, "not-present input consumes no retry debt");
        store.release_pending(&key, &claim).expect("restore claim");
    }
}

#[tokio::test]
async fn ac06_consecutive_release_bound_requeues_after_ten() {
    let (_root, runtime, fake, pump, _health, key) = build_test_pump();
    for _ in 0..HERDR_MAX_CONSECUTIVE_RELEASES {
        fake.queue_list_result(Ok(HerdrListOutcome {
            agents: vec![AgentSnapshot {
                name: Some(key.agent().to_string()),
                pane_id: None,
                status: HerdrAgentStatus::Idle,
                workspace_id: None,
            }],
        }));
    }
    for _ in 0..=HERDR_MAX_CONSECUTIVE_RELEASES {
        fake.queue_prompt_result(Err(atm_herdr::HerdrError::AgentBlocked));
    }

    let store = runtime.pending_nudge_store().expect("pending store");
    for release_number in 1..=HERDR_MAX_CONSECUTIVE_RELEASES + 1 {
        pump.tick_once().await;
        let claim = store
            .claim_next_pending(&key)
            .expect("claim resolved message")
            .expect("resolved claim remains pending");
        let expected_attempt = if release_number > HERDR_MAX_CONSECUTIVE_RELEASES {
            1
        } else {
            0
        };
        assert_eq!(claim.attempt, expected_attempt, "release {release_number}");
        assert_eq!(
            pump.release_streak_for(&key),
            if release_number > HERDR_MAX_CONSECUTIVE_RELEASES {
                0
            } else {
                release_number
            },
            "release counter at outcome {release_number}"
        );
        store.release_pending(&key, &claim).expect("restore claim");
    }
}

#[tokio::test]
async fn ac07_absent_members_are_not_presented_as_idle() {
    let (_root, runtime, fake, pump, _health, key) = build_test_pump_with_agents(Vec::new());
    pump.tick_once().await;
    assert_eq!(pump.stats().not_present, 1);
    assert!(
        !fake
            .calls()
            .iter()
            .any(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
    );
    assert!(
        runtime
            .pending_nudge_store()
            .expect("pending store")
            .list_pending_members()
            .expect("pending members")
            .contains(&key)
    );
    let observation = runtime
        .roster_ephemeral_state(key.team(), key.agent())
        .expect("canonical roster member")
        .runtime;
    assert_eq!(observation.state, RuntimeMemberState::Unknown);
    assert_eq!(observation.revision.get(), 1);
    assert_eq!(
        observation.availability,
        RuntimeObservationAvailability::Fresh
    );
}

#[tokio::test]
async fn ac08_dispatch_selector_is_used_by_tick_once() {
    let (_root, runtime, fake, key) = test_pump().await;
    let prompt_text = fake
        .calls()
        .into_iter()
        .find_map(|call| match call {
            atm_herdr::testing::FakeHerdrCall::Prompt { text, .. } => Some(text),
            _ => None,
        })
        .expect("queue tick prompt");
    let message_id = prompt_text
        .split("message-id=\"")
        .nth(1)
        .and_then(|value| value.split('\"').next())
        .expect("message id in rendered prompt");
    assert_eq!(
        prompt_text,
        format!(
            "<atm from=\"sender@aq27-team\" message-id=\"{message_id}\">\n  <action>atm read --message-id {message_id}</action>\n  <description>AQ2.7 test message</description>\n  <action>execute the assigned task</action>\n  <console announce=\"concise\" pause=\"false\"/>\n</atm>"
        )
    );
    assert!(
        fake.calls()
            .iter()
            .any(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
    );
    assert!(
        runtime
            .pending_nudge_store()
            .expect("pending store")
            .list_pending_members()
            .expect("pending members")
            .contains(&key),
        "successful deferred delivery rearms its queue marker"
    );
    assert_eq!(key.agent().as_str(), "aq27-agent");
}

#[tokio::test]
async fn ac09_fake_adapter_never_needs_wait_for_queue_wake() {
    let (_root, _runtime, fake, _key) = test_pump().await;
    assert!(
        !fake
            .calls()
            .iter()
            .any(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Wait { .. }))
    );
}

#[tokio::test]
async fn ac10_herdr_statuses_update_canonical_roster_state() {
    let agents = vec![AgentSnapshot {
        name: Some("aq27-agent".to_owned()),
        pane_id: None,
        status: HerdrAgentStatus::Working,
        workspace_id: None,
    }];
    let (_root, runtime, _fake, pump, health, key) = build_test_pump_with_agents(agents);
    pump.tick_once().await;
    let member = runtime
        .roster_ephemeral_state(key.team(), key.agent())
        .expect("Herdr member canonical observation")
        .runtime;
    assert_eq!(member.state, RuntimeMemberState::Active);
    assert_eq!(
        member.state_changed_by,
        Some(atm_core::protocol::RuntimeObservationSource::HerdrPoll)
    );
    assert!(health.snapshot().members.is_empty());
}

#[tokio::test]
async fn ac11_claim_drop_guard_releases_marker_on_cancellation() {
    let (_root, runtime, _fake, key) = cancel_inflight_prompt().await;
    assert!(
        runtime
            .pending_nudge_store()
            .expect("pending store")
            .list_pending_members()
            .expect("pending members")
            .contains(&key)
    );
    assert_eq!(
        runtime
            .pending_nudge_store()
            .expect("pending store")
            .claim_next_pending(&key)
            .expect("claim after cancellation")
            .expect("released claim")
            .attempt,
        0
    );
}

#[tokio::test]
async fn ac11_claim_drop_guard_release_is_joined_before_pump_shutdown() {
    let (_root, runtime, fake, pump, _health, key) = build_test_pump();
    let prompt_gate = fake.block_next_prompt();
    let prompt_started = pump.install_prompt_started_test_gate();
    let (shutdown_tx, shutdown_rx) = watch::channel(());
    let task = pump.clone().start(shutdown_rx);
    tokio::time::timeout(Duration::from_secs(1), prompt_started.notified())
        .await
        .expect("the fake prompt is in flight before shutdown");

    shutdown_tx.send(()).expect("shutdown notification");
    task.await.expect("poll task joins after shutdown");

    assert!(
        pump.release_handles
            .lock()
            .expect("release handles lock")
            .is_empty(),
        "shutdown must drain and join every drop release handle"
    );
    let claim = runtime
        .pending_nudge_store()
        .expect("pending store")
        .claim_next_pending(&key)
        .expect("claim after shutdown")
        .expect("cancellation releases the claim before pump shutdown returns");
    assert_eq!(
        claim.attempt, 0,
        "cancellation release preserves retry state"
    );
    drop(prompt_gate);
}

#[tokio::test(start_paused = true)]
async fn stalled_drop_release_does_not_outlive_the_shutdown_deadline() {
    let (_root, runtime, _fake, pump, health, key) = build_test_pump();
    let release_started = Arc::new(AtomicBool::new(false));
    let release_blocked = Arc::new(AtomicBool::new(true));
    let store = Arc::new(
        atm_storage::testing::DummyPendingNudgeStore::default()
            .with_release_blocker(Arc::clone(&release_started), Arc::clone(&release_blocked)),
    );
    let store: Arc<dyn atm_core::boundary::PendingNudgeStore + Send + Sync> = store;
    let release = ReleasePendingOnDrop::new(
        store,
        key,
        atm_core::boundary::NudgeClaim {
            msg: AtmMessageId::new(),
            attempt: 0,
        },
        Arc::new(Mutex::new(HashMap::new())),
        runtime,
        Arc::clone(&pump.release_handles),
        pump.blocking_bridge.clone(),
    );
    drop(release);
    while !release_started.load(Ordering::Acquire) {
        tokio::task::yield_now().await;
    }

    let (shutdown_tx, shutdown_rx) = watch::channel(());
    let task = pump.start(shutdown_rx);
    shutdown_tx.send(()).expect("shutdown notification");
    tokio::time::advance(HERDR_REQUEST_BUDGET).await;
    tokio::task::yield_now().await;

    assert!(
        task.is_finished(),
        "a stalled release store must not hold pump shutdown past its request deadline"
    );
    assert_eq!(health.snapshot().blocking_core_bridge_stalls_total, 1);
    release_blocked.store(false, Ordering::Release);
    task.await.expect("bounded pump shutdown joins");
}

#[test]
fn release_pending_on_drop_without_runtime_releases_synchronously() {
    let (_root, runtime, _fake, pump, _health, key) = build_test_pump();
    let store = runtime.pending_nudge_store().expect("pending store");
    let claim = store
        .claim_next_pending(&key)
        .expect("claim pending")
        .expect("queued message claim");
    let release_handles = Arc::new(Mutex::new(Vec::new()));
    let release = ReleasePendingOnDrop::new(
        Arc::clone(&store),
        key.clone(),
        claim,
        Arc::new(Mutex::new(HashMap::new())),
        runtime,
        release_handles,
        pump.blocking_bridge.clone(),
    );

    drop(release);

    let released_claim = store
        .claim_next_pending(&key)
        .expect("claim after synchronous release")
        .expect("drop release makes the message claimable");
    assert_eq!(
        released_claim.attempt, 0,
        "inline fallback preserves retry state"
    );
}

#[tokio::test]
async fn ac11_successful_prompt_cancellation_cannot_rerelease_claim() {
    let (_root, runtime, fake, pump, _health, key) = build_test_pump();
    let (clear_started, _allow_clear) = pump.install_handoff_cleanup_test_gate();
    let (shutdown_tx, shutdown_rx) = watch::channel(());
    let task = pump.clone().start(shutdown_rx);
    tokio::time::timeout(Duration::from_secs(1), clear_started.notified())
        .await
        .expect("marker cleanup completes before cancellation");

    shutdown_tx.send(()).expect("shutdown notification");
    task.await.expect("poll task join");
    assert!(
        runtime
            .pending_nudge_store()
            .expect("pending store")
            .claim_next_pending(&key)
            .expect("claim while cleanup is gated")
            .is_none(),
        "a successful Herdr prompt must not be re-released while cleanup is pending"
    );

    assert!(
        runtime
            .pending_nudge_store()
            .expect("pending store")
            .list_pending_members()
            .expect("pending members")
            .contains(&key),
        "completed handoff rearms the queue marker"
    );

    fake.queue_list_result(Ok(HerdrListOutcome {
        agents: vec![AgentSnapshot {
            name: Some("aq27-agent".to_owned()),
            pane_id: None,
            status: HerdrAgentStatus::Idle,
            workspace_id: None,
        }],
    }));
    pump.tick_once().await;
    assert_eq!(
        fake.calls()
            .iter()
            .filter(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
            .count(),
        1,
        "the next tick must not prompt the already accepted message again"
    );
    pump.clear_handoff_cleanup_test_gate();
}

#[tokio::test]
async fn ac12_cursor_contract_is_rotation_not_reordering() {
    let agents: Vec<AgentSnapshot> = (0..20)
        .map(|index| AgentSnapshot {
            name: Some(if index == 0 {
                "aq27-agent".to_owned()
            } else {
                format!("aq27-agent-{index:02}")
            }),
            pane_id: None,
            status: HerdrAgentStatus::Idle,
            workspace_id: None,
        })
        .collect();
    let (root, runtime, fake, pump, _health, _key) = build_test_pump_with_agents(agents);
    pump.tick_once().await;
    assert_eq!(pump.cursor_position(), HERDR_MAX_PROMPTS_PER_TICK);

    let changed_agents: Vec<AgentSnapshot> = (0..22)
        .map(|index| AgentSnapshot {
            name: Some(if index == 0 {
                "aq27-agent".to_owned()
            } else {
                format!("aq27-agent-{index:02}")
            }),
            pane_id: None,
            status: HerdrAgentStatus::Idle,
            workspace_id: None,
        })
        .collect();
    let team: TeamName = "aq27-team".parse().expect("team");
    let members = (0..22)
        .map(|index| {
            let agent = if index == 0 {
                "aq27-agent".to_owned()
            } else {
                format!("aq27-agent-{index:02}")
            };
            herdr_member(&team, &agent)
        })
        .collect();
    runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: team.clone(),
            members,
            refreshed_at: None,
        })
        .expect("changed roster");
    queue_message(root.path(), &runtime, &team, "aq27-agent");
    queue_message(root.path(), &runtime, &team, "aq27-agent-20");
    queue_message(root.path(), &runtime, &team, "aq27-agent-21");
    fake.queue_list_result(Ok(HerdrListOutcome {
        agents: changed_agents,
    }));
    pump.tick_once().await;
    let prompted: Vec<String> = fake
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            atm_herdr::testing::FakeHerdrCall::Prompt { agent, .. } => Some(agent),
            _ => None,
        })
        .collect();
    assert_eq!(prompted.len(), 23);
    for index in 0..22 {
        let agent = if index == 0 {
            "aq27-agent".to_owned()
        } else {
            format!("aq27-agent-{index:02}")
        };
        let expected = usize::from(index == 0) + 1;
        assert_eq!(
            prompted
                .iter()
                .filter(|prompted| **prompted == agent)
                .count(),
            expected,
            "prompt count for {agent}"
        );
    }
    assert_eq!(pump.stats().pending_members, 22);
    assert_eq!(pump.stats().prompted, 7);
    assert_eq!(pump.cursor_position(), 16);
}

#[test]
fn herdr_statuses_project_to_runtime_states() {
    assert_eq!(
        runtime_state(Some(HerdrAgentStatus::Idle)),
        RuntimeMemberState::Idle
    );
    assert_eq!(
        runtime_state(Some(HerdrAgentStatus::Done)),
        RuntimeMemberState::Idle
    );
    assert_eq!(
        runtime_state(Some(HerdrAgentStatus::Working)),
        RuntimeMemberState::Active
    );
    assert_eq!(
        runtime_state(Some(HerdrAgentStatus::Unknown)),
        RuntimeMemberState::Unknown
    );
}

struct UnusedMailStore;
impl atm_storage::contract::sealed::Sealed for UnusedMailStore {}
impl atm_storage::MessageStore for UnusedMailStore {
    fn save_message(&self, _message: &atm_storage::Message) -> Result<(), AtmError> {
        unreachable!("herdr candidate test never touches the mail store boundary")
    }

    fn save_messages_atomically(&self, _messages: &[atm_storage::Message]) -> Result<(), AtmError> {
        unreachable!("herdr candidate test never touches the mail store boundary")
    }

    fn load_message(
        &self,
        _key: &atm_storage::MessageKey,
    ) -> Result<Option<atm_storage::Message>, AtmError> {
        unreachable!("herdr candidate test never touches the mail store boundary")
    }

    fn list_messages(
        &self,
        _query: &atm_storage::MessageQuery,
    ) -> Result<Vec<atm_storage::Message>, AtmError> {
        unreachable!("herdr candidate test never touches the mail store boundary")
    }

    fn delete_message(&self, _key: &atm_storage::MessageKey) -> Result<(), AtmError> {
        unreachable!("herdr candidate test never touches the mail store boundary")
    }
}

struct CountingMessageStore {
    inner: Arc<dyn atm_storage::MessageStore + Send + Sync>,
    list_messages_calls: Arc<AtomicUsize>,
}

impl atm_storage::contract::sealed::Sealed for CountingMessageStore {}
impl atm_storage::MessageStore for CountingMessageStore {
    fn save_message(&self, message: &atm_storage::Message) -> Result<(), AtmError> {
        self.inner.save_message(message)
    }

    fn save_messages_atomically(&self, messages: &[atm_storage::Message]) -> Result<(), AtmError> {
        self.inner.save_messages_atomically(messages)
    }

    fn load_message(
        &self,
        key: &atm_storage::MessageKey,
    ) -> Result<Option<atm_storage::Message>, AtmError> {
        self.inner.load_message(key)
    }

    fn list_messages(
        &self,
        query: &atm_storage::MessageQuery,
    ) -> Result<Vec<atm_storage::Message>, AtmError> {
        self.list_messages_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.list_messages(query)
    }

    fn delete_message(&self, key: &atm_storage::MessageKey) -> Result<(), AtmError> {
        self.inner.delete_message(key)
    }
}

struct NoopNudgeTemplateOverrideStore;
impl atm_storage::contract::sealed::Sealed for NoopNudgeTemplateOverrideStore {}
impl atm_core::boundary::NudgeTemplateOverrideStore for NoopNudgeTemplateOverrideStore {
    fn list_stale_template_override_kinds(
        &self,
        _team: &TeamName,
    ) -> Result<Vec<atm_core::boundary::StaleNudgeTemplateOverrideKind>, AtmError> {
        Ok(Vec::new())
    }

    fn list_template_overrides(
        &self,
        _team: &TeamName,
    ) -> Result<Vec<atm_core::boundary::TeamNudgeTemplateOverrideRow>, AtmError> {
        Ok(Vec::new())
    }

    fn load_template_override(
        &self,
        _team: &TeamName,
        _kind: atm_core::boundary::BuiltInNudgeTemplateKind,
    ) -> Result<Option<atm_core::boundary::TeamNudgeTemplateOverrideRow>, AtmError> {
        Ok(None)
    }

    fn save_template_override(
        &self,
        _team: &TeamName,
        _kind: atm_core::boundary::BuiltInNudgeTemplateKind,
        _template_body: &str,
    ) -> Result<atm_core::boundary::TeamNudgeTemplateOverrideRow, AtmError> {
        unreachable!("herdr candidate test never touches the override-store boundary")
    }

    fn disable_template_override(
        &self,
        _team: &TeamName,
        _kind: atm_core::boundary::BuiltInNudgeTemplateKind,
    ) -> Result<atm_core::boundary::TeamNudgeTemplateOverrideRow, AtmError> {
        unreachable!("herdr candidate test never touches the override-store boundary")
    }

    fn clear_template_override(&self, _team: &TeamName, _kind: &str) -> Result<bool, AtmError> {
        unreachable!("herdr candidate test never touches the override-store boundary")
    }
}

struct UnusedNonClaudeOutbound;
impl atm_core::boundary::sealed::Sealed for UnusedNonClaudeOutbound {}
impl atm_core::boundary::NonClaudeOutbound for UnusedNonClaudeOutbound {
    fn deliver_payloads(
        &self,
        _request: atm_core::boundary::NonClaudeOutboundDeliveryRequest,
    ) -> Result<atm_core::boundary::NonClaudeOutboundDeliveryResponse, AtmError> {
        unreachable!("herdr candidate test never touches the non-Claude outbound boundary")
    }
}

/// Durable roster store that counts every call so the test can prove
/// `herdr_candidates` reads exclusively from the RAM roster after the
/// one hydration read performed at runtime construction.
struct CountingRosterStore {
    roster: RosterSnapshot,
    load_roster_calls: std::sync::atomic::AtomicUsize,
    list_teams_calls: std::sync::atomic::AtomicUsize,
}

impl atm_storage::contract::sealed::Sealed for CountingRosterStore {}
impl atm_storage::contract::RosterStore for CountingRosterStore {
    fn load_roster(&self, team: &TeamName) -> Result<RosterSnapshot, AtmError> {
        self.load_roster_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(team, &self.roster.team_name, "unexpected team requested");
        Ok(self.roster.clone())
    }

    fn save_roster(&self, _roster: &RosterSnapshot) -> Result<(), AtmError> {
        unreachable!("herdr candidate test never mutates the durable roster")
    }

    fn list_teams(&self) -> Result<Vec<TeamName>, AtmError> {
        self.list_teams_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(vec![self.roster.team_name.clone()])
    }
}

#[test]
fn herdr_candidates_never_reads_the_durable_roster_store_after_hydration() {
    let team: TeamName = "aq27-counting-team".parse().expect("team");
    let member = herdr_member(&team, "aq27-counting-agent");
    let durable = std::sync::Arc::new(CountingRosterStore {
        roster: RosterSnapshot {
            team_name: team.clone(),
            members: vec![member],
            refreshed_at: None,
        },
        load_roster_calls: std::sync::atomic::AtomicUsize::new(0),
        list_teams_calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let roster = atm_runtime_test_support::build_write_through_roster_for_test(durable.clone())
        .expect("write-through roster fixture hydrates from the counting fake");
    let runtime = LocalServiceRuntime::new_with_delivery_boundaries(
        std::sync::Arc::new(UnusedMailStore),
        roster,
        std::sync::Arc::new(NoopNudgeTemplateOverrideStore),
        std::sync::Arc::new(UnusedNonClaudeOutbound),
    );
    // Hydration (now performed by build_write_through_roster_for_test)
    // performs exactly one read of each kind.
    assert_eq!(
        durable
            .list_teams_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(
        durable
            .load_roster_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );

    let pending = std::collections::HashSet::new();
    for _ in 0..5 {
        let roster_store = runtime.shared_roster_store_arc();
        let candidates =
            super::herdr_candidates(roster_store.as_ref(), &pending).expect("candidates");
        assert_eq!(candidates.len(), 1);
    }

    // Five additional herdr_candidates calls must not touch the durable
    // store again: RAM is the only read path once hydrated.
    assert_eq!(
        durable
            .list_teams_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        1,
        "herdr_candidates must not call list_teams on the durable store"
    );
    assert_eq!(
        durable
            .load_roster_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        1,
        "herdr_candidates must not call load_roster on the durable store"
    );
}

#[test]
fn herdr_queue_wake_skips_a_nonconforming_canonical_name_without_panicking() {
    let team: TeamName = "aq27-invalid-herdr-name".parse().expect("team");
    let member = herdr_member(&team, "TeamLead");
    let durable = std::sync::Arc::new(CountingRosterStore {
        roster: RosterSnapshot {
            team_name: team,
            members: vec![member],
            refreshed_at: None,
        },
        load_roster_calls: std::sync::atomic::AtomicUsize::new(0),
        list_teams_calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let roster = atm_runtime_test_support::build_write_through_roster_for_test(durable)
        .expect("write-through roster fixture hydrates");
    let runtime = LocalServiceRuntime::new_with_delivery_boundaries(
        std::sync::Arc::new(UnusedMailStore),
        roster,
        std::sync::Arc::new(NoopNudgeTemplateOverrideStore),
        std::sync::Arc::new(UnusedNonClaudeOutbound),
    );

    let pending = std::collections::HashSet::new();
    let candidates = super::herdr_candidates(runtime.shared_roster_store_arc().as_ref(), &pending)
        .expect("invalid Herdr fallback is skipped, not surfaced as a failure");

    assert!(candidates.is_empty());
}
