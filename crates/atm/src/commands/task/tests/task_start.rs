#![cfg(test)]

use atm_core::error::AtmErrorCode;
use atm_core::schema::AtmMessageId;
use atm_core::test_support::{TEST_SENDER, TEST_TEAM};
use atm_storage::{MemberKey, Message, MessageEnvelope, MessageKey, TaskOp, TaskState};
use serial_test::serial;

use super::*;
use crate::composition::tests::LoopbackFixture;

fn caller(actor: &str) -> CallerArgs {
    CallerArgs {
        actor: Some(actor.into()),
        team: Some(TEST_TEAM.into()),
    }
}

fn start(task: &str, actor: &str, message: Option<&str>) -> TaskStartCommand {
    TaskStartCommand {
        task_id: task.parse().expect("task id"),
        message: message.map(str::to_owned),
        report: MessageSourceArgs {
            file: None,
            stdin: false,
            template: None,
            vars: None,
        },
        json: false,
        caller: caller(actor),
    }
}

fn seed_assignment(fixture: &LoopbackFixture, task: &str, assignee: &str) {
    let message_id = AtmMessageId::new();
    let timestamp = atm_storage::IsoTimestamp::now();
    fixture
        .message_store()
        .save_message(&Message {
            team: TEST_TEAM.parse().expect("team"),
            agent: assignee.parse().expect("assignee"),
            message_key: MessageKey::from(message_id),
            envelope: MessageEnvelope {
                from: TEST_SENDER.parse().expect("assigner"),
                source_chat_id: None,
                text: format!("assign {task}"),
                timestamp,
                read: false,
                source_team: Some(TEST_TEAM.parse().expect("team")),
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
                task_id: Some(task.parse().expect("task id")),
                placement: None,
                task_op: None,
                task_complete: None,
                extra: serde_json::Map::new(),
            },
        })
        .expect("seed assignment");
}

async fn run_start(fixture: &LoopbackFixture, command: TaskStartCommand) -> anyhow::Result<String> {
    let context = resolve_context(&command.caller)?;
    let observability = CliObservability::fallback();
    let composition = fixture.composition(&observability);
    command
        .execute(
            &composition,
            context,
            fixture.home_dir.clone(),
            fixture.current_dir.clone(),
        )
        .await
}

#[tokio::test]
#[serial(env)]
async fn task_start_sends_to_assigner_with_start_op() {
    let fixture = LoopbackFixture::new_with_identity("recipient", "recipient");
    seed_assignment(&fixture, "T1", "recipient");
    let output = run_start(&fixture, start("T1", "recipient", Some("begin now")))
        .await
        .expect("start");
    assert_eq!(output, "started T1\n");
    let messages = fixture.inbox_contents(TEST_SENDER);
    let start = messages.last().expect("start message");
    assert_eq!(start.text, "begin now");
    assert_eq!(start.task_op, Some(TaskOp::Start));
    assert_eq!(start.task_id.as_ref().unwrap().as_str(), "T1");
}

#[tokio::test]
#[serial(env)]
async fn task_start_defaults_message_when_omitted() {
    let fixture = LoopbackFixture::new_with_identity("recipient", "recipient");
    seed_assignment(&fixture, "T1", "recipient");
    run_start(&fixture, start("T1", "recipient", None))
        .await
        .expect("start");
    assert_eq!(
        fixture
            .inbox_contents(TEST_SENDER)
            .last()
            .expect("start message")
            .text,
        "started T1"
    );
}

#[tokio::test]
#[serial(env)]
async fn task_start_by_non_assignee_fails_before_sending() {
    let fixture = LoopbackFixture::new_with_identity("recipient", "test-lead");
    seed_assignment(&fixture, "T1", "recipient");
    let before = fixture.inbox_contents(TEST_SENDER).len();
    let error = run_start(&fixture, start("T1", "test-lead", None))
        .await
        .expect_err("preflight rejection");
    assert!(
        error
            .to_string()
            .contains("task T1 is not assigned to test-lead")
    );
    assert_eq!(fixture.inbox_contents(TEST_SENDER).len(), before);
}

#[tokio::test]
#[serial(env)]
async fn task_start_prints_writer_error_for_active_task() {
    let fixture = LoopbackFixture::new_with_identity("recipient", "recipient");
    seed_assignment(&fixture, "T1", "recipient");
    run_start(&fixture, start("T1", "recipient", None))
        .await
        .expect("first start");
    let error = run_start(&fixture, start("T1", "recipient", None))
        .await
        .expect_err("duplicate start");
    let typed = error
        .downcast_ref::<atm_core::error::AtmError>()
        .expect("writer error remains typed");
    assert_eq!(typed.code(), AtmErrorCode::TaskAlreadyActive);
    assert!(typed.message().starts_with("task T1 is already active"));
    assert_eq!(crate::exit_code_for_error(&error), 1);
    assert_eq!(
        fixture
            .task_store()
            .load_task(&TEST_TEAM.parse().unwrap(), &"T1".parse().unwrap())
            .unwrap()
            .unwrap()
            .state,
        TaskState::Active
    );
}

#[tokio::test]
#[serial(env)]
async fn task_start_line_reaches_assigner_at_write_time() {
    let fixture = LoopbackFixture::new_with_identity("recipient", "recipient");
    seed_assignment(&fixture, "T1", "recipient");
    run_start(
        &fixture,
        start("T1", "recipient", Some("starting despite busy assigner")),
    )
    .await
    .expect("start write");
    let line = fixture
        .inbox_contents(TEST_SENDER)
        .pop()
        .expect("immediate start message");
    assert_eq!(line.text, "starting despite busy assigner");
    assert_eq!(line.task_op, Some(TaskOp::Start));
}

#[tokio::test]
#[serial(env)]
async fn task_start_out_of_order_reorders_queue() {
    let fixture = LoopbackFixture::new_with_identity("recipient", "recipient");
    for task in ["T1", "T2", "T3"] {
        seed_assignment(&fixture, task, "recipient");
    }
    run_start(&fixture, start("T3", "recipient", None))
        .await
        .expect("start third task");
    let rows = fixture
        .task_store()
        .open_tasks(&MemberKey::new(
            TEST_TEAM.parse().unwrap(),
            "recipient".parse().unwrap(),
        ))
        .expect("open queue");
    assert_eq!(rows[0].task_id.as_str(), "T3");
    assert_eq!(rows[0].state, TaskState::Active);
    assert_eq!(rows[1].task_id.as_str(), "T1");
    assert_eq!(rows[2].task_id.as_str(), "T2");
}
