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
}
