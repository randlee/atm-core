use atm_core::test_support::{TEST_RECIPIENT_ADDRESS, TEST_SENDER, TEST_TEAM};
use atm_storage::{MoveTarget, QueuePosition, TaskEventKind};
use serial_test::serial;

use super::*;
use crate::composition::tests::LoopbackFixture;

fn caller(actor: &str) -> CallerArgs {
    CallerArgs {
        actor: Some(actor.into()),
        team: Some(TEST_TEAM.into()),
    }
}
fn assign(task: &str, assignee: &str, placement: Option<MoveTarget>) -> TaskAssignCommand {
    TaskAssignCommand {
        assignee: assignee.parse().unwrap(),
        task_id: Some(task.parse().unwrap()),
        before: match placement.clone() {
            Some(MoveTarget::Before { task_id }) => Some(task_id),
            _ => None,
        },
        head: matches!(placement, Some(MoveTarget::Head)),
        message: MessageSourceArgs {
            text: Some(format!("assign {task}")),
            file: None,
            stdin: false,
            template: None,
            vars: None,
        },
        json: true,
        caller: caller(TEST_SENDER),
    }
}
async fn run_assign(f: &LoopbackFixture, command: TaskAssignCommand) {
    let obs = CliObservability::fallback();
    let composition = f.composition(&obs);
    command
        .execute(&composition, f.home_dir.clone(), f.current_dir.clone())
        .await
        .unwrap();
}
async fn list(f: &LoopbackFixture, all: bool, actor: &str) -> String {
    let command = TaskListCommand {
        all,
        limit: None,
        json: true,
        caller: caller(actor),
    };
    let context = resolve_context(&command.caller).unwrap();
    let obs = CliObservability::fallback();
    let composition = f.composition(&obs);
    command
        .execute(
            &composition,
            context,
            f.home_dir.clone(),
            f.current_dir.clone(),
        )
        .await
        .unwrap()
}

#[tokio::test]
#[serial(env)]
async fn list_default_is_callers_own_queue() {
    let f = LoopbackFixture::new("recipient");
    run_assign(&f, assign("MINE", TEST_RECIPIENT_ADDRESS, None)).await;
    run_assign(&f, assign("OTHER", "test-lead@test-team", None)).await;
    let rows: Vec<atm_storage::TaskRow> =
        serde_json::from_str(&list(&f, false, "recipient").await).unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row.task_id.as_str())
            .collect::<Vec<_>>(),
        ["MINE"]
    );
}

#[tokio::test]
#[serial(env)]
async fn list_all_shows_every_member_grouped_with_state_header() {
    let f = LoopbackFixture::new("recipient");
    run_assign(&f, assign("A", TEST_RECIPIENT_ADDRESS, None)).await;
    run_assign(&f, assign("B", "test-lead@test-team", None)).await;
    let command = TaskListCommand {
        all: true,
        limit: None,
        json: false,
        caller: caller(TEST_SENDER),
    };
    let context = resolve_context(&command.caller).unwrap();
    let obs = CliObservability::fallback();
    let composition = f.composition(&obs);
    let output = command
        .execute(
            &composition,
            context,
            f.home_dir.clone(),
            f.current_dir.clone(),
        )
        .await
        .unwrap();
    assert!(output.contains("recipient (state:"));
    assert!(output.contains("test-lead (state:"));
}

#[tokio::test]
#[serial(env)]
async fn list_orders_by_position_not_assigned_at() {
    let f = LoopbackFixture::new("recipient");
    run_assign(&f, assign("T2", TEST_RECIPIENT_ADDRESS, None)).await;
    run_assign(
        &f,
        assign("T1", TEST_RECIPIENT_ADDRESS, Some(MoveTarget::Head)),
    )
    .await;
    let rows: Vec<atm_storage::TaskRow> =
        serde_json::from_str(&list(&f, false, "recipient").await).unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| (row.task_id.as_str(), row.position.map(QueuePosition::get)))
            .collect::<Vec<_>>(),
        [("T1", Some(1)), ("T2", Some(2))]
    );
    assert!(rows[0].assigned_at >= rows[1].assigned_at);
}

#[tokio::test]
#[serial(env)]
async fn events_are_seq_ordered_and_include_moved_and_started() {
    let f = LoopbackFixture::new("recipient");
    run_assign(&f, assign("T1", TEST_RECIPIENT_ADDRESS, None)).await;
    run_assign(&f, assign("T2", TEST_RECIPIENT_ADDRESS, None)).await;
    let move_command = TaskMoveCommand {
        task_id: "T2".parse().unwrap(),
        head: true,
        end: false,
        before: None,
        json: true,
        caller: caller(TEST_SENDER),
    };
    let obs = CliObservability::fallback();
    let composition = f.composition(&obs);
    move_command
        .execute(&composition, resolve_context(&caller(TEST_SENDER)).unwrap())
        .await
        .unwrap();
    let store = f.task_store();
    store
        .record_reminder(
            &atm_storage::MemberKey::new(TEST_TEAM.parse().unwrap(), "recipient".parse().unwrap()),
            &"T2".parse().unwrap(),
            atm_storage::IsoTimestamp::now(),
            atm_storage::ReminderOutcome::Emitted,
        )
        .unwrap();
    let mut start = atm_core::send::SendRequest::new(
        f.home_dir.clone(),
        f.current_dir.clone(),
        "atm-daemon".parse().unwrap(),
        TEST_RECIPIENT_ADDRESS,
        TEST_TEAM.parse().unwrap(),
        atm_core::send::SendMessageSource::Inline("start".into()),
        None,
        false,
        Some("T2".parse().unwrap()),
        false,
    )
    .unwrap();
    start.task_op = Some(atm_storage::TaskOp::Start);
    composition.send(start).await.unwrap();
    let command = TaskEventsCommand {
        task_id: "T2".parse().unwrap(),
        limit: None,
        all: false,
        json: true,
        caller: caller("recipient"),
    };
    let context = resolve_context(&command.caller).unwrap();
    let output = command
        .execute(
            &composition,
            context,
            f.home_dir.clone(),
            f.current_dir.clone(),
        )
        .await
        .unwrap();
    let events: Vec<atm_storage::TaskEventRow> = serde_json::from_str(&output).unwrap();
    assert!(events.windows(2).all(|pair| pair[0].seq < pair[1].seq));
    assert!(
        events
            .iter()
            .any(|event| event.event == TaskEventKind::Moved)
    );
    assert!(
        events
            .iter()
            .any(|event| event.event == TaskEventKind::Started)
    );
}
