mod ops;
mod stmt_cache;

pub(crate) use ops::{WriteOp, WriteOpResult, validate_upsert_message_request};

use crate::shared_db::{
    SharedDbTarget, SqliteConnection, ensure_schema, open_connection_for_target, sqlite_error,
};
use crate::{SqliteObservability, SqliteObservabilityEvent, SqliteObservabilityOutcome};
use atm_core::error::{AtmError, AtmErrorCode};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub(crate) const CHANNEL_CAPACITY: usize = 256;
pub(crate) const BATCH_SIZE_MAX: usize = 64;
pub(crate) const BATCH_TIME_BUDGET: Duration = Duration::from_millis(2);
const WRITE_OP_DEADLINE: Duration = Duration::from_secs(10);
const WRITER_SHUTDOWN_JOIN_DEADLINE: Duration = Duration::from_secs(5);
const SUBMIT_RETRY_INTERVAL: Duration = Duration::from_millis(5);

type ReplyTx = SyncSender<Result<WriteOpResult, AtmError>>;

enum WriterMessage {
    Submit { op: Box<WriteOp>, reply: ReplyTx },
    Shutdown,
}

struct QueuedWrite {
    op: Box<WriteOp>,
    reply: ReplyTx,
}

pub(crate) struct SqliteWriter {
    sender: Option<SyncSender<WriterMessage>>,
    worker: Option<JoinHandle<()>>,
    observability: Arc<dyn SqliteObservability>,
    write_op_deadline: Duration,
    shutdown_join_deadline: Duration,
}

impl std::fmt::Debug for SqliteWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteWriter")
            .field("sender_present", &self.sender.is_some())
            .field("worker_present", &self.worker.is_some())
            .field("write_op_deadline", &self.write_op_deadline)
            .field("shutdown_join_deadline", &self.shutdown_join_deadline)
            .finish()
    }
}

impl SqliteWriter {
    pub(crate) fn start(
        target: Arc<SharedDbTarget>,
        observability: Arc<dyn SqliteObservability>,
    ) -> Result<Self, AtmError> {
        Self::start_with_settings(
            target,
            observability,
            CHANNEL_CAPACITY,
            WRITE_OP_DEADLINE,
            WRITER_SHUTDOWN_JOIN_DEADLINE,
        )
    }

    fn start_with_settings(
        target: Arc<SharedDbTarget>,
        observability: Arc<dyn SqliteObservability>,
        channel_capacity: usize,
        write_op_deadline: Duration,
        shutdown_join_deadline: Duration,
    ) -> Result<Self, AtmError> {
        let mut connection = open_connection_for_target(target.as_ref())?;
        ensure_schema(&mut connection, target.as_ref())?;

        let (sender, receiver) = mpsc::sync_channel(channel_capacity);
        let worker_observability = Arc::clone(&observability);
        let worker = thread::Builder::new()
            .name("atm-sqlite-writer".to_string())
            .spawn(move || writer_loop(target, connection, receiver, worker_observability))
            .map_err(|error| {
                let error = AtmError::daemon_unavailable(format!(
                    "failed to start sqlite writer thread: {error}"
                ))
                .with_recovery(
                    "Inspect process thread limits or host resource exhaustion before retrying sqlite writer startup.",
                )
                .with_source(error);
                observability.emit_or_warn(SqliteObservabilityEvent::new(
                    "writer_start",
                    SqliteObservabilityOutcome::Failed,
                    error.message.clone(),
                    Some(error.code),
                ));
                error
            })?;
        Ok(Self {
            sender: Some(sender),
            worker: Some(worker),
            observability,
            write_op_deadline,
            shutdown_join_deadline,
        })
    }

    pub(crate) fn submit(&self, op: WriteOp) -> Result<WriteOpResult, AtmError> {
        let sender = self.sender.as_ref().ok_or_else(|| {
            let error = writer_channel_closed_error();
            self.observability
                .emit_or_warn(SqliteObservabilityEvent::new(
                    "writer_submit",
                    SqliteObservabilityOutcome::Failed,
                    error.message.clone(),
                    Some(error.code),
                ));
            error
        })?;
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        let deadline = Instant::now() + self.write_op_deadline;
        let mut message = WriterMessage::Submit {
            op: Box::new(op),
            reply: reply_tx,
        };
        loop {
            match sender.try_send(message) {
                Ok(()) => break,
                Err(TrySendError::Full(returned)) => {
                    if Instant::now() >= deadline {
                        let error = writer_queue_timeout_error(self.write_op_deadline);
                        self.observability
                            .emit_or_warn(SqliteObservabilityEvent::new(
                                "writer_submit",
                                SqliteObservabilityOutcome::Timeout,
                                error.message.clone(),
                                Some(error.code),
                            ));
                        return Err(error);
                    }
                    message = returned;
                    thread::park_timeout(SUBMIT_RETRY_INTERVAL);
                }
                Err(TrySendError::Disconnected(_)) => {
                    let error = writer_channel_closed_error();
                    self.observability
                        .emit_or_warn(SqliteObservabilityEvent::new(
                            "writer_submit",
                            SqliteObservabilityOutcome::Failed,
                            error.message.clone(),
                            Some(error.code),
                        ));
                    return Err(error);
                }
            }
        }
        reply_rx
            .recv_timeout(self.write_op_deadline)
            .map_err(|error| match error {
                RecvTimeoutError::Timeout => {
                    let error = writer_reply_timeout_error(self.write_op_deadline);
                    self.observability
                        .emit_or_warn(SqliteObservabilityEvent::new(
                            "writer_reply",
                            SqliteObservabilityOutcome::Timeout,
                            error.message.clone(),
                            Some(error.code),
                        ));
                    error
                }
                RecvTimeoutError::Disconnected => {
                    let error = writer_reply_channel_closed_error();
                    self.observability
                        .emit_or_warn(SqliteObservabilityEvent::new(
                            "writer_reply",
                            SqliteObservabilityOutcome::Failed,
                            error.message.clone(),
                            Some(error.code),
                        ));
                    error
                }
            })?
    }

    #[cfg(test)]
    fn start_for_test(
        target: Arc<SharedDbTarget>,
        observability: Arc<dyn SqliteObservability>,
        channel_capacity: usize,
        write_op_deadline: Duration,
        shutdown_join_deadline: Duration,
    ) -> Result<Self, AtmError> {
        Self::start_with_settings(
            target,
            observability,
            channel_capacity,
            write_op_deadline,
            shutdown_join_deadline,
        )
    }
}

impl Drop for SqliteWriter {
    fn drop(&mut self) {
        let sender = self.sender.take();
        let worker = self.worker.take();

        if let Some(sender) = sender {
            match sender.try_send(WriterMessage::Shutdown) {
                Ok(()) | Err(TrySendError::Disconnected(_)) => {}
                // Accepted risk: std::sync::mpsc::SyncSender does not expose a
                // queue-depth probe here, so shutdown logging cannot report an
                // exact depth without replacing the channel primitive.
                Err(TrySendError::Full(_)) => {
                    let detail = "sqlite writer shutdown signal skipped because the bounded queue was full; relying on channel disconnect to let the writer exit once in-flight work drains";
                    tracing::warn!("{detail}");
                    self.observability
                        .emit_or_warn(SqliteObservabilityEvent::new(
                            "writer_shutdown_signal",
                            SqliteObservabilityOutcome::Failed,
                            detail,
                            Some(AtmErrorCode::DaemonUnavailable),
                        ));
                }
            }
            drop(sender);
        }

        if let Some(handle) = worker {
            let (result_tx, result_rx) = mpsc::sync_channel(1);
            let join_helper = thread::spawn(move || {
                let _ = result_tx.send(handle.join());
            });
            match result_rx.recv_timeout(self.shutdown_join_deadline) {
                Ok(Ok(())) => {
                    let _ = join_helper.join();
                }
                Ok(Err(_)) => {
                    let _ = join_helper.join();
                    let detail = "sqlite writer thread panicked while shutting down; the durable write lane may have exited mid-drain";
                    tracing::warn!("{detail}");
                    self.observability
                        .emit_or_warn(SqliteObservabilityEvent::new(
                            "writer_shutdown_join",
                            SqliteObservabilityOutcome::Failed,
                            detail,
                            Some(AtmErrorCode::DaemonUnavailable),
                        ));
                }
                Err(RecvTimeoutError::Timeout) => {
                    drop(join_helper);
                    let detail = format!(
                        "sqlite writer shutdown exceeded the bounded join deadline ({:?}); detaching join helper",
                        self.shutdown_join_deadline
                    );
                    tracing::warn!(
                        timeout_ms = self.shutdown_join_deadline.as_millis(),
                        "{detail}"
                    );
                    self.observability
                        .emit_or_warn(SqliteObservabilityEvent::new(
                            "writer_shutdown_join",
                            SqliteObservabilityOutcome::Timeout,
                            detail,
                            Some(AtmErrorCode::DaemonUnavailable),
                        ));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let _ = join_helper.join();
                    let detail = "sqlite writer join helper disconnected before reporting the worker shutdown result";
                    tracing::warn!("{detail}");
                    self.observability
                        .emit_or_warn(SqliteObservabilityEvent::new(
                            "writer_shutdown_join",
                            SqliteObservabilityOutcome::Failed,
                            detail,
                            Some(AtmErrorCode::DaemonUnavailable),
                        ));
                }
            }
        }
    }
}

fn writer_channel_closed_error() -> AtmError {
    AtmError::daemon_unavailable("sqlite writer submission channel closed").with_recovery(
        "Restart the ATM daemon or reopen the sqlite boundary assembly before retrying the write.",
    )
}

fn writer_queue_timeout_error(deadline: Duration) -> AtmError {
    AtmError::daemon_unavailable(format!(
        "sqlite writer submission queue did not accept a write within {:?}",
        deadline
    ))
    .with_recovery(
        "Retry after the sqlite writer backlog drains or restart the ATM daemon if the writer lane is stalled.",
    )
}

fn writer_reply_timeout_error(deadline: Duration) -> AtmError {
    AtmError::daemon_unavailable(format!(
        "sqlite writer reply did not arrive within {:?}",
        deadline
    ))
    .with_recovery(
        "Retry after the sqlite writer backlog drains or restart the ATM daemon if the writer lane is stalled.",
    )
}

fn writer_reply_channel_closed_error() -> AtmError {
    AtmError::daemon_unavailable("sqlite writer reply channel closed").with_recovery(
        "Restart the ATM daemon or reopen the sqlite boundary assembly before retrying the write.",
    )
}

fn writer_unavailable_reply_error() -> AtmError {
    AtmError::daemon_unavailable("sqlite writer is unavailable during shutdown").with_recovery(
        "Retry after the sqlite boundary assembly is restarted and the writer lane is accepting submissions again.",
    )
}

fn drain_submit_replies(receiver: &Receiver<WriterMessage>) {
    loop {
        match receiver.try_recv() {
            Ok(WriterMessage::Submit { reply, .. }) => {
                let _ = reply.send(Err(writer_unavailable_reply_error()));
            }
            Ok(WriterMessage::Shutdown) => continue,
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
        }
    }
}

fn checkpoint_writer_connection(
    target: &SharedDbTarget,
    connection: &mut SqliteConnection,
    observability: &dyn SqliteObservability,
) {
    #[cfg(test)]
    if matches!(target, SharedDbTarget::InMemory { .. }) {
        return;
    }

    if let Err(error) = connection.query_row("PRAGMA wal_checkpoint(PASSIVE);", [], |_row| Ok(())) {
        let error = sqlite_error(
            target,
            "sqlite writer final wal checkpoint failed after draining the write lane",
            error,
        );
        tracing::warn!(
            path = %target.display(),
            %error,
            "sqlite writer final wal checkpoint failed after draining the write lane"
        );
        observability.emit_or_warn(SqliteObservabilityEvent::new(
            "writer_shutdown_checkpoint",
            SqliteObservabilityOutcome::Failed,
            error.message.clone(),
            Some(error.code),
        ));
    }
}

fn writer_loop(
    target: Arc<SharedDbTarget>,
    mut connection: SqliteConnection,
    receiver: Receiver<WriterMessage>,
    observability: Arc<dyn SqliteObservability>,
) {
    let mut cache = stmt_cache::WriterStatementCache;
    let mut shutting_down = false;
    loop {
        let Some(first) = receive_first_message(&receiver, shutting_down) else {
            break;
        };
        let mut batch = vec![first];
        if collect_batch(&receiver, &mut batch, &mut shutting_down).is_none() {
            break;
        }
        process_batch(&target, &mut connection, &mut cache, batch);
    }
    checkpoint_writer_connection(target.as_ref(), &mut connection, observability.as_ref());
}

fn receive_first_message(
    receiver: &Receiver<WriterMessage>,
    shutting_down: bool,
) -> Option<QueuedWrite> {
    if shutting_down {
        match receiver.try_recv() {
            Ok(WriterMessage::Submit { op, reply }) => Some(QueuedWrite { op, reply }),
            Ok(WriterMessage::Shutdown) => {
                drain_submit_replies(receiver);
                None
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
        }
    } else {
        match receiver.recv() {
            Ok(WriterMessage::Submit { op, reply }) => Some(QueuedWrite { op, reply }),
            Ok(WriterMessage::Shutdown) => {
                drain_submit_replies(receiver);
                None
            }
            Err(_) => None,
        }
    }
}

fn collect_batch(
    receiver: &Receiver<WriterMessage>,
    batch: &mut Vec<QueuedWrite>,
    shutting_down: &mut bool,
) -> Option<()> {
    let deadline = Instant::now() + BATCH_TIME_BUDGET;
    while batch.len() < BATCH_SIZE_MAX {
        match receiver.try_recv() {
            Ok(WriterMessage::Submit { op, reply }) => {
                batch.push(QueuedWrite { op, reply });
                continue;
            }
            Ok(WriterMessage::Shutdown) => {
                *shutting_down = true;
                continue;
            }
            Err(TryRecvError::Disconnected) => {
                *shutting_down = true;
                return Some(());
            }
            Err(TryRecvError::Empty) => {
                let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                    break;
                };
                if remaining.is_zero() {
                    break;
                }
                match receiver.recv_timeout(remaining) {
                    Ok(WriterMessage::Submit { op, reply }) => {
                        batch.push(QueuedWrite { op, reply });
                    }
                    Ok(WriterMessage::Shutdown) => {
                        *shutting_down = true;
                    }
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => {
                        *shutting_down = true;
                        return Some(());
                    }
                }
            }
        }
    }
    Some(())
}

fn process_batch(
    target: &SharedDbTarget,
    connection: &mut SqliteConnection,
    cache: &mut stmt_cache::WriterStatementCache,
    batch: Vec<QueuedWrite>,
) {
    let batch_len = batch.len();
    let mut transaction = match connection.transaction() {
        Ok(transaction) => transaction,
        Err(error) => {
            send_batch_transaction_open_error(target, batch, error);
            return;
        }
    };

    let mut replies: Vec<(ReplyTx, Result<WriteOpResult, AtmError>)> =
        Vec::with_capacity(batch_len);
    for queued in batch {
        replies.push(process_queued_write(
            target,
            &mut transaction,
            cache,
            queued,
        ));
    }

    let commit_error = transaction.commit().err().map(|error| {
        sqlite_error(
            target,
            "failed to commit sqlite writer batch transaction",
            error,
        )
    });
    for (reply, result) in replies {
        let final_result = if let Some(error) = &commit_error {
            match result {
                Ok(_) => Err(copy_error(target, error)),
                Err(existing) => Err(existing),
            }
        } else {
            result
        };
        let _ = reply.send(final_result);
    }
}

fn send_batch_transaction_open_error(
    target: &SharedDbTarget,
    batch: Vec<QueuedWrite>,
    error: rusqlite::Error,
) {
    let error = sqlite_error(
        target,
        "failed to open sqlite writer batch transaction",
        error,
    );
    for queued in batch {
        let _ = queued.reply.send(Err(copy_error(target, &error)));
    }
}

fn process_queued_write(
    target: &SharedDbTarget,
    transaction: &mut rusqlite::Transaction<'_>,
    cache: &mut stmt_cache::WriterStatementCache,
    queued: QueuedWrite,
) -> (ReplyTx, Result<WriteOpResult, AtmError>) {
    let savepoint = match transaction.savepoint() {
        Ok(savepoint) => savepoint,
        Err(error) => {
            return (
                queued.reply,
                Err(sqlite_error(
                    target,
                    "failed to open sqlite writer savepoint",
                    error,
                )),
            );
        }
    };

    let result = catch_unwind(AssertUnwindSafe(|| {
        ops::execute(&queued.op, &savepoint, cache, target)
    }));
    let reply = queued.reply;
    (reply, finalize_queued_write(target, savepoint, result))
}

fn finalize_queued_write(
    target: &SharedDbTarget,
    savepoint: rusqlite::Savepoint<'_>,
    result: std::thread::Result<Result<WriteOpResult, AtmError>>,
) -> Result<WriteOpResult, AtmError> {
    match result {
        Ok(Ok(op_result)) => commit_savepoint(target, savepoint, op_result),
        Ok(Err(error)) => {
            drop(savepoint);
            Err(error)
        }
        Err(_) => {
            drop(savepoint);
            Err(AtmError::daemon_unavailable("sqlite writer operation panicked").with_recovery(
                "Inspect the sqlite writer hot-path operation and retry after correcting the panic source.",
            ))
        }
    }
}

fn commit_savepoint(
    target: &SharedDbTarget,
    savepoint: rusqlite::Savepoint<'_>,
    op_result: WriteOpResult,
) -> Result<WriteOpResult, AtmError> {
    savepoint
        .commit()
        .map(|()| op_result)
        .map_err(|error| sqlite_error(target, "failed to commit sqlite writer savepoint", error))
}

fn copy_error(target: &SharedDbTarget, error: &AtmError) -> AtmError {
    let mut copied = match error
        .source
        .as_deref()
        .and_then(|source| source.downcast_ref::<rusqlite::Error>())
    {
        Some(rusqlite::Error::SqliteFailure(inner, _))
            if matches!(
                inner.code,
                rusqlite::ffi::ErrorCode::DatabaseBusy | rusqlite::ffi::ErrorCode::DatabaseLocked
            ) =>
        {
            match target {
                SharedDbTarget::Path(path) => AtmError::mailbox_lock_timeout(path),
                #[cfg(test)]
                SharedDbTarget::InMemory { .. } => AtmError::mailbox_lock(format!(
                    "timed out waiting for sqlite database lock on {}",
                    target.display()
                )),
            }
        }
        _ => match error.code {
            AtmErrorCode::DaemonUnavailable => AtmError::daemon_unavailable(error.message.clone()),
            _ => AtmError::mailbox_write(error.message.clone()),
        },
    };
    copied.code = error.code;
    copied.recovery = error.recovery.clone();
    copied
}

#[cfg(test)]
mod tests {
    use super::*;
    use atm_core::MessageKey;
    use atm_core::boundary;
    use atm_core::schema::{AtmMessageId, MessageEnvelope};
    use atm_core::types::{AgentName, IsoTimestamp, TaskId, TeamName};
    use rusqlite::OptionalExtension;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::thread;

    static NEXT_TEST_DB_ID: AtomicU64 = AtomicU64::new(1);

    fn in_memory_target() -> Arc<SharedDbTarget> {
        Arc::new(SharedDbTarget::InMemory {
            uri: format!(
                "file:atm-rusqlite-writer-test-{}?mode=memory&cache=shared",
                NEXT_TEST_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        })
    }

    fn team() -> TeamName {
        "test-team".parse().expect("team")
    }

    fn agent() -> AgentName {
        "test-agent".parse().expect("agent")
    }

    fn actor() -> AgentName {
        "test-actor".parse().expect("actor")
    }

    fn task_id() -> TaskId {
        "task-123".parse().expect("task")
    }

    fn message_key(value: &str) -> MessageKey {
        MessageKey::new(value).expect("message key")
    }

    fn envelope(text: &str) -> MessageEnvelope {
        MessageEnvelope {
            from: actor(),
            text: text.to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(team()),
            summary: Some(format!("summary: {text}")),
            message_id: None,
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: Some(task_id()),
            extra: serde_json::Map::new(),
        }
    }

    fn message_record(key: &str, text: &str) -> boundary::MailStoreMessageRecord {
        boundary::MailStoreMessageRecord {
            team: team(),
            agent: agent(),
            message_key: message_key(key),
            envelope: envelope(text),
        }
    }

    #[derive(Default)]
    struct RecordingSqliteObservability {
        events: Mutex<Vec<crate::SqliteObservabilityEvent>>,
    }

    impl crate::SqliteObservability for RecordingSqliteObservability {
        fn emit(&self, event: crate::SqliteObservabilityEvent) -> Result<(), AtmError> {
            self.events
                .lock()
                .expect("sqlite observability events")
                .push(event);
            Ok(())
        }
    }

    fn upsert_message_request(index: usize) -> WriteOp {
        WriteOp::UpsertMessage(Box::new(boundary::MailStoreUpsertMessageRequest {
            record: boundary::MailStoreMessageRecord {
                team: team(),
                agent: agent(),
                message_key: message_key(&format!("atm:writer-{index}")),
                envelope: envelope(&format!("writer message {index}")),
            },
        }))
    }

    fn message_count(target: &SharedDbTarget) -> i64 {
        message_count_in_connection(target)
    }

    fn message_count_in_connection(target: &SharedDbTarget) -> i64 {
        let connection = open_connection_for_target(target).expect("open verifier connection");
        connection
            .query_row("SELECT COUNT(1) FROM mail_messages;", [], |row| row.get(0))
            .expect("count rows")
    }

    #[test]
    fn writer_drop_drains_queued_messages_before_exit() {
        let target = in_memory_target();
        let writer = SqliteWriter::start_for_test(
            Arc::clone(&target),
            Arc::new(crate::NullSqliteObservability),
            CHANNEL_CAPACITY,
            Duration::from_millis(50),
            Duration::from_secs(1),
        )
        .expect("writer");
        let _verifier = open_connection_for_target(target.as_ref()).expect("open verifier");

        let mut replies = Vec::new();
        for index in 0..8 {
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            writer
                .sender
                .as_ref()
                .expect("sender")
                .send(WriterMessage::Submit {
                    op: Box::new(upsert_message_request(index)),
                    reply: reply_tx,
                })
                .expect("queue submit");
            replies.push(reply_rx);
        }

        drop(writer);

        for reply in replies {
            assert!(reply.recv().expect("reply").is_ok());
        }
        assert_eq!(message_count_in_connection(target.as_ref()), 8);
    }

    #[test]
    fn writer_commits_more_than_one_batch_boundary_of_messages() {
        let target = in_memory_target();
        let writer = SqliteWriter::start_for_test(
            Arc::clone(&target),
            Arc::new(crate::NullSqliteObservability),
            CHANNEL_CAPACITY,
            Duration::from_millis(50),
            Duration::from_secs(1),
        )
        .expect("writer");
        let _verifier = open_connection_for_target(target.as_ref()).expect("open verifier");

        let mut replies = Vec::new();
        for index in 0..(BATCH_SIZE_MAX + 6) {
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            writer
                .sender
                .as_ref()
                .expect("sender")
                .send(WriterMessage::Submit {
                    op: Box::new(upsert_message_request(index)),
                    reply: reply_tx,
                })
                .expect("queue submit");
            replies.push(reply_rx);
        }

        drop(writer);

        for reply in replies {
            assert!(reply.recv().expect("reply").is_ok());
        }
        assert_eq!(
            message_count_in_connection(target.as_ref()),
            (BATCH_SIZE_MAX + 6) as i64
        );
    }

    #[test]
    fn shutdown_first_drains_pending_submitters_with_daemon_unavailable() {
        let (sender, receiver) = mpsc::sync_channel(4);
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);

        sender
            .send(WriterMessage::Shutdown)
            .expect("queue shutdown");
        sender
            .send(WriterMessage::Submit {
                op: Box::new(upsert_message_request(0)),
                reply: reply_tx,
            })
            .expect("queue submit");
        drop(sender);

        assert!(receive_first_message(&receiver, false).is_none());
        let error = reply_rx
            .recv()
            .expect("reply")
            .expect_err("daemon unavailable");
        assert_eq!(error.code, AtmErrorCode::DaemonUnavailable);
    }

    #[test]
    fn submit_times_out_when_the_queue_is_full() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let (reply_tx, _reply_rx) = mpsc::sync_channel(1);
        let observability = Arc::new(RecordingSqliteObservability::default());
        sender
            .send(WriterMessage::Submit {
                op: Box::new(upsert_message_request(0)),
                reply: reply_tx,
            })
            .expect("prefill queue");

        let writer = SqliteWriter {
            sender: Some(sender),
            worker: None,
            observability: observability.clone(),
            write_op_deadline: Duration::from_millis(10),
            shutdown_join_deadline: Duration::from_millis(10),
        };
        let error = writer
            .submit(upsert_message_request(1))
            .expect_err("queue full should time out");
        assert_eq!(error.code, AtmErrorCode::DaemonUnavailable);
        assert!(
            error
                .message
                .contains("sqlite writer submission queue did not accept a write")
        );
        let events = observability
            .events
            .lock()
            .expect("sqlite observability events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, "writer_submit");
        assert_eq!(
            events[0].outcome,
            crate::SqliteObservabilityOutcome::Timeout
        );

        drop(receiver);
    }

    #[test]
    fn invalid_message_row_does_not_poison_other_rows_in_same_batch() {
        let target = in_memory_target();
        let mut connection = open_connection_for_target(target.as_ref()).expect("open connection");
        ensure_schema(&mut connection, target.as_ref()).expect("ensure schema");
        let verify = open_connection_for_target(target.as_ref()).expect("open verifier");
        let mut cache = stmt_cache::WriterStatementCache;

        let (invalid_tx, invalid_rx) = mpsc::sync_channel(1);
        let (valid_tx, valid_rx) = mpsc::sync_channel(1);
        process_batch(
            target.as_ref(),
            &mut connection,
            &mut cache,
            vec![
                QueuedWrite {
                    op: Box::new(WriteOp::UpsertMessage(Box::new(
                        boundary::MailStoreUpsertMessageRequest {
                            record: message_record("bad-key", "invalid"),
                        },
                    ))),
                    reply: invalid_tx,
                },
                QueuedWrite {
                    op: Box::new(WriteOp::UpsertMessage(Box::new(
                        boundary::MailStoreUpsertMessageRequest {
                            record: message_record("atm:valid", "valid"),
                        },
                    ))),
                    reply: valid_tx,
                },
            ],
        );

        let invalid = invalid_rx.recv().expect("invalid reply");
        let valid = valid_rx.recv().expect("valid reply");
        assert!(
            invalid
                .expect_err("invalid row should fail")
                .is_validation()
        );
        assert_eq!(
            valid.expect("valid row should survive same batch"),
            WriteOpResult::UpsertMessage { inserted: true }
        );

        let stored: String = verify
            .query_row(
                "SELECT message_text FROM mail_messages WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                rusqlite::params![team().as_str(), agent().as_str(), "atm:valid"],
                |row| row.get(0),
            )
            .expect("stored valid row");
        assert_eq!(stored, "valid");
        let missing: Option<String> = verify
            .query_row(
                "SELECT message_text FROM mail_messages WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                rusqlite::params![team().as_str(), agent().as_str(), "bad-key"],
                |row| row.get(0),
            )
            .optional()
            .expect("query invalid row");
        assert!(missing.is_none(), "invalid row must not be written");
    }

    #[test]
    fn single_successor_violation_does_not_poison_other_rows_in_same_batch() {
        let target = in_memory_target();
        let mut connection = open_connection_for_target(target.as_ref()).expect("open connection");
        ensure_schema(&mut connection, target.as_ref()).expect("ensure schema");
        let verify = open_connection_for_target(target.as_ref()).expect("open verifier");
        let mut cache = stmt_cache::WriterStatementCache;

        let parent_message_id = AtmMessageId::new();
        let mut existing_successor = message_record("atm:existing-successor", "existing");
        existing_successor.envelope.parent_message_id = Some(parent_message_id);

        let (seed_tx, seed_rx) = mpsc::sync_channel(1);
        process_batch(
            target.as_ref(),
            &mut connection,
            &mut cache,
            vec![QueuedWrite {
                op: Box::new(WriteOp::UpsertMessage(Box::new(
                    boundary::MailStoreUpsertMessageRequest {
                        record: existing_successor,
                    },
                ))),
                reply: seed_tx,
            }],
        );
        assert_eq!(
            seed_rx.recv().expect("seed reply").expect("seed insert"),
            WriteOpResult::UpsertMessage { inserted: true }
        );

        let mut conflicting_successor = message_record("atm:conflicting-successor", "conflict");
        conflicting_successor.envelope.parent_message_id = Some(parent_message_id);

        let (root_tx, root_rx) = mpsc::sync_channel(1);
        let (conflict_tx, conflict_rx) = mpsc::sync_channel(1);
        process_batch(
            target.as_ref(),
            &mut connection,
            &mut cache,
            vec![
                QueuedWrite {
                    op: Box::new(WriteOp::UpsertMessage(Box::new(
                        boundary::MailStoreUpsertMessageRequest {
                            record: message_record("atm:root", "root"),
                        },
                    ))),
                    reply: root_tx,
                },
                QueuedWrite {
                    op: Box::new(WriteOp::UpsertMessage(Box::new(
                        boundary::MailStoreUpsertMessageRequest {
                            record: conflicting_successor,
                        },
                    ))),
                    reply: conflict_tx,
                },
            ],
        );

        assert_eq!(
            root_rx.recv().expect("root reply").expect("root insert"),
            WriteOpResult::UpsertMessage { inserted: true }
        );
        assert!(
            conflict_rx
                .recv()
                .expect("conflict reply")
                .expect_err("single-successor violation")
                .is_validation()
        );

        let stored_root: String = verify
            .query_row(
                "SELECT message_text FROM mail_messages WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                rusqlite::params![team().as_str(), agent().as_str(), "atm:root"],
                |row| row.get(0),
            )
            .expect("stored root row");
        assert_eq!(stored_root, "root");
        let conflicting: Option<String> = verify
            .query_row(
                "SELECT message_text FROM mail_messages WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                rusqlite::params![team().as_str(), agent().as_str(), "atm:conflicting-successor"],
                |row| row.get(0),
            )
            .optional()
            .expect("query conflicting row");
        assert!(conflicting.is_none(), "conflicting row must not be written");
    }

    #[test]
    fn writer_duplicate_key_preserves_first_payload() {
        let target = in_memory_target();
        let writer = SqliteWriter::start_for_test(
            Arc::clone(&target),
            Arc::new(crate::NullSqliteObservability),
            CHANNEL_CAPACITY,
            Duration::from_millis(50),
            Duration::from_secs(1),
        )
        .expect("writer");

        let first = writer
            .submit(WriteOp::UpsertMessage(Box::new(
                boundary::MailStoreUpsertMessageRequest {
                    record: message_record("atm:dup-test", "original"),
                },
            )))
            .expect("first insert");
        assert_eq!(first, WriteOpResult::UpsertMessage { inserted: true });

        let second = writer
            .submit(WriteOp::UpsertMessage(Box::new(
                boundary::MailStoreUpsertMessageRequest {
                    record: message_record("atm:dup-test", "overwrite"),
                },
            )))
            .expect("duplicate insert");
        assert_eq!(second, WriteOpResult::UpsertMessage { inserted: false });

        let connection = open_connection_for_target(target.as_ref()).expect("open verifier");
        let (message_text, envelope_json): (String, String) = connection
            .query_row(
                "SELECT message_text, envelope_json FROM mail_messages WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                rusqlite::params![team().as_str(), agent().as_str(), "atm:dup-test"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("stored duplicate row");
        assert_eq!(message_text, "original");
        assert!(envelope_json.contains("\"text\":\"original\""));
        assert!(!envelope_json.contains("\"text\":\"overwrite\""));
    }

    #[test]
    fn deadline_flush_commits_messages_without_waiting_for_batch_capacity() {
        let target = in_memory_target();
        let writer = SqliteWriter::start_for_test(
            Arc::clone(&target),
            Arc::new(crate::NullSqliteObservability),
            CHANNEL_CAPACITY,
            Duration::from_millis(50),
            Duration::from_secs(1),
        )
        .expect("writer");

        let mut replies = Vec::new();
        for index in 0..3 {
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            writer
                .sender
                .as_ref()
                .expect("sender")
                .send(WriterMessage::Submit {
                    op: Box::new(upsert_message_request(index)),
                    reply: reply_tx,
                })
                .expect("queue submit");
            replies.push(reply_rx);
            thread::park_timeout(BATCH_TIME_BUDGET + Duration::from_millis(1));
        }

        for reply in replies {
            assert!(reply.recv().expect("reply").is_ok());
        }
        assert_eq!(message_count(target.as_ref()), 3);

        drop(writer);
    }
}
