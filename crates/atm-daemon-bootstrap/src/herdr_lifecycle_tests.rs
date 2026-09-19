//! AY4 real-runtime lifecycle fixtures.
//!
//! These tests deliberately compose the production Tokio queue-wake pump with
//! the isolated SQLite runtime. They use AY2's fake only at the Herdr trait
//! boundary, never a legacy daemon or a live ATM database.

use std::sync::Arc;
use std::time::Duration;

use crate::active_received_hook_selector_with_health;
use atm_core::LocalServiceRuntime;
use atm_core::boundary::RosterEntry;
use atm_core::delivery_channel::test_backend_type_metadata;
use atm_core::observability::NullObservability;
use atm_core::send::{NudgeMode, SendMessageSource, WriteRequest, write_mail_with_runtime};
use atm_core::types::{AgentName, ModelName, TeamName};
use atm_herdr::{
    AgentSnapshot, HerdrAgentStatus, HerdrError, HerdrListOutcome, HerdrProcessAdapter,
};
use atm_http_runtime::{HerdrQueueWakePump, RuntimeHealth};
use atm_runtime_test_support::open_isolated_sqlite_boundary;
use atm_storage::{RosterHarness, RosterMemberKind, RosterSnapshot};

struct Fixture {
    _root: tempfile::TempDir,
    runtime: LocalServiceRuntime,
    fake: Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
    pump: HerdrQueueWakePump,
    team: TeamName,
    worker: AgentName,
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
        active_received_hook_selector_with_health(
            runtime.clone(),
            Arc::clone(&process),
            RuntimeHealth::default(),
        ),
        RuntimeHealth::default(),
        process,
    )
    .with_daemon_home(home);
    Fixture {
        _root: root,
        runtime,
        fake,
        pump,
        team,
        worker,
    }
}

fn idle(worker: &AgentName) -> HerdrListOutcome {
    HerdrListOutcome {
        agents: vec![AgentSnapshot {
            name: Some(worker.to_string()),
            pane_id: None,
            status: HerdrAgentStatus::Idle,
            workspace_id: None,
        }],
    }
}

fn prompt_count(fake: &atm_herdr::testing::FakeHerdrProcessAdapter) -> usize {
    fake.calls()
        .iter()
        .filter(|call| matches!(call, atm_herdr::testing::FakeHerdrCall::Prompt { .. }))
        .count()
}

fn pending_worker(fixture: &Fixture) -> bool {
    fixture
        .runtime
        .pending_nudge_store()
        .expect("pending store")
        .list_pending_members()
        .expect("list pending members")
        .iter()
        .any(|member| member.team() == &fixture.team && member.agent() == &fixture.worker)
}

#[tokio::test]
async fn ay4_l4_connection_reset_keeps_unknown_prompt_pending_without_duplicate_submission() {
    let fixture = fixture();
    fixture.fake.queue_list_result(Ok(idle(&fixture.worker)));
    fixture
        .fake
        .queue_prompt_result(Err(HerdrError::ServerUnavailable {
            message: "connection reset after prompt submission".to_owned(),
            retry_after: None,
            io_error_kind: Some(std::io::ErrorKind::ConnectionReset),
        }));

    fixture.pump.tick_once().await;

    assert_eq!(
        prompt_count(&fixture.fake),
        1,
        "a connection reset leaves one unknown prompt submission, never a duplicate"
    );
    assert!(
        pending_worker(&fixture),
        "unknown prompt keeps durable mail pending"
    );

    fixture.fake.queue_list_result(Ok(idle(&fixture.worker)));
    fixture.pump.tick_once().await;
    assert_eq!(
        prompt_count(&fixture.fake),
        2,
        "a later queue-wake admission re-nudges the retained pending mail"
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
}
