#![cfg(test)]

use atm_core::test_support::{
    TEST_LEAD, TEST_LEAD_ADDRESS, TEST_RECIPIENT_ADDRESS, TEST_SENDER, TEST_SENDER_ADDRESS,
    TEST_TEAM,
};
use atm_storage::{MemberKey, ReminderOutcome, TaskEventKind, TaskOp, TaskState};
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
