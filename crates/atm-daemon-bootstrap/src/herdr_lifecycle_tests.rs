//! AY4 real-runtime lifecycle fixtures.
//!
//! These tests deliberately compose the production Tokio queue-wake pump with
//! the isolated SQLite runtime. They use AY2's fake only at the Herdr trait
//! boundary, never a legacy daemon or a live ATM database.

use std::sync::Arc;
use std::time::Duration;

use atm_core::LocalServiceRuntime;
use atm_core::boundary::{BuiltInPostSendDispatch, MessageReceivedHookSelector, RosterEntry};
use atm_core::delivery_channel::test_backend_type_metadata;
use atm_core::observability::NullObservability;
use atm_core::send::{NudgeMode, SendMessageSource, WriteRequest, write_mail_with_runtime};
use atm_core::types::{AgentName, ModelName, TeamName};
use atm_herdr::{
    AgentSnapshot, HerdrAgentStatus, HerdrError, HerdrListOutcome, HerdrProcessAdapter,
};
use atm_http_runtime::{HerdrQueueWakePump, RuntimeHealth};
use atm_runtime_test_support::open_isolated_sqlite_boundary;
use atm_storage::{MessageQuery, MessageStore, RosterHarness, RosterMemberKind, RosterSnapshot};

struct NoopSelector;

impl atm_core::boundary::sealed::Sealed for NoopSelector {}

impl MessageReceivedHookSelector for NoopSelector {
    fn select_emitter(
        &self,
        _dispatch: &BuiltInPostSendDispatch,
    ) -> Option<&dyn atm_core::boundary::AsyncMessageReceivedHookEmitter> {
        None
    }
}

struct Fixture {
    root: tempfile::TempDir,
    runtime: LocalServiceRuntime,
    messages: Arc<dyn MessageStore + Send + Sync>,
    fake: Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
    pump: HerdrQueueWakePump,
    team: TeamName,
    worker: AgentName,
    lead: AgentName,
}

fn herdr_member(team: &TeamName, agent: AgentName) -> RosterEntry {
    RosterEntry {
        team_name: team.clone(),
        agent_name: agent,
        member_kind: RosterMemberKind::Permanent,
        harness: RosterHarness::CodexCli,
        agent_type: atm_storage::AgentType::Worker,
        model: ModelName::default(),
        recipient_pane_id: None,
        metadata_json: test_backend_type_metadata("herdr"),
    }
}

fn fixture() -> Fixture {
    let root = tempfile::tempdir().expect("temporary AY4 lifecycle root");
    let assembly = open_isolated_sqlite_boundary(root.path()).expect("isolated runtime");
    let runtime = assembly.service_runtime.clone();
    let messages = assembly.message_store_arc();
    let team: TeamName = "ay4-lifecycle".parse().expect("team");
    let worker: AgentName = "worker".parse().expect("worker");
    let lead: AgentName = "lead".parse().expect("lead");
    let mut lead_entry = herdr_member(&team, lead.clone());
    lead_entry.agent_type = atm_storage::AgentType::Lead;
    runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: team.clone(),
            members: vec![herdr_member(&team, worker.clone()), lead_entry],
            refreshed_at: None,
        })
        .expect("seed Herdr roster");
    let home = root.path().join("home");
    std::fs::create_dir_all(&home).expect("test home");
    write_mail_with_runtime(
        WriteRequest::new(
            home.clone(),
            home.clone(),
            "sender".parse().expect("sender"),
            &format!("{worker}@{team}"),
            team.clone(),
            SendMessageSource::Inline("AY4 pending work".to_owned()),
            None,
            false,
            None,
            false,
        )
        .expect("pending work")
        .with_nudge_mode(NudgeMode::Deferred),
        &NullObservability,
        &runtime,
    )
    .expect("persist pending work");
    let fake = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
    let process: Arc<dyn HerdrProcessAdapter> = fake.clone();
    let pump = HerdrQueueWakePump::new(
        runtime.clone(),
        Arc::new(NoopSelector),
        RuntimeHealth::default(),
        process,
    )
    .with_daemon_home(home);
    Fixture {
        root,
        runtime,
        messages,
        fake,
        pump,
        team,
        worker,
        lead,
    }
}

fn idle(worker: &AgentName) -> HerdrListOutcome {
    HerdrListOutcome {
        agents: vec![AgentSnapshot {
            name: Some(worker.to_string()),
            status: HerdrAgentStatus::Idle,
            workspace_id: None,
        }],
    }
}

fn notify_count(fake: &atm_herdr::testing::FakeHerdrProcessAdapter) -> usize {
    fake.calls()
        .iter()
        .filter(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Notify { .. }))
        .count()
}

fn lead_mail_count(fixture: &Fixture) -> usize {
    fixture
        .messages
        .list_messages(&MessageQuery {
            team: fixture.team.clone(),
            agent: fixture.lead.clone(),
            sender: None,
            task_id: None,
            limit: None,
        })
        .expect("inspect isolated lead inbox")
        .len()
}

fn outage() -> HerdrError {
    HerdrError::ServerUnavailable {
        message: "test outage".to_owned(),
        retry_after: None,
        io_error_kind: None,
    }
}

#[tokio::test]
async fn ay4_l1_l2_missing_or_unavailable_herdr_does_not_stop_queue_wake() {
    let fixture = fixture();
    fixture.pump.tick_once().await;
    assert_eq!(notify_count(&fixture.fake), 0, "empty fake is optional");

    fixture.fake.queue_list_result(Err(outage()));
    fixture.pump.tick_once().await;
    assert_eq!(notify_count(&fixture.fake), 1, "one open cycle escalates");
    assert_eq!(lead_mail_count(&fixture), 1, "lead mail remains durable");
}

#[tokio::test]
async fn ay4_l3_l9_protocol_failure_recovers_on_the_first_success() {
    let fixture = fixture();
    fixture
        .fake
        .queue_list_result(Err(HerdrError::ProtocolMismatch {
            message: "test mismatch".to_owned(),
        }));
    fixture.pump.tick_once().await;
    assert_eq!(notify_count(&fixture.fake), 1);

    fixture.fake.queue_list_result(Ok(idle(&fixture.worker)));
    fixture.pump.tick_once().await;
    assert_eq!(
        notify_count(&fixture.fake),
        1,
        "first successful list closes the failed cycle without another escalation"
    );
}

#[tokio::test]
async fn ay4_l7_notification_failure_keeps_durable_lead_mail() {
    let fixture = fixture();
    fixture.fake.queue_notify_result(Err(outage()));
    fixture.fake.queue_list_result(Err(outage()));
    fixture.pump.tick_once().await;

    assert_eq!(
        notify_count(&fixture.fake),
        1,
        "one best-effort notification"
    );
    assert_eq!(
        lead_mail_count(&fixture),
        1,
        "notification failure cannot roll back mail"
    );
}

#[tokio::test(start_paused = true)]
async fn ay4_l12_notification_stall_times_out_without_losing_durable_mail() {
    let fixture = fixture();
    let _notify_gate = fixture.fake.block_next_notify();
    fixture.fake.queue_list_result(Err(outage()));

    fixture.pump.tick_once().await;

    assert_eq!(
        notify_count(&fixture.fake),
        1,
        "the stalled notification was attempted once"
    );
    assert_eq!(
        lead_mail_count(&fixture),
        1,
        "the five-second notification deadline cannot roll back durable lead mail"
    );
}

#[tokio::test]
async fn ay4_l10_l11_flapping_is_suppressed_but_restart_gets_one_new_claim() {
    let fixture = fixture();
    fixture.fake.queue_list_result(Err(outage()));
    fixture.pump.tick_once().await;
    fixture.fake.queue_list_result(Ok(idle(&fixture.worker)));
    fixture.pump.tick_once().await;
    fixture.fake.queue_list_result(Err(outage()));
    fixture.pump.tick_once().await;
    assert_eq!(
        notify_count(&fixture.fake),
        1,
        "flapping stays inside cooldown"
    );

    let process: Arc<dyn HerdrProcessAdapter> = fixture.fake.clone();
    let restarted = HerdrQueueWakePump::new(
        fixture.runtime.clone(),
        Arc::new(NoopSelector),
        RuntimeHealth::default(),
        process,
    )
    .with_daemon_home(fixture.root.path().join("home"));
    fixture.fake.queue_list_result(Err(outage()));
    restarted.tick_once().await;
    assert_eq!(
        notify_count(&fixture.fake),
        2,
        "restart owns one fresh in-memory claim"
    );
}

#[tokio::test]
async fn ay4_l5_shutdown_stops_new_queue_wake_admissions() {
    let fixture = fixture();
    let list_gate = fixture.fake.block_next_list();
    let (shutdown, receiver) = tokio::sync::watch::channel(());
    let task = Arc::new(fixture.pump.clone()).start(receiver);
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if fixture
                .fake
                .calls()
                .iter()
                .any(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::List { .. }))
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("queue wake began its bounded list request");
    shutdown.send(()).expect("shutdown signal");
    list_gate.notify_waiters();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("queue wake joins after completing in-flight work")
        .expect("queue wake task join");
    assert_eq!(
        notify_count(&fixture.fake),
        0,
        "shutdown admits no escalation"
    );
}
