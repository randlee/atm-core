#![cfg(test)]

use atm_core::schema::AtmMessageId;
use atm_core::test_support::{
    TEST_LEAD, TEST_LEAD_ADDRESS, TEST_RECIPIENT_ADDRESS, TEST_SENDER, TEST_SENDER_ADDRESS,
    TEST_TEAM,
};
use atm_storage::{
    MemberKey, Message, MessageEnvelope, MessageKey, ReminderOutcome, TaskEventKind, TaskOp,
    TaskState, TeamName,
};
use serial_test::serial;

use super::*;
use crate::composition::tests::LoopbackFixture;

fn caller(actor: &str) -> CallerArgs {
    CallerArgs {
        actor: Some(actor.to_owned()),
        team: Some(TEST_TEAM.to_owned()),
    }
}

fn assign(task: &str, assignee: &str, actor: &str) -> TaskAssignCommand {
    TaskAssignCommand {
        assignee: assignee.parse().expect("address"),
        task_id: Some(task.parse().expect("task id")),
        before: None,
        head: false,
        preempt: false,
        message: MessageSourceArgs {
            text: Some(format!("assignment {task}")),
            file: None,
            stdin: false,
            template: None,
            vars: None,
        },
        json: true,
        caller: caller(actor),
    }
}

async fn execute_assign(fixture: &LoopbackFixture, command: TaskAssignCommand) -> String {
    let observability = CliObservability::fallback();
    let composition = fixture.composition(&observability);
    command
        .execute(
            &composition,
            fixture.home_dir.clone(),
            fixture.current_dir.clone(),
        )
        .await
        .expect("assignment command")
}

fn seed_assignment(fixture: &LoopbackFixture, task_id: &str, assignee: &str, assigner: &str) {
    let message_id = AtmMessageId::new();
    fixture
        .message_store()
        .save_message(&Message {
            team: TEST_TEAM.parse().expect("team"),
            agent: assignee.parse().expect("assignee"),
            message_key: MessageKey::from(message_id),
            envelope: MessageEnvelope {
                from: assigner.parse().expect("assigner"),
                source_chat_id: None,
                text: format!("assign {task_id}"),
                timestamp: atm_storage::IsoTimestamp::now(),
                read: false,
                source_team: Some(TEST_TEAM.parse().expect("source team")),
                destination_chat_id: None,
                summary: None,
                message_id: Some(message_id),
                requires_ack: false,
                pending_ack_at: None,
                acknowledged_at: None,
                acknowledges_message_id: None,
                parent_message_id: None,
                thread_mode: None,
                expires_at: None,
                task_id: Some(task_id.parse().expect("task id")),
                placement: None,
                task_op: None,
                task_complete: None,
                extra: serde_json::Map::new(),
            },
        })
        .expect("seed assignment");
}

#[tokio::test]
#[serial(env)]
async fn non_assigner_reassign_returns_structured_cli_error() {
    let fixture = LoopbackFixture::new("recipient");
    seed_assignment(&fixture, "EQ008-CLI", "recipient", TEST_LEAD);
    let observability = CliObservability::fallback();
    let composition = fixture.composition(&observability);

    let error = assign("EQ008-CLI", TEST_RECIPIENT_ADDRESS, TEST_SENDER)
        .execute(
            &composition,
            fixture.home_dir.clone(),
            fixture.current_dir.clone(),
        )
        .await
        .expect_err("non-assigner CLI reassignment must fail");
    let error = error
        .downcast_ref::<atm_core::error::AtmError>()
        .expect("typed task refusal");
    assert_eq!(error.code(), atm_storage::AtmErrorCode::TaskNotCounterparty);
    assert!(error.detail().contains("task EQ008-CLI"));
    assert!(error.detail().contains(TEST_LEAD));
    assert!(error.detail().contains(TEST_SENDER));
}

#[tokio::test]
#[serial(env)]
async fn assign_to_active_member_persists_with_zero_prompts_until_idle() {
    let fixture = LoopbackFixture::new("recipient");
    execute_assign(
        &fixture,
        assign("ACTIVE", TEST_RECIPIENT_ADDRESS, TEST_SENDER),
    )
    .await;
    let store = fixture.task_store();
    let team: TeamName = TEST_TEAM.parse().expect("team");
    let member = MemberKey::new(team.clone(), "recipient".parse().expect("recipient"));
    store
        .record_reminder(
            &member,
            &"ACTIVE".parse().expect("task"),
            atm_storage::IsoTimestamp::now(),
            ReminderOutcome::Emitted,
        )
        .expect("active reminder");
    let mut start_active = atm_core::send::SendRequest::new(
        fixture.home_dir.clone(),
        fixture.current_dir.clone(),
        "recipient".parse().unwrap(),
        TEST_SENDER_ADDRESS,
        team.clone(),
        atm_core::send::SendMessageSource::Inline("start ACTIVE".into()),
        None,
        false,
        Some("ACTIVE".parse().unwrap()),
        false,
    )
    .unwrap();
    start_active.task_op = Some(TaskOp::Start);
    let observability = CliObservability::fallback();
    fixture
        .composition(&observability)
        .send(start_active)
        .await
        .unwrap();
    execute_assign(
        &fixture,
        assign("QUEUED", TEST_RECIPIENT_ADDRESS, TEST_SENDER),
    )
    .await;
    for _ in 0..20 {
        let queued = store
            .load_task(&team, &"QUEUED".parse().expect("task"))
            .expect("load")
            .expect("queued row");
        assert_eq!(queued.state, TaskState::Assigned);
        assert_eq!(queued.reminder_count, 0);
        assert!(queued.last_reminded_at.is_none());
    }
    let mut close_active = atm_core::send::SendRequest::new(
        fixture.home_dir.clone(),
        fixture.current_dir.clone(),
        "recipient".parse().unwrap(),
        TEST_SENDER_ADDRESS,
        team.clone(),
        atm_core::send::SendMessageSource::Inline("active work finished".into()),
        None,
        false,
        Some("ACTIVE".parse().unwrap()),
        false,
    )
    .unwrap();
    close_active.task_op = Some(TaskOp::Close {
        outcome: atm_storage::TaskCloseOutcome::Completed,
        reason: Some("active work finished".into()),
    });
    let observability = CliObservability::fallback();
    fixture
        .composition(&observability)
        .send(close_active)
        .await
        .unwrap();
    store
        .record_reminder(
            &member,
            &"QUEUED".parse().expect("task"),
            atm_storage::IsoTimestamp::now(),
            ReminderOutcome::Emitted,
        )
        .expect("idle-transition reminder");
    let mut start = atm_core::send::SendRequest::new(
        fixture.home_dir.clone(),
        fixture.current_dir.clone(),
        "recipient".parse().unwrap(),
        TEST_SENDER_ADDRESS,
        team.clone(),
        atm_core::send::SendMessageSource::Inline("start".into()),
        None,
        false,
        Some("QUEUED".parse().unwrap()),
        false,
    )
    .unwrap();
    start.task_op = Some(TaskOp::Start);
    let observability = CliObservability::fallback();
    fixture
        .composition(&observability)
        .send(start)
        .await
        .unwrap();
    let active = store
        .load_task(&team, &"QUEUED".parse().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(active.state, TaskState::Active);
    assert_eq!(active.reminder_count, 1);
}

#[tokio::test]
#[serial(env)]
async fn assign_existing_open_id_to_other_agent_reassigns_in_place() {
    let fixture = LoopbackFixture::new("recipient");
    execute_assign(&fixture, assign("T1", TEST_RECIPIENT_ADDRESS, TEST_SENDER)).await;
    execute_assign(&fixture, assign("T1", TEST_LEAD_ADDRESS, TEST_SENDER)).await;
    let store = fixture.task_store();
    let team = TEST_TEAM.parse().expect("team");
    let task = "T1".parse().expect("task");
    let row = store.load_task(&team, &task).unwrap().unwrap();
    assert_eq!(row.assignee.as_str(), TEST_LEAD);
    assert_eq!(
        store
            .list_task_events(&team, &task, None)
            .unwrap()
            .last()
            .unwrap()
            .event,
        TaskEventKind::Reassigned
    );
}

#[tokio::test]
#[serial(env)]
async fn assign_closed_id_reopens_same_row() {
    let fixture = LoopbackFixture::new("recipient");
    execute_assign(&fixture, assign("T1", TEST_RECIPIENT_ADDRESS, TEST_SENDER)).await;
    let observability = CliObservability::fallback();
    let composition = fixture.composition(&observability);
    TaskCloseCommand {
        task_id: "T1".parse().unwrap(),
        outcome: OutcomeArg::Refused,
        reason: Some("no capacity".into()),
        report: MessageSourceArgs {
            text: None,
            file: None,
            stdin: false,
            template: None,
            vars: None,
        },
        json: true,
        caller: caller(TEST_SENDER),
    }
    .execute(
        &composition,
        resolve_context(&caller(TEST_SENDER)).unwrap(),
        fixture.home_dir.clone(),
        fixture.current_dir.clone(),
    )
    .await
    .expect("close");
    execute_assign(&fixture, assign("T1", TEST_LEAD_ADDRESS, TEST_SENDER)).await;
    let store = fixture.task_store();
    let team = TEST_TEAM.parse().unwrap();
    let task = "T1".parse().unwrap();
    assert_eq!(
        store.load_task(&team, &task).unwrap().unwrap().state,
        TaskState::Assigned
    );
    assert_eq!(
        store
            .list_task_events(&team, &task, None)
            .unwrap()
            .last()
            .unwrap()
            .event,
        TaskEventKind::Reopened
    );
}

fn preempt(task: &str, assignee: &str, actor: &str) -> TaskAssignCommand {
    TaskAssignCommand {
        preempt: true,
        ..assign(task, assignee, actor)
    }
}

/// Sends the assignee's own `--task-id <task> --task-op start` write
/// directly, bypassing the CLI's `CallerArgs` identity-match guard (the
/// fixture's `ATM_IDENTITY` stays pinned to the assigner throughout these
/// tests, matching every `assign`/`preempt` call).
async fn start_task(fixture: &LoopbackFixture, team: &TeamName, task: &str) {
    let mut request = atm_core::send::SendRequest::new(
        fixture.home_dir.clone(),
        fixture.current_dir.clone(),
        "recipient".parse().unwrap(),
        TEST_SENDER_ADDRESS,
        team.clone(),
        atm_core::send::SendMessageSource::Inline(format!("start {task}")),
        None,
        false,
        Some(task.parse().expect("task id")),
        false,
    )
    .unwrap();
    request.task_op = Some(TaskOp::Start);
    let observability = CliObservability::fallback();
    fixture
        .composition(&observability)
        .send(request)
        .await
        .expect("start task");
}

/// Sends the assignee's own `--task-id <task> --task-op close` write
/// directly, for the same identity-match reason as [`start_task`].
async fn close_task(fixture: &LoopbackFixture, team: &TeamName, task: &str) {
    let mut request = atm_core::send::SendRequest::new(
        fixture.home_dir.clone(),
        fixture.current_dir.clone(),
        "recipient".parse().unwrap(),
        TEST_SENDER_ADDRESS,
        team.clone(),
        atm_core::send::SendMessageSource::Inline(format!("{task} done")),
        None,
        false,
        Some(task.parse().expect("task id")),
        false,
    )
    .unwrap();
    request.task_op = Some(TaskOp::Close {
        outcome: atm_storage::TaskCloseOutcome::Completed,
        reason: Some("done".into()),
    });
    let observability = CliObservability::fallback();
    fixture
        .composition(&observability)
        .send(request)
        .await
        .expect("close task");
}

#[tokio::test]
#[serial(env)]
async fn preempt_pauses_active_task_to_position_two_and_queues_new_task_at_head() {
    let fixture = LoopbackFixture::new("recipient");
    let team: TeamName = TEST_TEAM.parse().expect("team");
    execute_assign(&fixture, assign("X", TEST_RECIPIENT_ADDRESS, TEST_SENDER)).await;
    start_task(&fixture, &team, "X").await;
    execute_assign(&fixture, preempt("Y", TEST_RECIPIENT_ADDRESS, TEST_SENDER)).await;

    let store = fixture.task_store();
    let member = MemberKey::new(team.clone(), "recipient".parse().expect("recipient"));

    let paused = store
        .load_task(&team, &"X".parse().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(paused.state, TaskState::Assigned);
    assert_eq!(paused.position.expect("position").get(), 2);
    let head = store
        .load_task(&team, &"Y".parse().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(head.position.expect("position").get(), 1);

    let x_events = store
        .list_task_events(&team, &"X".parse().unwrap(), None)
        .unwrap();
    let paused_events: Vec<_> = x_events
        .iter()
        .filter(|event| event.event == TaskEventKind::Paused)
        .collect();
    assert_eq!(paused_events.len(), 1, "exactly one paused event on X");
    assert!(paused_events[0].detail.as_deref().unwrap().contains("Y"));

    let open = store.open_tasks(&member).unwrap();
    assert_eq!(
        open.iter()
            .map(|row| (row.task_id.as_str(), row.position.unwrap().get()))
            .collect::<std::collections::BTreeMap<_, _>>(),
        std::collections::BTreeMap::from([("X", 2), ("Y", 1)])
    );
}

#[tokio::test]
#[serial(env)]
async fn preempt_without_active_task_behaves_exactly_like_head() {
    let fixture = LoopbackFixture::new("recipient");
    let team: TeamName = TEST_TEAM.parse().expect("team");
    execute_assign(&fixture, assign("A", TEST_RECIPIENT_ADDRESS, TEST_SENDER)).await;
    execute_assign(&fixture, preempt("B", TEST_RECIPIENT_ADDRESS, TEST_SENDER)).await;

    let store = fixture.task_store();
    let a = store
        .load_task(&team, &"A".parse().unwrap())
        .unwrap()
        .unwrap();
    let b = store
        .load_task(&team, &"B".parse().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(b.position.expect("position").get(), 1);
    assert_eq!(a.position.expect("position").get(), 2);
    assert!(
        store
            .list_task_events(&team, &"A".parse().unwrap(), None)
            .unwrap()
            .iter()
            .all(|event| event.event != TaskEventKind::Paused),
        "no active task means no paused event"
    );
}

#[tokio::test]
#[serial(env)]
async fn plain_head_never_pauses_the_active_task() {
    let fixture = LoopbackFixture::new("recipient");
    let team: TeamName = TEST_TEAM.parse().expect("team");
    execute_assign(&fixture, assign("X", TEST_RECIPIENT_ADDRESS, TEST_SENDER)).await;
    start_task(&fixture, &team, "X").await;
    execute_assign(
        &fixture,
        TaskAssignCommand {
            head: true,
            ..assign("Y", TEST_RECIPIENT_ADDRESS, TEST_SENDER)
        },
    )
    .await;

    let store = fixture.task_store();
    let active = store
        .load_task(&team, &"X".parse().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(active.state, TaskState::Active);
    assert_eq!(active.position.expect("position").get(), 1);
    assert!(
        store
            .list_task_events(&team, &"X".parse().unwrap(), None)
            .unwrap()
            .iter()
            .all(|event| event.event != TaskEventKind::Paused),
        "plain --head never pauses the active task"
    );
}

#[tokio::test]
#[serial(env)]
async fn preempted_task_resumes_with_task_start_and_keeps_its_original_history() {
    let fixture = LoopbackFixture::new("recipient");
    let team: TeamName = TEST_TEAM.parse().expect("team");
    execute_assign(&fixture, assign("X", TEST_RECIPIENT_ADDRESS, TEST_SENDER)).await;
    start_task(&fixture, &team, "X").await;
    execute_assign(&fixture, preempt("Y", TEST_RECIPIENT_ADDRESS, TEST_SENDER)).await;

    let store = fixture.task_store();
    let assigned_event_before = store
        .list_task_events(&team, &"X".parse().unwrap(), None)
        .unwrap()
        .into_iter()
        .find(|event| event.event == TaskEventKind::Assigned)
        .expect("original assigned event");

    close_task(&fixture, &team, "Y").await;
    start_task(&fixture, &team, "X").await;

    let resumed = store
        .load_task(&team, &"X".parse().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(resumed.state, TaskState::Active);

    let events_after = store
        .list_task_events(&team, &"X".parse().unwrap(), None)
        .unwrap();
    let assigned_event_after = events_after
        .iter()
        .find(|event| event.event == TaskEventKind::Assigned)
        .expect("original assigned event still present");
    assert_eq!(assigned_event_after.seq, assigned_event_before.seq);
    assert_eq!(assigned_event_after.at, assigned_event_before.at);
    assert!(
        events_after
            .iter()
            .any(|event| event.event == TaskEventKind::Started),
        "resumed task records a fresh started event"
    );
}

/// The urgent task is usually already queued: `--preempt` against a task id
/// already `assigned` to the same agent must still pause the active task and
/// move the queued task to the head, in the same write as the same-agent
/// update (#1620 review, placement decision, 2026-09-27).
#[tokio::test]
#[serial(env)]
async fn preempt_against_an_already_queued_same_agent_task_pauses_and_moves_it_to_head() {
    let fixture = LoopbackFixture::new("recipient");
    let team: TeamName = TEST_TEAM.parse().expect("team");
    execute_assign(&fixture, assign("X", TEST_RECIPIENT_ADDRESS, TEST_SENDER)).await;
    start_task(&fixture, &team, "X").await;
    execute_assign(&fixture, assign("Y", TEST_RECIPIENT_ADDRESS, TEST_SENDER)).await;

    let store = fixture.task_store();
    let queued_before = store
        .load_task(&team, &"Y".parse().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(queued_before.position.expect("position").get(), 2);

    execute_assign(&fixture, preempt("Y", TEST_RECIPIENT_ADDRESS, TEST_SENDER)).await;

    let paused = store
        .load_task(&team, &"X".parse().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(paused.state, TaskState::Assigned);
    assert_eq!(paused.position.expect("position").get(), 2);
    let head = store
        .load_task(&team, &"Y".parse().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(head.state, TaskState::Assigned);
    assert_eq!(head.position.expect("position").get(), 1);

    let x_events = store
        .list_task_events(&team, &"X".parse().unwrap(), None)
        .unwrap();
    let paused_events: Vec<_> = x_events
        .iter()
        .filter(|event| event.event == TaskEventKind::Paused)
        .collect();
    assert_eq!(paused_events.len(), 1, "exactly one paused event on X");
    assert!(paused_events[0].detail.as_deref().unwrap().contains('Y'));

    let y_events = store
        .list_task_events(&team, &"Y".parse().unwrap(), None)
        .unwrap();
    assert_eq!(
        y_events
            .iter()
            .filter(|event| event.event == TaskEventKind::Updated)
            .count(),
        1,
        "the already-queued task gets one updated event, not a fresh assignment"
    );
    assert!(
        y_events
            .iter()
            .all(|event| event.event != TaskEventKind::Moved),
        "preempt's head move is silent; only `updated` records it"
    );
    assert_eq!(
        y_events
            .iter()
            .filter(|event| event.event == TaskEventKind::Assigned)
            .count(),
        1,
        "the original assigned event is untouched, not repeated"
    );
}
