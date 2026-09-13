#![cfg(test)]

use atm_core::list::ListOutcome;
use atm_core::read::BucketCounts;
use atm_core::test_support::{TEST_RECIPIENT_ADDRESS, TEST_SENDER, TEST_SENDER_ADDRESS, TEST_TEAM};
use atm_core::types::{CommandAction, ReadSelection};
use atm_storage::{
    BuiltInNudgeTemplateKind, MessageKey, MoveTarget, PromptHandoff, PromptTrigger, QueuePosition,
    TaskActor, TaskEventKind, TaskEventRow,
};
use serde::Deserialize;
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
        "recipient".parse().unwrap(),
        TEST_SENDER_ADDRESS,
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
    let output: serde_json::Value = serde_json::from_str(&output).unwrap();
    let events: Vec<atm_storage::TaskEventRow> =
        serde_json::from_value(output["events"].clone()).unwrap();
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
    assert_eq!(output["handoffs"], serde_json::json!([]));
}

fn event_at(at: &str) -> TaskEventRow {
    TaskEventRow {
        team: TEST_TEAM.parse().unwrap(),
        task_id: "T1".parse().unwrap(),
        assignee: "recipient".parse().unwrap(),
        seq: 1,
        at: at.parse().unwrap(),
        event: TaskEventKind::Assigned,
        from_state: None,
        to_state: None,
        actor: TaskActor::Member(TEST_SENDER.parse().unwrap()),
        message_id: None,
        outcome: None,
        marker: None,
        detail: None,
    }
}

fn handoff_at(at: &str, message_key: &str) -> PromptHandoff {
    PromptHandoff {
        team: TEST_TEAM.parse().unwrap(),
        agent: "recipient".parse().unwrap(),
        message_key: MessageKey::new(message_key).unwrap(),
        kind: BuiltInNudgeTemplateKind::TaskReady,
        task_id: "T1".parse().unwrap(),
        attempt: 0,
        trigger: PromptTrigger::TaskPass,
        at: at.parse().unwrap(),
    }
}

#[test]
fn task_events_interleaves_handoffs_by_time() {
    let event = event_at("2026-09-12T15:27:34Z");
    let earlier = handoff_at("2026-09-12T15:27:33Z", "01M2EARLIER000000000000000");
    let tied = handoff_at("2026-09-12T15:27:34Z", "01M2TIED00000000000000000");

    let output = render_task_events(&[event], &[earlier, tied], false).unwrap();
    let earlier_at = output.find("msg=01M2EARLIER").unwrap();
    let event_at = output.find("assigned").unwrap();
    let tied_at = output.find("msg=01M2TIED").unwrap();

    assert!(earlier_at < event_at);
    assert!(event_at < tied_at, "task events must win an equal-at tie");
    assert!(output.contains(
        "2026-09-12T15:27:34Z  prompt  task_ready  attempt=0  trigger=task_pass  msg=01M2TIED"
    ));
}

fn task_events_outcome(handoffs: Vec<PromptHandoff>) -> ListOutcome {
    ListOutcome {
        action: CommandAction::List,
        team: TEST_TEAM.parse().unwrap(),
        agent: "recipient".parse().unwrap(),
        selection_mode: ReadSelection::Actionable,
        history_collapsed: false,
        count: 1 + handoffs.len(),
        rows: Vec::new(),
        bucket_counts: BucketCounts {
            unread: 0,
            pending_ack: 0,
            history: 0,
        },
        task_rows: Vec::new(),
        task_event_rows: vec![event_at("2026-09-12T15:27:34Z")],
        handoffs,
    }
}

#[test]
fn task_events_decodes_response_without_handoffs_field() {
    let mut value = serde_json::to_value(task_events_outcome(vec![handoff_at(
        "2026-09-12T15:27:34Z",
        "01M2HANDOFF00000000000000",
    )]))
    .unwrap();
    assert!(value.as_object_mut().unwrap().remove("handoffs").is_some());

    let decoded: ListOutcome = serde_json::from_value(value).unwrap();

    assert!(decoded.handoffs.is_empty());
    assert_eq!(decoded.task_event_rows.len(), 1);
}

#[test]
fn task_events_omits_empty_handoffs_from_json() {
    let value = serde_json::to_value(task_events_outcome(Vec::new())).unwrap();

    assert!(value.get("handoffs").is_none());
}

#[derive(Deserialize)]
struct Frozen18TaskEventsResponse {
    action: CommandAction,
    team: atm_core::types::TeamName,
    agent: atm_core::types::AgentName,
    selection_mode: ReadSelection,
    history_collapsed: bool,
    count: usize,
    rows: Vec<atm_core::list::ListRow>,
    bucket_counts: BucketCounts,
    #[serde(default)]
    task_rows: Vec<atm_storage::TaskRow>,
    #[serde(default)]
    task_event_rows: Vec<TaskEventRow>,
}

#[test]
fn frozen_1_8_task_events_response_decodes_1_9_payload_with_handoffs() {
    let payload = serde_json::to_value(task_events_outcome(vec![handoff_at(
        "2026-09-12T15:27:34Z",
        "01M2HANDOFF00000000000000",
    )]))
    .unwrap();

    let frozen: Frozen18TaskEventsResponse = serde_json::from_value(payload).unwrap();

    assert_eq!(frozen.action, CommandAction::List);
    assert_eq!(frozen.team.as_str(), TEST_TEAM);
    assert_eq!(frozen.agent.as_str(), "recipient");
    assert_eq!(frozen.selection_mode, ReadSelection::Actionable);
    assert!(!frozen.history_collapsed);
    assert_eq!(frozen.count, 2);
    assert!(frozen.rows.is_empty());
    assert_eq!(frozen.bucket_counts.unread, 0);
    assert!(frozen.task_rows.is_empty());
    assert_eq!(frozen.task_event_rows.len(), 1);
}
