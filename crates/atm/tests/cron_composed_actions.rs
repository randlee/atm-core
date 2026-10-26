//! Deterministic production-composition coverage for cron task and acknowledgement actions.
//!
//! CLI argv parsing is deliberately covered in `commands::cron`; this target creates the typed
//! request that parser owns, then proves that a real worker drives the fixture actions through
//! the production router into durable SQLite state.

use std::path::PathBuf;

use atm_core::{
    boundary::{PhaseId, StopCondition},
    hook::{CronEndRequest, CronStartRequest},
    list::{ListQuery, TaskLedgerQuery},
    protocol::{RequestEnvelope, ResponseEnvelope, SendResponseEnvelope},
    send::{SendMessageSource, WriteRequest},
    test_support::{TEST_RECIPIENT, TEST_SENDER, TEST_TEAM},
    types::{IsoTimestamp, ReadSelection, TaskId},
};
use atm_daemon_bootstrap::test_support::ControlledCronComposition;
use atm_storage::{TaskActor, TaskCloseOutcome, TaskEventKind, TaskState};

const TASK_ID: &str = "BK15-F1-TASK";

fn fixture_module() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hook_module/cron_composed_actions.py")
        .canonicalize()
        .expect("fixture module exists")
}

fn start_request(
    composition: &ControlledCronComposition,
    name: &str,
    args: Vec<String>,
) -> RequestEnvelope {
    RequestEnvelope::CronStart(CronStartRequest {
        caller_identity: TEST_SENDER.parse().expect("test sender"),
        caller_team: TEST_TEAM.parse().expect("test team"),
        phase: PhaseId::parse(TEST_TEAM).expect("test phase"),
        name: name.to_owned(),
        cron_expr: "0 * * * * *".to_owned(),
        module_path: fixture_module(),
        cwd: composition.root_path().to_path_buf(),
        args,
        timeout_ms: 2_500,
        stop: StopCondition::Until {
            at: "2026-10-24T01:00:00Z"
                .parse::<IsoTimestamp>()
                .expect("fixed stop time"),
        },
        custom: serde_json::json!({}),
    })
}

fn end_request(name: &str) -> RequestEnvelope {
    RequestEnvelope::CronEnd(CronEndRequest {
        caller_identity: TEST_SENDER.parse().expect("test sender"),
        caller_team: TEST_TEAM.parse().expect("test team"),
        phase: PhaseId::parse(TEST_TEAM).expect("test phase"),
        name: Some(name.to_owned()),
    })
}

async fn run_action(
    composition: ControlledCronComposition,
    name: &str,
    mode: &str,
    identifier: &str,
) -> ControlledCronComposition {
    let started = composition
        .execute(start_request(
            &composition,
            name,
            vec![mode.to_owned(), identifier.to_owned()],
        ))
        .await
        .expect("start cron action");
    let ResponseEnvelope::CronStarted(schedule) = started else {
        panic!("cron action must start")
    };
    composition
        .start_cron_worker()
        .await
        .expect("start controlled worker");
    composition
        .advance_clock_and_wait_for_run(&schedule, chrono::Duration::minutes(1))
        .await
        .expect("cron action run completes");
    let composition = composition.reopen().await.expect("reopen composition");
    let ended = composition
        .execute(end_request(name))
        .await
        .expect("end cron action");
    assert!(matches!(ended, ResponseEnvelope::CronEnded(_)));
    composition
}

fn task_query(
    composition: &ControlledCronComposition,
    task_id: &str,
    ledger: TaskLedgerQuery,
) -> RequestEnvelope {
    RequestEnvelope::List(
        ListQuery::new(
            composition.root_path().into(),
            composition.root_path().into(),
            TEST_SENDER.parse().expect("test sender"),
            None,
            TEST_TEAM.parse().expect("test team"),
            ReadSelection::All,
            false,
            None,
            None,
            None,
            Some(task_id),
            None,
        )
        .expect("task query")
        .with_task_ledger(ledger),
    )
}

fn mailbox_query(composition: &ControlledCronComposition, target: &str) -> RequestEnvelope {
    RequestEnvelope::List(
        ListQuery::new(
            composition.root_path().into(),
            composition.root_path().into(),
            TEST_SENDER.parse().expect("test sender"),
            Some(target),
            TEST_TEAM.parse().expect("test team"),
            ReadSelection::All,
            false,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("mailbox query"),
    )
}

#[tokio::test]
async fn cron_task_actions_persist_assignment_lifecycle_and_identities() {
    let composition = ControlledCronComposition::new().expect("compose daemon");
    let composition = run_action(composition, "task-assign", "task-assign", TASK_ID).await;
    let composition = run_action(composition, "task-start", "task-start", TASK_ID).await;
    let composition = run_action(composition, "task-close", "task-close", TASK_ID).await;

    let ResponseEnvelope::List(tasks) = composition
        .execute(task_query(
            &composition,
            TASK_ID,
            TaskLedgerQuery::Tasks { member: None },
        ))
        .await
        .expect("read task")
    else {
        panic!("task list response")
    };
    let [task] = tasks.task_rows.as_slice() else {
        panic!("expected exactly one persisted task: {:?}", tasks.task_rows)
    };
    assert_eq!(task.task_id.as_str(), TASK_ID);
    assert_eq!(task.assignee.as_str(), TEST_RECIPIENT);
    assert_eq!(task.assigner.as_str(), TEST_SENDER);
    assert_eq!(task.state, TaskState::Complete(TaskCloseOutcome::Completed));

    let task_id: TaskId = TASK_ID.parse().expect("task id");
    let ResponseEnvelope::List(events) = composition
        .execute(task_query(
            &composition,
            TASK_ID,
            TaskLedgerQuery::Events {
                task_id,
                member: None,
            },
        ))
        .await
        .expect("read task events")
    else {
        panic!("task event response")
    };
    assert_eq!(
        events
            .task_event_rows
            .iter()
            .map(|row| row.event)
            .collect::<Vec<_>>(),
        vec![
            TaskEventKind::Assigned,
            TaskEventKind::Started,
            TaskEventKind::Completed,
        ]
    );
    assert_eq!(
        events
            .task_event_rows
            .iter()
            .map(|row| &row.actor)
            .collect::<Vec<_>>(),
        vec![
            &TaskActor::Member(TEST_SENDER.parse().expect("sender")),
            &TaskActor::Member(TEST_RECIPIENT.parse().expect("recipient")),
            &TaskActor::Member(TEST_RECIPIENT.parse().expect("recipient")),
        ]
    );
}

#[tokio::test]
async fn cron_acknowledgement_clears_only_the_seeded_pending_ack() {
    let composition = ControlledCronComposition::new().expect("compose daemon");
    let source = WriteRequest::new(
        composition.root_path().to_path_buf(),
        composition.root_path().to_path_buf(),
        TEST_SENDER.parse().expect("test sender"),
        "recipient@test-team",
        TEST_TEAM.parse().expect("test team"),
        SendMessageSource::Inline("fixture ack source".to_owned()),
        None,
        true,
        None,
        false,
    )
    .expect("seed source request");
    let ResponseEnvelope::Send(SendResponseEnvelope::Sent(source)) = composition
        .execute(RequestEnvelope::Write(Box::new(source)))
        .await
        .expect("seed acknowledgement source")
    else {
        panic!("source send response")
    };

    let composition = run_action(
        composition,
        "ack-source",
        "ack",
        &source.message_id.to_string(),
    )
    .await;

    let ResponseEnvelope::List(recipient_mail) = composition
        .execute(mailbox_query(&composition, "recipient@test-team"))
        .await
        .expect("read recipient mailbox")
    else {
        panic!("recipient mailbox response")
    };
    let source_row = recipient_mail
        .rows
        .iter()
        .find(|row| row.message_id.as_ref() == Some(&source.message_id))
        .expect("seeded source remains persisted");
    assert!(
        !source_row.pending_ack,
        "cron acknowledgement clears source pending ack"
    );

    let ResponseEnvelope::List(sender_mail) = composition
        .execute(mailbox_query(&composition, "sender-a@test-team"))
        .await
        .expect("read sender mailbox")
    else {
        panic!("sender mailbox response")
    };
    let acknowledgement = sender_mail
        .rows
        .iter()
        .find(|row| row.summary == "fixture acknowledgement")
        .expect("one persisted acknowledgement reply");
    assert_eq!(acknowledgement.from.as_str(), TEST_RECIPIENT);
    assert!(!acknowledgement.pending_ack);
}
