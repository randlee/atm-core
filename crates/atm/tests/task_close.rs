use atm_core::schema::AtmMessageId;
use atm_core::test_support::{TEST_SENDER, TEST_TEAM};
use atm_storage::{
    Message, MessageEnvelope, MessageKey, QueuePosition, TaskActor, TaskCloseOutcome, TaskState,
};
use clap::Parser;
use serial_test::serial;

use super::*;
use crate::composition::tests::LoopbackFixture;

fn caller(actor: &str) -> CallerArgs {
    CallerArgs {
        actor: Some(actor.into()),
        team: Some(TEST_TEAM.into()),
    }
}

fn close(task: &str, actor: &str, outcome: OutcomeArg) -> TaskCloseCommand {
    TaskCloseCommand {
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
    }
}

fn seed_assignment(f: &LoopbackFixture, task: &str, assignee: &str, assigner: &str) {
    let message_id = AtmMessageId::new();
    let timestamp = atm_storage::IsoTimestamp::now();
    f.message_store()
        .save_message(&Message {
            team: TEST_TEAM.parse().unwrap(),
            agent: assignee.parse().unwrap(),
            message_key: MessageKey::from(message_id),
            envelope: MessageEnvelope {
                from: assigner.parse().unwrap(),
                source_chat_id: None,
                text: format!("assign {task}"),
                timestamp,
                read: false,
                source_team: Some(TEST_TEAM.parse().unwrap()),
                destination_chat_id: None,
                summary: None,
                message_id: Some(message_id),
                requires_ack: true,
                pending_ack_at: Some(timestamp),
                acknowledged_at: None,
                acknowledges_message_id: None,
                parent_message_id: None,
                thread_mode: None,
                expires_at: None,
                task_id: Some(task.parse().unwrap()),
                placement: None,
                task_op: None,
                task_complete: None,
                extra: serde_json::Map::new(),
            },
        })
        .unwrap();
}

async fn run_close(f: &LoopbackFixture, command: TaskCloseCommand) -> Result<String> {
    let context = resolve_context(&command.caller)?;
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
}

#[tokio::test]
#[serial(env)]
async fn close_open_task_delivers_and_closes_in_one_write() {
    for (task, arg, expected) in [
        ("T1", OutcomeArg::Completed, TaskCloseOutcome::Completed),
        ("T2", OutcomeArg::Refused, TaskCloseOutcome::Refused),
        ("T3", OutcomeArg::Cancelled, TaskCloseOutcome::Cancelled),
    ] {
        let f = LoopbackFixture::new_with_identity("recipient", "recipient");
        seed_assignment(&f, task, "recipient", TEST_SENDER);
        run_close(&f, close(task, "recipient", arg)).await.unwrap();
        let row = f
            .task_store()
            .load_task(&TEST_TEAM.parse().unwrap(), &task.parse().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(row.state, TaskState::Complete(expected));
        assert_eq!(f.inbox_contents(TEST_SENDER).len(), 1);
    }
}

#[tokio::test]
#[serial(env)]
async fn close_by_assigner_reports_to_assignee() {
    let f = LoopbackFixture::new("recipient");
    seed_assignment(&f, "T1", "recipient", TEST_SENDER);
    run_close(&f, close("T1", TEST_SENDER, OutcomeArg::Cancelled))
        .await
        .unwrap();
    assert_eq!(f.inbox_contents("recipient").len(), 2);
}

#[tokio::test]
#[serial(env)]
async fn close_unknown_task_sends_nothing_and_exits_one() {
    let f = LoopbackFixture::new_with_identity("recipient", "recipient");
    let before = f.inbox_contents(TEST_SENDER).len();
    let error = run_close(&f, close("UNKNOWN", "recipient", OutcomeArg::Completed))
        .await
        .unwrap_err();
    assert_eq!(crate::exit_code_for_error(&error), 1);
    assert_eq!(f.inbox_contents(TEST_SENDER).len(), before);
}

#[tokio::test]
#[serial(env)]
async fn close_already_closed_delivers_report_without_task_event() {
    let f = LoopbackFixture::new_with_identity("recipient", "recipient");
    seed_assignment(&f, "T1", "recipient", TEST_SENDER);
    let mut first_close = close("T1", "recipient", OutcomeArg::Completed);
    first_close.json = false;
    let first = run_close(&f, first_close).await.unwrap();
    assert_eq!(first, "closed T1 (completed)\n");
    let store = f.task_store();
    let team = TEST_TEAM.parse().unwrap();
    let task = "T1".parse().unwrap();
    let before = store.list_task_events(&team, &task, None).unwrap().len();
    let mut late_close = close("T1", "recipient", OutcomeArg::Refused);
    late_close.json = false;
    let already_closed = run_close(&f, late_close).await.unwrap();
    assert_eq!(
        already_closed,
        "task T1 was already closed (completed); report delivered\n"
    );
    assert_eq!(
        store.list_task_events(&team, &task, None).unwrap().len(),
        before
    );
    assert_eq!(f.inbox_contents(TEST_SENDER).len(), 2);
}

#[tokio::test]
#[serial(env)]
async fn stale_counterparty_rejection_exits_three_without_retry() {
    let f = LoopbackFixture::new_with_identity("recipient", "test-lead");
    seed_assignment(&f, "T1", "recipient", TEST_SENDER);
    let error = run_close(&f, close("T1", "test-lead", OutcomeArg::Completed))
        .await
        .unwrap_err();
    assert_eq!(crate::exit_code_for_error(&error), 3);
    assert_eq!(
        f.task_store()
            .load_task(&TEST_TEAM.parse().unwrap(), &"T1".parse().unwrap())
            .unwrap()
            .unwrap()
            .state,
        TaskState::Assigned
    );
}

#[tokio::test]
#[serial(env)]
async fn close_by_third_party_sends_nothing_and_exits_three() {
    let f = LoopbackFixture::new_with_identity("recipient", "test-lead");
    seed_assignment(&f, "T1", "recipient", TEST_SENDER);
    let sender_before = f.inbox_contents(TEST_SENDER).len();
    let recipient_before = f.inbox_contents("recipient").len();
    let store = f.task_store();
    let team: TeamName = TEST_TEAM.parse().unwrap();
    let task: TaskId = "T1".parse().unwrap();
    let rows_before = store.list_tasks(&team, None).unwrap().len();
    let events_before = store.list_task_events(&team, &task, None).unwrap().len();
    let error = run_close(&f, close("T1", "test-lead", OutcomeArg::Completed))
        .await
        .unwrap_err();
    assert_eq!(crate::exit_code_for_error(&error), 3);
    assert_eq!(f.inbox_contents(TEST_SENDER).len(), sender_before);
    assert_eq!(f.inbox_contents("recipient").len(), recipient_before);
    assert_eq!(store.list_tasks(&team, None).unwrap().len(), rows_before);
    let events = store.list_task_events(&team, &task, None).unwrap();
    assert_eq!(events.len(), events_before + 1);
    let rejected = events.last().unwrap();
    assert_eq!(rejected.event.as_str(), "rejected");
    assert_eq!(
        rejected.actor,
        TaskActor::Member("test-lead".parse().unwrap())
    );
}

#[tokio::test]
#[serial(env)]
async fn task_target_on_other_team_or_host_is_rejected_before_send() {
    for target in ["recipient@other-team", "recipient@test-team.remote-host"] {
        let command = TaskAssignCommand {
            assignee: target.parse().unwrap(),
            task_id: Some("T1".parse().unwrap()),
            before: None,
            head: false,
            message: MessageSourceArgs {
                text: Some("message".into()),
                file: None,
                stdin: false,
                template: None,
                vars: None,
            },
            json: true,
            caller: caller(TEST_SENDER),
        };
        let f = LoopbackFixture::new("recipient");
        let obs = CliObservability::fallback();
        let composition = f.composition(&obs);
        let error = command
            .execute(&composition, f.home_dir.clone(), f.current_dir.clone())
            .await
            .unwrap_err();
        assert_eq!(crate::exit_code_for_error(&error), 3);
        assert!(
            f.task_store()
                .list_tasks(&TEST_TEAM.parse().unwrap(), None)
                .unwrap()
                .is_empty()
        );
        drop(composition);
        drop(f);
        for alias in ["send", "queue"] {
            for completion in [false, true] {
                let f = LoopbackFixture::new("recipient");
                let mut args = vec!["atm", alias, target, "message", "--task-id", "T1"];
                if completion {
                    args.push("--task-complete");
                }
                let cli = crate::commands::Cli::try_parse_from(args).expect("task alias parses");
                let error = cli.run(&CliObservability::fallback()).await.unwrap_err();
                assert_eq!(crate::exit_code_for_error(&error), 1);
                assert!(
                    f.task_store()
                        .list_tasks(&TEST_TEAM.parse().unwrap(), None)
                        .unwrap()
                        .is_empty()
                );
            }
        }
    }
}

#[tokio::test]
#[serial(env)]
async fn refusal_releases_next_queued_task() {
    let f = LoopbackFixture::new_with_identity("recipient", "recipient");
    seed_assignment(&f, "T1", "recipient", TEST_SENDER);
    seed_assignment(&f, "T2", "recipient", TEST_SENDER);
    run_close(&f, close("T1", "recipient", OutcomeArg::Refused))
        .await
        .unwrap();
    let row = f
        .task_store()
        .load_task(&TEST_TEAM.parse().unwrap(), &"T2".parse().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(row.position, Some(QueuePosition::HEAD));
    assert!(
        f.inbox_contents("recipient")
            .iter()
            .any(|message| message.pending_ack_at.is_some())
    );
}
