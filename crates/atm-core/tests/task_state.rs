//! End-to-end task state coverage through the public core write pipeline.

use atm_core::ack::{AckRequest, ack_mail_with_runtime};
use atm_core::boundary::{RosterEntry, RosterHarness, RosterMemberKind};
use atm_core::observability::NullObservability;
use atm_core::schema::AtmMessageId;
use atm_core::send::{
    NudgeMode, SendMessageSource, WriteRequest, prepare_write_with_runtime, write_mail_with_runtime,
};
use atm_core::types::{AgentName, IsoTimestamp, ModelName, PaneId, TaskId, TeamName};
use atm_storage::{RosterSnapshot, TaskEventKind, TaskState};

fn setup(harness: RosterHarness) -> (tempfile::TempDir, atm_core::LocalServiceRuntime, TeamName) {
    let root = tempfile::tempdir().expect("temporary task-state root");
    let assembly = atm_runtime_test_support::open_isolated_sqlite_boundary(root.path())
        .expect("sqlite runtime");
    let team: TeamName = "task-state-team".parse().expect("team");
    assembly
        .service_runtime
        .shared_roster_store_arc()
        .save_roster(&RosterSnapshot {
            team_name: team.clone(),
            members: vec![
                member(&team, "sender", harness),
                member(&team, "recipient", harness),
            ],
            refreshed_at: None,
        })
        .expect("seed roster");
    (root, assembly.service_runtime, team)
}

fn member(team: &TeamName, agent: &str, harness: RosterHarness) -> RosterEntry {
    RosterEntry {
        team_name: team.clone(),
        agent_name: agent.parse().expect("agent"),
        member_kind: RosterMemberKind::Permanent,
        harness,
        agent_type: atm_core::schema::AgentType::default(),
        model: ModelName::default(),
        recipient_pane_id: Some(PaneId::from_cli("%9").expect("pane")),
        metadata_json: serde_json::Map::new(),
    }
}

fn task_write_request(
    home: &std::path::Path,
    team: &TeamName,
    sender: &str,
    task_id: TaskId,
) -> (WriteRequest, AtmMessageId) {
    let message_id = AtmMessageId::new();
    let request = WriteRequest::new(
        home.to_path_buf(),
        home.to_path_buf(),
        sender.parse::<AgentName>().expect("sender"),
        "recipient@task-state-team",
        team.clone(),
        SendMessageSource::Inline("complete the end-to-end task test".to_owned()),
        None,
        true,
        None,
        false,
    )
    .expect("request")
    .with_nudge_mode(NudgeMode::Immediate)
    .with_origin_metadata(message_id, IsoTimestamp::now());
    let mut request = request;
    request.task_id = Some(task_id);
    (request, message_id)
}

fn ack_request(home: &std::path::Path, team: &TeamName, message_id: AtmMessageId) -> AckRequest {
    AckRequest {
        home_dir: home.to_path_buf(),
        current_dir: home.to_path_buf(),
        caller_identity: "recipient".parse().expect("recipient"),
        caller_chat_id: None,
        caller_team: team.clone(),
        activity_observation: None,
        message_id,
        reply_body: "acknowledged".to_owned(),
    }
}

fn task_row(
    runtime: &atm_core::LocalServiceRuntime,
    team: &TeamName,
    task_id: &TaskId,
) -> atm_storage::TaskRow {
    let member = atm_storage::MemberKey::new(
        team.clone(),
        "recipient".parse::<AgentName>().expect("recipient"),
    );
    runtime
        .task_store()
        .expect("installed task store")
        .load_task(&member, task_id)
        .expect("load task")
        .expect("task row")
}

#[test]
fn synchronous_tmux_task_write_acknowledgement_and_completion_reach_the_task_store() {
    let (root, runtime, team) = setup(RosterHarness::ClaudeCode);
    let home = root.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let task_id: TaskId = "AX3-E2E".parse().expect("task id");
    let (request, assignment_id) = task_write_request(&home, &team, "sender", task_id.clone());

    write_mail_with_runtime(request, &NullObservability, &runtime).expect("task write");
    assert_eq!(
        task_row(&runtime, &team, &task_id).state,
        TaskState::Assigned
    );

    ack_mail_with_runtime(
        ack_request(&home, &team, assignment_id),
        &NullObservability,
        &runtime,
    )
    .expect("task acknowledgement");
    assert_eq!(task_row(&runtime, &team, &task_id).state, TaskState::Active);

    let (mut completion, _) = task_write_request(&home, &team, "sender", task_id.clone());
    completion.task_id = None;
    completion.task_complete = Some(task_id.clone());
    write_mail_with_runtime(completion, &NullObservability, &runtime).expect("task completion");

    let row = task_row(&runtime, &team, &task_id);
    assert_eq!(row.state, TaskState::Complete);
    let events = runtime
        .task_store()
        .expect("installed task store")
        .list_task_events(&team, &task_id, Some(&row.assignee))
        .expect("task events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event == TaskEventKind::Acked)
            .count(),
        1,
        "one successful acknowledgement appends exactly one Acked event"
    );
}

#[test]
fn deferred_herdr_prepare_persists_the_same_task_assignment() {
    let (root, runtime, team) = setup(RosterHarness::Hermes);
    let home = root.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let task_id: TaskId = "AX3-ASYNC".parse().expect("task id");
    let (request, assignment_id) = task_write_request(&home, &team, "sender", task_id.clone());

    let mut prepared = prepare_write_with_runtime(request, &NullObservability, &runtime)
        .expect("deferred task write");
    prepared
        .finish(&runtime, &NullObservability)
        .expect("finish write");
    assert_eq!(
        task_row(&runtime, &team, &task_id).state,
        TaskState::Assigned
    );

    ack_mail_with_runtime(
        ack_request(&home, &team, assignment_id),
        &NullObservability,
        &runtime,
    )
    .expect("task acknowledgement");
    assert_eq!(task_row(&runtime, &team, &task_id).state, TaskState::Active);
}
