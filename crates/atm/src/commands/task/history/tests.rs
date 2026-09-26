#![cfg(test)]

use atm_core::test_support::{
    TEST_LEAD, TEST_LEAD_ADDRESS, TEST_RECIPIENT_ADDRESS, TEST_SENDER, TEST_SENDER_ADDRESS,
    TEST_TEAM,
};
use atm_storage::{
    AtmMessageId, IsoTimestamp, TaskActor, TaskCloseOutcome, TaskEventKind, TaskEventRow, TaskOp,
    TaskRow, TaskState,
};
use serial_test::serial;

use super::*;
use crate::commands::task::{CallerArgs, MessageSourceArgs, OutcomeArg, TaskAssignCommand};
use crate::composition::tests::LoopbackFixture;

fn caller(actor: &str) -> CallerArgs {
    CallerArgs {
        actor: Some(actor.into()),
        team: Some(TEST_TEAM.into()),
    }
}

fn assign(task: &str, assignee: &str) -> TaskAssignCommand {
    TaskAssignCommand {
        assignee: assignee.parse().unwrap(),
        task_id: Some(task.parse().unwrap()),
        before: None,
        head: false,
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

async fn run_start(f: &LoopbackFixture, task: &str, assignee: &str) {
    let obs = CliObservability::fallback();
    let composition = f.composition(&obs);
    let mut start = atm_core::send::SendRequest::new(
        f.home_dir.clone(),
        f.current_dir.clone(),
        assignee.parse().unwrap(),
        TEST_SENDER_ADDRESS,
        TEST_TEAM.parse().unwrap(),
        atm_core::send::SendMessageSource::Inline("start".into()),
        None,
        false,
        Some(task.parse().unwrap()),
        false,
    )
    .unwrap();
    start.task_op = Some(TaskOp::Start);
    composition.send(start).await.unwrap();
}

async fn run_close(f: &LoopbackFixture, task: &str, actor: &str, outcome: OutcomeArg) {
    let command = crate::commands::task::TaskCloseCommand {
        task_id: task.parse().unwrap(),
        outcome,
        reason: Some("report".into()),
        report: MessageSourceArgs {
            text: None,
            file: None,
            stdin: false,
            template: None,
            vars: None,
        },
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
        .unwrap();
}

async fn run_history(f: &LoopbackFixture, command: TaskHistoryCommand) -> String {
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

fn history_command(
    member: Option<&str>,
    limit: Option<usize>,
    events: bool,
    json: bool,
) -> TaskHistoryCommand {
    TaskHistoryCommand {
        member: member.map(|value| value.parse().unwrap()),
        limit,
        events,
        json,
        caller: caller(TEST_SENDER),
    }
}

#[tokio::test]
#[serial(env)]
async fn history_lists_open_and_completed_tasks_newest_first() {
    let f = LoopbackFixture::new("recipient");
    run_assign(&f, assign("T1", TEST_RECIPIENT_ADDRESS)).await;
    run_assign(&f, assign("T2", TEST_RECIPIENT_ADDRESS)).await;
    run_close(&f, "T2", TEST_SENDER, OutcomeArg::Completed).await;
    run_assign(&f, assign("T3", TEST_LEAD_ADDRESS)).await;
    run_close(&f, "T3", TEST_SENDER, OutcomeArg::Refused).await;

    let output = run_history(&f, history_command(None, None, false, true)).await;
    let payload: serde_json::Value = serde_json::from_str(&output).unwrap();
    let tasks = payload["tasks"].as_array().unwrap();

    assert_eq!(
        tasks
            .iter()
            .map(|task| task["task_id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["T3", "T2", "T1"],
        "newest first, including the completed rows"
    );
    assert_eq!(tasks[0]["outcome"], "refused");
    assert_eq!(tasks[0]["state"], "complete");
    assert!(tasks[0]["closed_at"].is_string());
    assert_eq!(tasks[1]["outcome"], "completed");
    assert_eq!(
        tasks[2]["outcome"], "assigned",
        "open tasks report their state"
    );
    assert!(tasks[2]["closed_at"].is_null());
    assert!(tasks[2]["started_at"].is_null());
    assert_eq!(tasks[2]["reminders"], 0);
    assert_eq!(tasks[2]["escalations"], 0);
    assert!(payload.get("events").is_none());
}

#[tokio::test]
#[serial(env)]
async fn history_honours_limit_and_member() {
    let f = LoopbackFixture::new("recipient");
    run_assign(&f, assign("T1", TEST_RECIPIENT_ADDRESS)).await;
    run_assign(&f, assign("T2", TEST_LEAD_ADDRESS)).await;
    run_close(&f, "T2", TEST_SENDER, OutcomeArg::Completed).await;
    run_assign(&f, assign("T3", TEST_RECIPIENT_ADDRESS)).await;

    let limited = run_history(&f, history_command(None, Some(2), false, true)).await;
    let limited: serde_json::Value = serde_json::from_str(&limited).unwrap();
    assert_eq!(
        limited["tasks"].as_array().unwrap().len(),
        2,
        "--limit bounds the page"
    );

    let scoped = run_history(&f, history_command(Some("recipient"), None, false, true)).await;
    let scoped: serde_json::Value = serde_json::from_str(&scoped).unwrap();
    let task_ids = scoped["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["task_id"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(task_ids, ["T3", "T1"], "--member filters to one assignee");
}

#[tokio::test]
#[serial(env)]
async fn history_events_interleaves_two_tasks_chronologically() {
    let f = LoopbackFixture::new("recipient");
    run_assign(&f, assign("T1", TEST_RECIPIENT_ADDRESS)).await;
    run_assign(&f, assign("T2", TEST_LEAD_ADDRESS)).await;
    run_start(&f, "T2", TEST_LEAD).await;
    run_start(&f, "T1", "recipient").await;
    run_close(&f, "T2", TEST_SENDER, OutcomeArg::Refused).await;
    run_close(&f, "T1", TEST_SENDER, OutcomeArg::Completed).await;

    let output = run_history(&f, history_command(None, None, true, true)).await;
    let payload: serde_json::Value = serde_json::from_str(&output).unwrap();
    let events: Vec<TaskEventRow> = serde_json::from_value(payload["events"].clone()).unwrap();

    let sequence = events
        .iter()
        .map(|event| (event.task_id.as_str(), event.event))
        .collect::<Vec<_>>();
    assert_eq!(
        sequence,
        [
            ("T1", TaskEventKind::Assigned),
            ("T2", TaskEventKind::Assigned),
            ("T2", TaskEventKind::Started),
            ("T1", TaskEventKind::Started),
            ("T2", TaskEventKind::Refused),
            ("T1", TaskEventKind::Completed),
        ],
        "ledger rows from both tasks interleave in wall-clock order"
    );
    assert!(events.windows(2).all(|pair| pair[0].at <= pair[1].at));

    let table = run_history(&f, history_command(None, None, true, false)).await;
    assert!(table.contains("T1 "));
    assert!(table.contains("T2 "));
    assert!(table.contains("seq at event from→to actor detail"));
}

fn task_row(task_id: &str, assignee: &str, state: TaskState, assigned_at: &str) -> TaskRow {
    TaskRow {
        team: TEST_TEAM.parse().unwrap(),
        task_id: task_id.parse().unwrap(),
        assignee: assignee.parse().unwrap(),
        assigner: TEST_SENDER.parse().unwrap(),
        state,
        position: state
            .is_open()
            .then(|| atm_storage::QueuePosition::new(1).unwrap()),
        assignment_message_id: AtmMessageId::new(),
        description: task_id.to_owned(),
        assigned_at: assigned_at.parse().unwrap(),
        updated_at: assigned_at.parse().unwrap(),
        last_reminded_at: None,
        reminder_count: 0,
        lead_notified_count: 0,
    }
}

fn close_event(task_id: &str, at: &str, event: TaskEventKind) -> TaskEventRow {
    TaskEventRow {
        team: TEST_TEAM.parse().unwrap(),
        task_id: task_id.parse().unwrap(),
        assignee: "recipient".parse().unwrap(),
        seq: 1,
        at: at.parse().unwrap(),
        event,
        from_state: None,
        to_state: None,
        actor: TaskActor::Member(TEST_SENDER.parse().unwrap()),
        message_id: None,
        outcome: None,
        marker: None,
        detail: None,
    }
}

#[test]
fn table_shows_one_date_line_per_change_hhmm_and_dash_for_open_tasks() {
    let open = HistoryRow::from_row_and_events(
        task_row(
            "T2",
            "recipient",
            TaskState::Assigned,
            "2026-01-02T09:05:00Z",
        ),
        Vec::new(),
    );
    let closed = HistoryRow::from_row_and_events(
        task_row(
            "T1",
            "recipient",
            TaskState::Complete(TaskCloseOutcome::Completed),
            "2026-01-01T08:00:00Z",
        ),
        vec![close_event(
            "T1",
            "2026-01-01T10:38:00Z",
            TaskEventKind::Completed,
        )],
    );

    let table = render_history_table(&[open, closed], false);

    assert_eq!(table.matches("2026-01-02").count(), 1);
    assert_eq!(table.matches("2026-01-01").count(), 1);
    assert!(table.contains("09:05"));
    assert!(table.contains("08:00"));
    assert!(table.contains("10:38"));
    assert!(
        table.contains(" - "),
        "open row has a dash for started/closed"
    );
    assert!(
        table.contains("2h 38m"),
        "T1 ran assigned->completed for 2h38m"
    );
}

#[test]
fn duration_formats_days_hours_and_minutes() {
    assert_eq!(format_duration(chrono::Duration::minutes(32)), "32m");
    assert_eq!(
        format_duration(chrono::Duration::minutes(9 * 60 + 22)),
        "9h 22m"
    );
    assert_eq!(
        format_duration(chrono::Duration::minutes(3 * 24 * 60 + 4 * 60)),
        "3d 4h"
    );
}

#[test]
fn history_row_takes_the_first_started_and_last_close_event() {
    let row = task_row(
        "T1",
        "recipient",
        TaskState::Complete(TaskCloseOutcome::Refused),
        "2026-01-01T00:00:00Z",
    );
    let events = vec![
        close_event("T1", "2026-01-01T00:05:00Z", TaskEventKind::Started),
        close_event("T1", "2026-01-01T00:10:00Z", TaskEventKind::Reopened),
        close_event("T1", "2026-01-01T00:15:00Z", TaskEventKind::Started),
        close_event("T1", "2026-01-01T00:20:00Z", TaskEventKind::Refused),
    ];
    let history = HistoryRow::from_row_and_events(row, events);

    assert_eq!(
        history.started_at,
        Some("2026-01-01T00:05:00Z".parse::<IsoTimestamp>().unwrap()),
        "the first started event, not the most recent"
    );
    assert_eq!(
        history.closed_at,
        Some("2026-01-01T00:20:00Z".parse::<IsoTimestamp>().unwrap())
    );
    assert_eq!(history.outcome, "refused");
}

#[test]
fn resolved_limit_defaults_to_ten_and_rejects_zero() {
    let default = history_command(None, None, false, false);
    assert_eq!(default.resolved_limit().unwrap(), DEFAULT_HISTORY_LIMIT);

    let zero = history_command(None, Some(0), false, false);
    assert!(zero.resolved_limit().is_err());
}
