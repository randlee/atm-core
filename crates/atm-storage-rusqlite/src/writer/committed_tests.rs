//! Transaction-boundary regression tests for committed task-write replies.

use super::*;
use crate::shared_db::{ensure_schema, open_writer_connection_for_target};
use atm_storage::{AtmMessageId, TaskCloseOutcome, TaskOp};

fn fixture() -> (
    SharedDbTarget,
    SqliteConnection,
    stmt_cache::WriterStatementCache,
) {
    let target = SharedDbTarget::InMemory {
        uri: format!("file:bd2-{}?mode=memory&cache=shared", AtmMessageId::new()),
    };
    let mut connection = open_writer_connection_for_target(&target).unwrap();
    ensure_schema(&mut connection, &target).unwrap();
    (target, connection, stmt_cache::WriterStatementCache)
}

fn missing_close() -> atm_storage::Message {
    let mut message = super::tests::message("atm:bd2-missing-close");
    message.envelope.task_id = Some("MISSING".parse().unwrap());
    message.envelope.task_op = Some(TaskOp::Close {
        outcome: TaskCloseOutcome::Completed,
        reason: None,
    });
    message
}

#[test]
fn bd2_commit_failure_discards_audit_rows_and_preserves_original_error() {
    let (target, mut connection, mut cache) = fixture();
    // Test-only deferred constraint fails the outer COMMIT, after the real
    // Rust task-close operation and its rejection-audit append have run.
    connection
        .execute_batch(
            "CREATE TABLE bd2_parent (id INTEGER PRIMARY KEY);
         CREATE TABLE bd2_deferred (id INTEGER REFERENCES bd2_parent(id)
             DEFERRABLE INITIALLY DEFERRED);
         CREATE TRIGGER bd2_fail_commit AFTER INSERT ON task_events
             BEGIN INSERT INTO bd2_deferred VALUES (1); END;",
        )
        .unwrap();
    let (rejected, rejected_reply) = super::tests::queued_upsert(missing_close());
    let (ordinary, ordinary_reply) =
        super::tests::queued_upsert(super::tests::message("atm:bd2-ordinary"));
    process_batch(
        &target,
        &mut connection,
        &mut cache,
        vec![rejected, ordinary],
    );
    let error = rejected_reply.recv().unwrap().unwrap_err();
    assert_eq!(error.code(), AtmErrorCode::TaskNotFound);
    assert!(ordinary_reply.recv().unwrap().is_err());
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM task_events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM mail_messages", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn bd2_audit_failure_preserves_existing_audit_error_selection() {
    let (target, mut connection, mut cache) = fixture();
    connection
        .execute_batch(
            "CREATE TRIGGER bd2_fail_audit BEFORE INSERT ON task_events
         BEGIN SELECT RAISE(ABORT, 'bd2 audit failure'); END;",
        )
        .unwrap();
    let (queued, reply) = super::tests::queued_upsert(missing_close());
    process_batch(&target, &mut connection, &mut cache, vec![queued]);
    let error = reply.recv().unwrap().unwrap_err();
    assert_eq!(error.code(), AtmError::validation("audit").code());
    assert!(error.message().contains("failed to append task event"));
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM task_events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn bd2_rolled_back_assignment_returns_no_rows() {
    let (target, mut connection, mut cache) = fixture();
    connection
        .execute_batch(
            "CREATE TRIGGER bd2_fail_assignment BEFORE INSERT ON task_events
         WHEN NEW.event = 'assigned'
         BEGIN SELECT RAISE(ABORT, 'bd2 assignment failure'); END;",
        )
        .unwrap();
    let mut message = super::tests::message("atm:bd2-assignment");
    message.envelope.task_id = Some("TASK".parse().unwrap());
    message.envelope.message_id = Some(AtmMessageId::new());
    let (queued, reply) = super::tests::queued_upsert(message);
    process_batch(&target, &mut connection, &mut cache, vec![queued]);
    let committed = reply.recv().unwrap().unwrap();
    assert!(committed.operation.is_err());
    assert!(committed.task_events.is_empty());
    for table in ["task_events", "tasks", "mail_messages"] {
        let count: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "{table} must roll back with the assignment");
    }
}

#[test]
fn bd2_rejection_reply_is_not_sent_before_outer_commit() {
    let (target, mut connection, mut cache) = fixture();
    let mut transaction = connection.transaction().unwrap();
    let (queued, receiver) = super::tests::queued_upsert(missing_close());
    let (reply, result) =
        super::batch::process_queued_write(&target, &mut transaction, &mut cache, queued);
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    let committed = result.as_ref().unwrap();
    assert_eq!(
        committed.operation.as_ref().unwrap_err().code(),
        AtmErrorCode::TaskNotFound
    );
    assert_eq!(committed.task_events.len(), 1);
    transaction.commit().unwrap();
    reply.send(result);
    let returned = receiver.recv().unwrap().unwrap();
    let persisted = crate::task_sql::select_task_events(
        &connection,
        &"writer-test-team".parse().unwrap(),
        &"MISSING".parse().unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(returned.task_events, persisted);
    assert!(
        persisted[0]
            .detail
            .as_deref()
            .is_some_and(|detail| detail.starts_with("ATM_TASK_NOT_FOUND: "))
    );
}

fn assignment(key: &str, task: &str) -> atm_storage::Message {
    let mut message = super::tests::message(key);
    message.envelope.task_id = Some(task.parse().unwrap());
    message.envelope.message_id = Some(AtmMessageId::new());
    message
}

/// Runs one writer batch and returns each reply in submission order.
fn replies(
    target: &SharedDbTarget,
    connection: &mut SqliteConnection,
    cache: &mut stmt_cache::WriterStatementCache,
    messages: Vec<atm_storage::Message>,
) -> Vec<atm_storage::CommittedTaskWrite<WriteOpResult>> {
    let (queued, receivers): (Vec<_>, Vec<_>) = messages
        .into_iter()
        .map(super::tests::queued_upsert)
        .unzip();
    process_batch(target, connection, cache, queued);
    receivers
        .into_iter()
        .map(|receiver| receiver.recv().unwrap().unwrap())
        .collect()
}

/// The admission rows a successful upsert carries in its own result.
fn admitted_rows(committed: &atm_storage::CommittedTaskWrite<WriteOpResult>) -> usize {
    match &committed.operation {
        Ok(WriteOpResult::UpsertMessage { task_events, .. }) => task_events.len(),
        Ok(other) => panic!("unexpected upsert result: {other:?}"),
        Err(_) => 0,
    }
}

/// Positive: on every writer path (single admission, shared-savepoint group,
/// group replayed after a member's rejection) a successful admission returns
/// its rows only in `UpsertMessage::task_events` and a rejected one only in
/// the outer carrier, so the two carriers are never both non-empty.
/// Negative: the rejected close returns exactly its one committed audit row.
/// No-Claim: covers message admissions only; the move and direct-audit owners
/// are pinned by `tests/task_identity/committed_outcomes.rs`.
#[test]
fn committed_rows_have_exactly_one_owner_on_every_admission_path() {
    let (target, mut connection, mut cache) = fixture();
    let single = replies(
        &target,
        &mut connection,
        &mut cache,
        vec![assignment("atm:f10-single", "SINGLE")],
    );
    let group = replies(
        &target,
        &mut connection,
        &mut cache,
        vec![
            assignment("atm:f10-group-a", "GROUP-A"),
            assignment("atm:f10-group-b", "GROUP-B"),
        ],
    );
    let replayed = replies(
        &target,
        &mut connection,
        &mut cache,
        vec![assignment("atm:f10-replayed", "REPLAYED"), missing_close()],
    );
    for committed in single.iter().chain(&group).chain(&replayed[..1]) {
        assert!(committed.operation.is_ok());
        assert!(
            committed.task_events.is_empty(),
            "success rows have one owner"
        );
        assert!(admitted_rows(committed) > 0, "assignment rows are carried");
    }
    let rejected = &replayed[1];
    assert_eq!(
        rejected.operation.as_ref().unwrap_err().code(),
        AtmErrorCode::TaskNotFound
    );
    assert_eq!(admitted_rows(rejected), 0);
    assert_eq!(rejected.task_events.len(), 1);
    assert_eq!(
        rejected.task_events[0].event,
        atm_storage::TaskEventKind::Rejected
    );
}

fn draft_names() -> (
    atm_storage::TeamName,
    atm_storage::TaskId,
    atm_storage::AgentName,
    atm_storage::IsoTimestamp,
) {
    (
        "atm-test".parse().unwrap(),
        "DRAFT".parse().unwrap(),
        "alice".parse().unwrap(),
        atm_storage::IsoTimestamp::now(),
    )
}

#[test]
fn task_event_draft_persists_typed_outcome_marker_and_detail_in_their_columns() {
    let (target, connection, _cache) = fixture();
    let (team, task_id, agent, at) = draft_names();
    let row = task_ops::append_task_event(
        &connection,
        &target,
        &task_ops::TaskEventDraft {
            team: &team,
            task_id: &task_id,
            assignee: &agent,
            at: &at,
            event: atm_storage::TaskEventKind::Reminded,
            from_state: Some(atm_storage::TaskStateTag::Assigned),
            to_state: Some(atm_storage::TaskStateTag::Assigned),
            close_outcome: None,
            actor: &agent,
            message_id: None,
            outcome: Some(atm_storage::ReminderOutcome::Blocked),
            marker: Some(atm_storage::TaskEventMarker::AssignmentMissing),
            detail: Some("note"),
        },
    )
    .unwrap();
    assert_eq!(row.outcome, Some(atm_storage::ReminderOutcome::Blocked));
    assert_eq!(
        row.marker,
        Some(atm_storage::TaskEventMarker::AssignmentMissing)
    );
    assert_eq!(row.detail.as_deref(), Some("note"));
    let stored: (String, String, String) = connection
        .query_row(
            "SELECT outcome, marker, detail FROM task_events WHERE task_id = 'DRAFT'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        stored,
        (
            "blocked".to_owned(),
            "assignment_missing".to_owned(),
            "note".to_owned()
        )
    );
}

#[test]
fn task_event_draft_rejects_an_invalid_state_before_inserting() {
    let (target, connection, _cache) = fixture();
    let (team, task_id, agent, at) = draft_names();
    let error = task_ops::append_task_event(
        &connection,
        &target,
        &task_ops::TaskEventDraft {
            team: &team,
            task_id: &task_id,
            assignee: &agent,
            at: &at,
            event: atm_storage::TaskEventKind::Completed,
            from_state: Some(atm_storage::TaskStateTag::Active),
            to_state: Some(atm_storage::TaskStateTag::Complete),
            close_outcome: None,
            actor: &agent,
            message_id: None,
            outcome: None,
            marker: None,
            detail: None,
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM task_events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0, "no row is written for a rejected state");
}
