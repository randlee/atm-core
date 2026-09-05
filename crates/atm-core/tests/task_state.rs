//! End-to-end task state coverage through the public core write pipeline.

use atm_core::ack::{AckRequest, ack_mail_with_runtime};
use atm_core::boundary::{
    LocalSteerTarget, NudgeKind, PostSendBuiltInTarget, RosterEntry, RosterHarness,
    RosterMemberKind,
};
use atm_core::nudge_dispatch::rebuild_received_hook_dispatch;
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
    let (recipient_pane_id, metadata_json) = match harness {
        RosterHarness::Hermes => {
            let mut metadata = atm_core::delivery_channel::test_backend_type_metadata("herdr");
            metadata.insert("herdrSession".to_owned(), serde_json::json!("ax3-herdr"));
            (None, metadata)
        }
        _ => (
            Some(PaneId::from_cli("%9").expect("pane")),
            serde_json::Map::new(),
        ),
    };
    RosterEntry {
        team_name: team.clone(),
        agent_name: agent.parse().expect("agent"),
        member_kind: RosterMemberKind::Permanent,
        harness,
        agent_type: atm_core::schema::AgentType::default(),
        model: ModelName::default(),
        recipient_pane_id,
        metadata_json,
    }
}

fn task_write_request(
    home: &std::path::Path,
    team: &TeamName,
    sender: &str,
    task_id: TaskId,
) -> (WriteRequest, AtmMessageId) {
    task_write_request_with_mode(home, team, sender, task_id, NudgeMode::Immediate)
}

fn task_write_request_with_mode(
    home: &std::path::Path,
    team: &TeamName,
    sender: &str,
    task_id: TaskId,
    nudge_mode: NudgeMode,
) -> (WriteRequest, AtmMessageId) {
    let message_id = AtmMessageId::new();
    let request = WriteRequest::new(
        home.to_path_buf(),
        home.to_path_buf(),
        sender.parse::<AgentName>().expect("sender"),
        &format!("recipient@{team}"),
        team.clone(),
        SendMessageSource::Inline("complete the end-to-end task test".to_owned()),
        None,
        true,
        None,
        false,
    )
    .expect("request")
    .with_nudge_mode(nudge_mode)
    .with_origin_metadata(message_id, IsoTimestamp::now());
    let mut request = request;
    request.task_id = Some(task_id);
    (request, message_id)
}

fn assert_task_send_surface(harness: RosterHarness, nudge_mode: NudgeMode, task_id: &str) {
    let (root, runtime, team) = setup(harness);
    let home = root.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let task_id = task_id.parse::<TaskId>().expect("task id");
    let (request, _) =
        task_write_request_with_mode(&home, &team, "sender", task_id.clone(), nudge_mode);
    let outcome =
        write_mail_with_runtime(request, &NullObservability, &runtime).expect("task write");

    assert_eq!(
        task_row(&runtime, &team, &task_id).state,
        TaskState::Assigned
    );
    let member = atm_storage::MemberKey::new(
        team.clone(),
        "recipient".parse::<AgentName>().expect("recipient"),
    );
    assert!(
        runtime
            .pending_nudge_store()
            .expect("pending store")
            .list_pending_members()
            .expect("pending members")
            .contains(&member)
    );

    let dispatch = rebuild_received_hook_dispatch(
        &runtime,
        &member,
        outcome.persisted_message_id(),
        NudgeKind::Queue,
    )
    .expect("rebuild task dispatch")
    .expect("task dispatch");
    assert_eq!(dispatch.event.task_id, Some(task_id.clone()));
    let rendered = match &dispatch.target {
        PostSendBuiltInTarget::LocalSteer(LocalSteerTarget::Herdr(target)) => {
            &target.rendered_nudge
        }
        PostSendBuiltInTarget::LocalSteer(LocalSteerTarget::Tmux(target)) => &target.rendered_nudge,
        target => panic!("unexpected task target: {target:?}"),
    };
    assert!(rendered.contains(&format!("<task id=\"{}\">", task_id.as_str())));
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
    let roster = runtime
        .shared_roster_store_arc()
        .load_roster(&team)
        .expect("load roster");
    let recipient = roster
        .members
        .iter()
        .find(|member| member.agent_name.as_str() == "recipient")
        .expect("recipient roster member");
    assert!(matches!(
        atm_core::delivery_channel::local_message_received_backend(recipient),
        Some(atm_core::delivery_channel::LocalMessageReceivedBackend::Herdr { .. })
    ));
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

#[test]
fn ac03_send_task_to_herdr_sets_marker_and_renders_task_body() {
    assert_task_send_surface(
        RosterHarness::Hermes,
        NudgeMode::Immediate,
        "AX5-SEND-HERDR",
    );
}

#[test]
fn ac03_queue_task_to_herdr_sets_marker_and_renders_task_body() {
    assert_task_send_surface(
        RosterHarness::Hermes,
        NudgeMode::Deferred,
        "AX5-QUEUE-HERDR",
    );
}

#[test]
fn ac03_send_task_to_tmux_sets_marker_and_renders_task_body() {
    assert_task_send_surface(
        RosterHarness::ClaudeCode,
        NudgeMode::Immediate,
        "AX5-SEND-TMUX",
    );
}

#[test]
fn ac03_queue_task_to_tmux_sets_marker_and_renders_task_body() {
    assert_task_send_surface(
        RosterHarness::ClaudeCode,
        NudgeMode::Deferred,
        "AX5-QUEUE-TMUX",
    );
}
