mod ops;
mod stmt_cache;

pub(crate) use ops::{WriteOp, WriteOpResult, validate_upsert_message_request};

use crate::observability::{
    SqliteObservability, SqliteObservabilityEvent, SqliteObservabilityOutcome,
};
use crate::shared_db::{
    SharedDbTarget, SqliteConnection, ensure_schema, open_writer_connection_for_target,
    sqlite_error,
};
use atm_storage::{AtmError, AtmErrorCode};
use rusqlite::TransactionBehavior;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub(crate) const CHANNEL_CAPACITY: usize = 256;
// Coalesce writes that arrive immediately after the first admission so one
// SQLite commit can durably acknowledge the concurrent burst. This is private
// to the sole writer ingress: callers, HTTP, TLS, and benchmark modes cannot
// tune or bypass it.
const BATCH_TIME_BUDGET: Duration = Duration::from_millis(1);
// Bound one write request long enough for a short lock wait + flush cycle while
// still surfacing wedged durable-state work as an actionable timeout.
const WRITE_OP_DEADLINE: Duration = Duration::from_secs(10);
// Keep shutdown bounded if the writer thread stalls after a queue drain or
// filesystem delay; callers can restart cleanly after this deadline expires.
const WRITER_SHUTDOWN_JOIN_DEADLINE: Duration = Duration::from_secs(5);
const SUBMIT_RETRY_INTERVAL: Duration = Duration::from_millis(5);

enum ReplyTx {
    Sync(SyncSender<Result<WriteOpResult, AtmError>>),
    Async(tokio::sync::oneshot::Sender<Result<WriteOpResult, AtmError>>),
}

impl ReplyTx {
    fn send(self, result: Result<WriteOpResult, AtmError>) {
        match self {
            Self::Sync(sender) => {
                let _ = sender.send(result);
            }
            Self::Async(sender) => {
                let _ = sender.send(result);
            }
        }
    }
}

enum WriterMessage {
    Submit {
        op: Box<WriteOp>,
        reply: ReplyTx,
        _permit: OwnedSemaphorePermit,
    },
    Shutdown,
}

struct QueuedWrite {
    op: Box<WriteOp>,
    reply: ReplyTx,
    _permit: OwnedSemaphorePermit,
}

pub(crate) struct SqliteWriter {
    sender: Option<SyncSender<WriterMessage>>,
    capacity: Arc<Semaphore>,
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
        let mut connection = open_writer_connection_for_target(target.as_ref())?;
        ensure_schema(&mut connection, target.as_ref())?;

        let (sender, receiver) = mpsc::sync_channel(channel_capacity);
        let capacity = Arc::new(Semaphore::new(channel_capacity));
        let worker_observability = Arc::clone(&observability);
        let worker = thread::Builder::new()
            .name("atm-sqlite-writer".to_string())
            .spawn(move || writer_loop(target, connection, receiver, worker_observability))
            .map_err(|error| {
                let error = AtmError::daemon_unavailable(format!(
                    "failed to start sqlite writer thread: {error}"
                ));
                observability.emit_or_warn(SqliteObservabilityEvent::new(
                    "writer_start",
                    SqliteObservabilityOutcome::Failed,
                    error.message().to_owned(),
                    Some(error.code()),
                ));
                error
            })?;
        Ok(Self {
            sender: Some(sender),
            capacity,
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
                    error.message().to_owned(),
                    Some(error.code()),
                ));
            error
        })?;
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        let deadline = Instant::now() + self.write_op_deadline;
        let permit = loop {
            match Arc::clone(&self.capacity).try_acquire_owned() {
                Ok(permit) => break permit,
                Err(tokio::sync::TryAcquireError::NoPermits) if Instant::now() < deadline => {
                    thread::park_timeout(SUBMIT_RETRY_INTERVAL);
                }
                Err(tokio::sync::TryAcquireError::NoPermits) => {
                    let error = writer_queue_timeout_error(self.write_op_deadline);
                    self.observability
                        .emit_or_warn(SqliteObservabilityEvent::new(
                            "writer_submit",
                            SqliteObservabilityOutcome::Timeout,
                            error.message().to_owned(),
                            Some(error.code()),
                        ));
                    return Err(error);
                }
                Err(tokio::sync::TryAcquireError::Closed) => {
                    let error = writer_channel_closed_error();
                    self.observability
                        .emit_or_warn(SqliteObservabilityEvent::new(
                            "writer_submit",
                            SqliteObservabilityOutcome::Failed,
                            error.message().to_owned(),
                            Some(error.code()),
                        ));
                    return Err(error);
                }
            }
        };
        let message = WriterMessage::Submit {
            op: Box::new(op),
            reply: ReplyTx::Sync(reply_tx),
            _permit: permit,
        };
        loop {
            match sender.try_send(message) {
                Ok(()) => break,
                // The permit is acquired before enqueueing, so a full queue
                // here would violate the writer's fixed-capacity invariant.
                Err(TrySendError::Full(_)) => {
                    let error = writer_channel_closed_error();
                    self.observability
                        .emit_or_warn(SqliteObservabilityEvent::new(
                            "writer_submit",
                            SqliteObservabilityOutcome::Failed,
                            error.message().to_owned(),
                            Some(error.code()),
                        ));
                    return Err(error);
                }
                Err(TrySendError::Disconnected(_)) => {
                    let error = writer_channel_closed_error();
                    self.observability
                        .emit_or_warn(SqliteObservabilityEvent::new(
                            "writer_submit",
                            SqliteObservabilityOutcome::Failed,
                            error.message().to_owned(),
                            Some(error.code()),
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
                            error.message().to_owned(),
                            Some(error.code()),
                        ));
                    error
                }
                RecvTimeoutError::Disconnected => {
                    let error = writer_reply_channel_closed_error();
                    self.observability
                        .emit_or_warn(SqliteObservabilityEvent::new(
                            "writer_reply",
                            SqliteObservabilityOutcome::Failed,
                            error.message().to_owned(),
                            Some(error.code()),
                        ));
                    error
                }
            })?
    }

    /// Enqueues one operation without blocking the Tokio executor, then awaits
    /// the reply from the single synchronous SQLite writer thread.
    pub(crate) async fn submit_async(&self, op: WriteOp) -> Result<WriteOpResult, AtmError> {
        let sender = self.sender.as_ref().ok_or_else(|| {
            let error = writer_channel_closed_error();
            self.observability
                .emit_or_warn(SqliteObservabilityEvent::new(
                    "writer_submit",
                    SqliteObservabilityOutcome::Failed,
                    error.message().to_owned(),
                    Some(error.code()),
                ));
            error
        })?;
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let permit = tokio::time::timeout(
            self.write_op_deadline,
            Arc::clone(&self.capacity).acquire_owned(),
        )
        .await
        .map_err(|_| {
            let error = writer_queue_timeout_error(self.write_op_deadline);
            self.observability
                .emit_or_warn(SqliteObservabilityEvent::new(
                    "writer_submit",
                    SqliteObservabilityOutcome::Timeout,
                    error.message().to_owned(),
                    Some(error.code()),
                ));
            error
        })?
        .map_err(|_| {
            let error = writer_channel_closed_error();
            self.observability
                .emit_or_warn(SqliteObservabilityEvent::new(
                    "writer_submit",
                    SqliteObservabilityOutcome::Failed,
                    error.message().to_owned(),
                    Some(error.code()),
                ));
            error
        })?;
        let message = WriterMessage::Submit {
            op: Box::new(op),
            reply: ReplyTx::Async(reply_tx),
            _permit: permit,
        };
        sender.try_send(message).map_err(|_| {
            let error = writer_channel_closed_error();
            self.observability
                .emit_or_warn(SqliteObservabilityEvent::new(
                    "writer_submit",
                    SqliteObservabilityOutcome::Failed,
                    error.message().to_owned(),
                    Some(error.code()),
                ));
            error
        })?;
        tokio::time::timeout(self.write_op_deadline, reply_rx)
            .await
            .map_err(|_| {
                let error = writer_reply_timeout_error(self.write_op_deadline);
                self.observability
                    .emit_or_warn(SqliteObservabilityEvent::new(
                        "writer_reply",
                        SqliteObservabilityOutcome::Timeout,
                        error.message().to_owned(),
                        Some(error.code()),
                    ));
                error
            })?
            .map_err(|_| {
                let error = writer_reply_channel_closed_error();
                self.observability
                    .emit_or_warn(SqliteObservabilityEvent::new(
                        "writer_reply",
                        SqliteObservabilityOutcome::Failed,
                        error.message().to_owned(),
                        Some(error.code()),
                    ));
                error
            })?
    }
}

impl Drop for SqliteWriter {
    fn drop(&mut self) {
        let sender = self.sender.take();
        let worker = self.worker.take();

        if let Some(sender) = sender {
            match sender.try_send(WriterMessage::Shutdown) {
                Ok(()) | Err(TrySendError::Disconnected(_)) => {}
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
    AtmError::daemon_unavailable("sqlite writer submission channel closed")
}

fn writer_queue_timeout_error(deadline: Duration) -> AtmError {
    AtmError::daemon_unavailable(format!(
        "sqlite writer submission queue did not accept a write within {:?}",
        deadline
    ))
}

fn writer_reply_timeout_error(deadline: Duration) -> AtmError {
    AtmError::daemon_unavailable(format!(
        "sqlite writer reply did not arrive within {:?}",
        deadline
    ))
}

fn writer_reply_channel_closed_error() -> AtmError {
    AtmError::daemon_unavailable("sqlite writer reply channel closed")
}

fn writer_unavailable_reply_error() -> AtmError {
    AtmError::daemon_unavailable("sqlite writer is unavailable during shutdown")
}

fn drain_submit_replies(receiver: &Receiver<WriterMessage>) {
    loop {
        match receiver.try_recv() {
            Ok(WriterMessage::Submit { reply, .. }) => {
                reply.send(Err(writer_unavailable_reply_error()));
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
            error.message().to_owned(),
            Some(error.code()),
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
        collect_batch(&receiver, &mut batch, &mut shutting_down);
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
            Ok(WriterMessage::Submit { op, reply, _permit }) => {
                Some(QueuedWrite { op, reply, _permit })
            }
            Ok(WriterMessage::Shutdown) => {
                drain_submit_replies(receiver);
                None
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
        }
    } else {
        match receiver.recv() {
            Ok(WriterMessage::Submit { op, reply, _permit }) => {
                Some(QueuedWrite { op, reply, _permit })
            }
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
) {
    let deadline = Instant::now() + BATCH_TIME_BUDGET;
    loop {
        match receiver.try_recv() {
            Ok(WriterMessage::Submit { op, reply, _permit }) => {
                batch.push(QueuedWrite { op, reply, _permit });
                continue;
            }
            Ok(WriterMessage::Shutdown) => {
                *shutting_down = true;
                continue;
            }
            Err(TryRecvError::Disconnected) => {
                *shutting_down = true;
                return;
            }
            Err(TryRecvError::Empty) => {
                if *shutting_down {
                    return;
                }
                let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                    return;
                };
                if remaining.is_zero() {
                    return;
                }
                match receiver.recv_timeout(remaining) {
                    Ok(WriterMessage::Submit { op, reply, _permit }) => {
                        batch.push(QueuedWrite { op, reply, _permit });
                    }
                    Ok(WriterMessage::Shutdown) => {
                        *shutting_down = true;
                    }
                    Err(RecvTimeoutError::Timeout) => return,
                    Err(RecvTimeoutError::Disconnected) => {
                        *shutting_down = true;
                        return;
                    }
                }
            }
        }
    }
}

fn process_batch(
    target: &SharedDbTarget,
    connection: &mut SqliteConnection,
    cache: &mut stmt_cache::WriterStatementCache,
    batch: Vec<QueuedWrite>,
) {
    let batch_len = batch.len();
    let mut transaction = match connection.transaction_with_behavior(TransactionBehavior::Immediate)
    {
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
        reply.send(final_result);
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
        queued.reply.send(Err(copy_error(target, &error)));
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
            Err(AtmError::daemon_unavailable(
                "sqlite writer operation panicked",
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
    let _ = target;
    error.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queued_write() -> WriterMessage {
        let (reply, _receiver) = mpsc::sync_channel(1);
        WriterMessage::Submit {
            op: Box::new(WriteOp::UpsertMessages(Vec::new())),
            reply: ReplyTx::Sync(reply),
            _permit: Arc::new(Semaphore::new(1))
                .try_acquire_owned()
                .expect("test permit"),
        }
    }

    fn first_queued_write(receiver: &Receiver<WriterMessage>) -> QueuedWrite {
        let WriterMessage::Submit { op, reply, _permit } = receiver.recv().expect("first write")
        else {
            panic!("test queue contains only submit messages");
        };
        QueuedWrite { op, reply, _permit }
    }

    #[test]
    fn batch_collection_drains_all_queued_writes_within_its_window() {
        let (sender, receiver) = mpsc::sync_channel(128);
        sender.send(queued_write()).expect("first queued write");
        for _ in 0..96 {
            sender.send(queued_write()).expect("queued write");
        }
        let first = first_queued_write(&receiver);
        let mut batch = vec![first];
        let mut shutting_down = false;
        collect_batch(&receiver, &mut batch, &mut shutting_down);

        assert_eq!(batch.len(), 97);
        assert!(!shutting_down);
    }

    #[test]
    fn batch_collection_leaves_write_received_after_window_for_next_transaction() {
        let (sender, receiver) = mpsc::sync_channel(8);
        sender.send(queued_write()).expect("first queued write");
        let first = first_queued_write(&receiver);
        let mut batch = vec![first];
        let mut shutting_down = false;
        collect_batch(&receiver, &mut batch, &mut shutting_down);

        assert_eq!(batch.len(), 1);
        assert!(!shutting_down);
        sender.send(queued_write()).expect("later queued write");
        assert!(matches!(
            receiver.try_recv(),
            Ok(WriterMessage::Submit { .. })
        ));
    }

    #[test]
    fn batch_collection_drains_an_already_queued_shutdown() {
        let (sender, receiver) = mpsc::sync_channel(8);
        sender.send(queued_write()).expect("first queued write");
        sender.send(queued_write()).expect("queued write");
        sender
            .send(WriterMessage::Shutdown)
            .expect("queued shutdown");
        let first = first_queued_write(&receiver);
        let mut batch = vec![first];
        let mut shutting_down = false;
        collect_batch(&receiver, &mut batch, &mut shutting_down);

        assert_eq!(batch.len(), 2);
        assert!(shutting_down);
    }

    #[test]
    fn batch_collection_marks_disconnected_receiver_for_shutdown() {
        let (sender, receiver) = mpsc::sync_channel(8);
        sender.send(queued_write()).expect("first queued write");
        let first = first_queued_write(&receiver);
        drop(sender);
        let mut batch = vec![first];
        let mut shutting_down = false;

        collect_batch(&receiver, &mut batch, &mut shutting_down);

        assert_eq!(batch.len(), 1);
        assert!(shutting_down);
    }
}
