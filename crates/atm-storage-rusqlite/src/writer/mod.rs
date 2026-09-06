mod ops;
mod ops_envelope;
mod shutdown_support;
mod stmt_cache;

pub(crate) use ops::{WriteOp, WriteOpResult, validate_upsert_message_request};
use shutdown_support::{
    checkpoint_writer_connection, drain_submit_replies, writer_channel_closed_error,
    writer_queue_timeout_error, writer_reply_channel_closed_error, writer_reply_timeout_error,
};

use crate::DIAGNOSTIC_PRUNE_CHECK_EVERY;
use crate::observability::{
    SqliteObservability, SqliteObservabilityEvent, SqliteObservabilityOutcome,
};
use crate::shared_db::{
    SharedDbTarget, SqliteConnection, ensure_schema, open_writer_connection_for_target,
    sqlite_error,
};
use atm_storage::{AtmError, AtmErrorCode, DiagnosticEvent};
use rusqlite::TransactionBehavior;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub(crate) const CHANNEL_CAPACITY: usize = 256;
/// A diagnostic producer owns at most one bounded batch at a time.
pub(crate) const DIAGNOSTIC_BATCH_MAX: usize = 128;
/// The diagnostic lane never holds more than eight batches behind the writer.
pub(crate) const DIAGNOSTIC_QUEUE_BATCHES: usize = 8;
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

/// The sole mutable SQLite connection. Both typed writer operations and the
/// remaining blocking control-path boundary borrow this queue, so SQLite
/// mutations cannot race on independent writer connections.
pub(crate) struct SerialWriterQueue {
    connection: Mutex<SqliteConnection>,
}

impl SerialWriterQueue {
    pub(crate) fn open(target: &SharedDbTarget) -> Result<Self, AtmError> {
        let mut connection = open_writer_connection_for_target(target)?;
        ensure_schema(&mut connection, target)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub(crate) fn with_connection<T>(
        &self,
        operation: impl FnOnce(&mut SqliteConnection) -> T,
    ) -> T {
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        operation(&mut connection)
    }
}

pub(crate) enum ReplyTx {
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

pub(crate) enum WriterMessage {
    Submit { op: Box<WriteOp>, reply: ReplyTx },
    Shutdown,
}

pub(crate) enum DiagnosticWriterMessage {
    Records(Vec<DiagnosticEvent>),
    Prune {
        now_unix_ms: i64,
        reply: SyncSender<Result<u64, AtmError>>,
    },
}

/// Result of a non-blocking diagnostic-lane offer.  Diagnostic events must
/// never backpressure or fail a primary durable-state operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiagnosticBatchOffer {
    Accepted,
    QueueFull,
    WriterClosed,
    InvalidBatch,
}

/// Persist-side counters are separate from producer queue-full accounting so
/// the health projection never treats a merely accepted batch as durable.
#[derive(Debug, Default)]
pub struct DiagnosticTimelinePersistenceStats {
    written_total: AtomicU64,
    persist_error_total: AtomicU64,
}

impl DiagnosticTimelinePersistenceStats {
    pub fn written_total(&self) -> u64 {
        self.written_total.load(Ordering::Relaxed)
    }

    pub fn persist_error_total(&self) -> u64 {
        self.persist_error_total.load(Ordering::Relaxed)
    }

    /// Simulates a writer-thread persistence failure without a real SQLite
    /// fault, so cross-crate regression tests can exercise counter-fold
    /// logic that only consumes this type's public getters.
    #[cfg(any(test, feature = "test-support"))]
    pub fn increment_persist_error_total_for_test(&self) {
        self.persist_error_total.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) struct QueuedWrite {
    op: Box<WriteOp>,
    reply: ReplyTx,
}

pub(crate) struct SqliteWriter {
    sender: Option<tokio::sync::mpsc::Sender<WriterMessage>>,
    diagnostic_sender: Option<tokio::sync::mpsc::Sender<DiagnosticWriterMessage>>,
    diagnostic_stats: Arc<DiagnosticTimelinePersistenceStats>,
    worker: Option<JoinHandle<()>>,
    observability: Arc<dyn SqliteObservability>,
    write_op_deadline: Duration,
    shutdown_join_deadline: Duration,
}

impl std::fmt::Debug for SqliteWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteWriter")
            .field("sender_present", &self.sender.is_some())
            .field(
                "diagnostic_sender_present",
                &self.diagnostic_sender.is_some(),
            )
            .field("worker_present", &self.worker.is_some())
            .field("write_op_deadline", &self.write_op_deadline)
            .field("shutdown_join_deadline", &self.shutdown_join_deadline)
            .finish()
    }
}

impl SqliteWriter {
    pub(crate) fn start_with_queue(
        target: Arc<SharedDbTarget>,
        observability: Arc<dyn SqliteObservability>,
        serial_queue: Arc<SerialWriterQueue>,
    ) -> Result<Self, AtmError> {
        Self::start_with_settings(
            target,
            observability,
            serial_queue,
            CHANNEL_CAPACITY,
            WRITE_OP_DEADLINE,
            WRITER_SHUTDOWN_JOIN_DEADLINE,
        )
    }

    fn start_with_settings(
        target: Arc<SharedDbTarget>,
        observability: Arc<dyn SqliteObservability>,
        serial_queue: Arc<SerialWriterQueue>,
        channel_capacity: usize,
        write_op_deadline: Duration,
        shutdown_join_deadline: Duration,
    ) -> Result<Self, AtmError> {
        Self::start_with_runtime_builder(
            target,
            observability,
            serial_queue,
            channel_capacity,
            write_op_deadline,
            shutdown_join_deadline,
            || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
            },
        )
    }

    fn start_with_runtime_builder<BuildRuntime>(
        target: Arc<SharedDbTarget>,
        observability: Arc<dyn SqliteObservability>,
        serial_queue: Arc<SerialWriterQueue>,
        channel_capacity: usize,
        write_op_deadline: Duration,
        shutdown_join_deadline: Duration,
        build_runtime: BuildRuntime,
    ) -> Result<Self, AtmError>
    where
        BuildRuntime: FnOnce() -> std::io::Result<tokio::runtime::Runtime>,
    {
        let (sender, receiver) = tokio::sync::mpsc::channel(channel_capacity);
        let (diagnostic_sender, diagnostic_receiver) =
            tokio::sync::mpsc::channel(DIAGNOSTIC_QUEUE_BATCHES);
        let diagnostic_stats = Arc::new(DiagnosticTimelinePersistenceStats::default());
        let worker_runtime = build_runtime().map_err(|error| {
            let error = AtmError::daemon_unavailable(format!(
                "failed to initialize sqlite writer timer: {error}"
            ));
            observability.emit_or_warn(SqliteObservabilityEvent::new(
                "writer_start",
                SqliteObservabilityOutcome::Failed,
                error.message().to_owned(),
                Some(error.code()),
            ));
            error
        })?;
        let worker_observability = Arc::clone(&observability);
        let worker_diagnostic_stats = Arc::clone(&diagnostic_stats);
        let worker = thread::Builder::new()
            .name("atm-sqlite-writer".to_string())
            .spawn(move || {
                writer_loop(
                    target,
                    serial_queue,
                    receiver,
                    diagnostic_receiver,
                    worker_diagnostic_stats,
                    worker_observability,
                    worker_runtime,
                )
            })
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
            diagnostic_sender: Some(diagnostic_sender),
            diagnostic_stats,
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
        let mut message = WriterMessage::Submit {
            op: Box::new(op),
            reply: ReplyTx::Sync(reply_tx),
        };
        loop {
            match sender.try_send(message) {
                Ok(()) => break,
                Err(tokio::sync::mpsc::error::TrySendError::Full(returned)) => {
                    if Instant::now() >= deadline {
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
                    message = returned;
                    thread::park_timeout(SUBMIT_RETRY_INTERVAL);
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
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
        let message = WriterMessage::Submit {
            op: Box::new(op),
            reply: ReplyTx::Async(reply_tx),
        };
        tokio::time::timeout(self.write_op_deadline, sender.send(message))
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

    /// Offers a complete diagnostic batch without ever waiting for the SQLite
    /// worker.  The caller owns queue-full accounting; persist errors are
    /// deliberately isolated on the lower-priority worker lane.
    pub(crate) fn try_record_diagnostics(
        &self,
        batch: Vec<DiagnosticEvent>,
    ) -> DiagnosticBatchOffer {
        if batch.is_empty() || batch.len() > DIAGNOSTIC_BATCH_MAX {
            return DiagnosticBatchOffer::InvalidBatch;
        }
        let Some(sender) = self.diagnostic_sender.as_ref() else {
            return DiagnosticBatchOffer::WriterClosed;
        };
        match sender.try_send(DiagnosticWriterMessage::Records(batch)) {
            Ok(()) => DiagnosticBatchOffer::Accepted,
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => DiagnosticBatchOffer::QueueFull,
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                DiagnosticBatchOffer::WriterClosed
            }
        }
    }

    pub(crate) fn diagnostic_stats(&self) -> Arc<DiagnosticTimelinePersistenceStats> {
        Arc::clone(&self.diagnostic_stats)
    }

    pub(crate) fn prune_diagnostics(&self, now_unix_ms: i64) -> Result<u64, AtmError> {
        let sender = self
            .diagnostic_sender
            .as_ref()
            .ok_or_else(writer_channel_closed_error)?;
        let (reply, receiver) = mpsc::sync_channel(1);
        sender
            .try_send(DiagnosticWriterMessage::Prune { now_unix_ms, reply })
            .map_err(|error| match error {
                tokio::sync::mpsc::error::TrySendError::Full(_) => {
                    writer_queue_timeout_error(self.write_op_deadline)
                }
                tokio::sync::mpsc::error::TrySendError::Closed(_) => writer_channel_closed_error(),
            })?;
        receiver
            .recv_timeout(self.write_op_deadline)
            .map_err(|error| match error {
                RecvTimeoutError::Timeout => writer_reply_timeout_error(self.write_op_deadline),
                RecvTimeoutError::Disconnected => writer_reply_channel_closed_error(),
            })?
    }
}

impl Drop for SqliteWriter {
    fn drop(&mut self) {
        let sender = self.sender.take();
        let diagnostic_sender = self.diagnostic_sender.take();
        let worker = self.worker.take();

        if let Some(sender) = sender {
            match sender.try_send(WriterMessage::Shutdown) {
                Ok(()) | Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {}
                // Accepted risk: std::sync::mpsc::SyncSender does not expose a
                // queue-depth probe here, so shutdown logging cannot report an
                // exact depth without replacing the channel primitive.
                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
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
        drop(diagnostic_sender);

        self.join_worker(worker);
    }
}

impl SqliteWriter {
    fn join_worker(&self, worker: Option<thread::JoinHandle<()>>) {
        let Some(handle) = worker else { return };
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        let join_helper = thread::spawn(move || {
            let _ = result_tx.send(handle.join());
        });
        match result_rx.recv_timeout(self.shutdown_join_deadline) {
            Ok(Ok(())) => { let _ = join_helper.join(); }
            Ok(Err(_)) => self.finish_failed_join(join_helper, "sqlite writer thread panicked while shutting down; the durable write lane may have exited mid-drain"),
            Err(RecvTimeoutError::Timeout) => self.finish_timed_out_join(join_helper),
            Err(RecvTimeoutError::Disconnected) => self.finish_failed_join(join_helper, "sqlite writer join helper disconnected before reporting the worker shutdown result"),
        }
    }

    fn finish_failed_join(&self, join_helper: thread::JoinHandle<()>, detail: &'static str) {
        let _ = join_helper.join();
        tracing::warn!("{detail}");
        self.observability
            .emit_or_warn(SqliteObservabilityEvent::new(
                "writer_shutdown_join",
                SqliteObservabilityOutcome::Failed,
                detail,
                Some(AtmErrorCode::DaemonUnavailable),
            ));
    }

    fn finish_timed_out_join(&self, join_helper: thread::JoinHandle<()>) {
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
}

mod batch;
pub(crate) use batch::writer_loop;
#[cfg(test)]
pub(crate) use batch::{
    WriterWork, collect_batch, process_batch, process_diagnostic_batch, receive_next_work,
};
#[cfg(test)]
mod tests {
    use super::*;
    use crate::observability::NullSqliteObservability;
    use crate::shared_db::{SharedDbTarget, ensure_schema, open_writer_connection_for_target};
    use atm_storage::contract::{Message, MessageKey};
    use atm_storage::schema::MessageEnvelope;
    use atm_storage::types::{AgentName, IsoTimestamp, TeamName};
    use chrono::Utc;
    use rusqlite::params;
    use serde_json::Map;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn queued_primary_work_drains_before_a_full_diagnostic_lane() {
        const PRIMARY_OPS: usize = 10_000;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("test runtime");
        let (primary_tx, mut primary_rx) = tokio::sync::mpsc::channel(PRIMARY_OPS);
        let (diagnostic_tx, mut diagnostic_rx) =
            tokio::sync::mpsc::channel(DIAGNOSTIC_QUEUE_BATCHES);
        for _ in 0..PRIMARY_OPS {
            let (reply, _receiver) = mpsc::sync_channel(1);
            primary_tx
                .try_send(WriterMessage::Submit {
                    op: Box::new(WriteOp::UpsertMessages(Vec::new())),
                    reply: ReplyTx::Sync(reply),
                })
                .expect("primary queue capacity is sized for the test");
        }
        for _ in 0..DIAGNOSTIC_QUEUE_BATCHES {
            diagnostic_tx
                .try_send(DiagnosticWriterMessage::Records(vec![DiagnosticEvent {
                    ts_unix_ms: 0,
                    level: "warn".to_owned(),
                    component: "writer-priority-test".to_owned(),
                    code: None,
                    correlation_id: None,
                    origin: "tracing".to_owned(),
                    message: "diagnostic".to_owned(),
                    detail: None,
                    id: 0,
                }]))
                .expect("diagnostic queue capacity is exactly eight batches");
        }
        for _ in 0..PRIMARY_OPS {
            assert!(matches!(
                runtime.block_on(receive_next_work(
                    &mut primary_rx,
                    &mut diagnostic_rx,
                    false
                )),
                Some(WriterWork::Primary(_))
            ));
        }
        assert!(matches!(
            runtime.block_on(receive_next_work(
                &mut primary_rx,
                &mut diagnostic_rx,
                false
            )),
            Some(WriterWork::Diagnostics(DiagnosticWriterMessage::Records(_)))
        ));
    }

    #[test]
    fn ac3_primary_mailbox_insert_completes_while_diagnostic_lane_is_full() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("test runtime");
        let target = SharedDbTarget::InMemory {
            uri: format!(
                "file:writer-ac3-non-interference-{}?mode=memory&cache=shared",
                NEXT_TEST_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        };
        let mut connection = open_writer_connection_for_target(&target).expect("writer connection");
        ensure_schema(&mut connection, &target).expect("schema");
        let mut cache = stmt_cache::WriterStatementCache;

        let (primary_sender, mut primary_receiver) = tokio::sync::mpsc::channel(1);
        let (diagnostic_sender, mut diagnostic_receiver) =
            tokio::sync::mpsc::channel(DIAGNOSTIC_QUEUE_BATCHES);
        for _ in 0..DIAGNOSTIC_QUEUE_BATCHES {
            diagnostic_sender
                .try_send(DiagnosticWriterMessage::Records(vec![DiagnosticEvent {
                    ts_unix_ms: 0,
                    level: "warn".to_owned(),
                    component: "writer-ac3-non-interference".to_owned(),
                    code: None,
                    correlation_id: None,
                    origin: "tracing".to_owned(),
                    message: "diagnostic".to_owned(),
                    detail: None,
                    id: 0,
                }]))
                .expect("fill the real bounded diagnostic channel");
        }
        assert!(matches!(
            diagnostic_sender.try_send(DiagnosticWriterMessage::Records(Vec::new())),
            Err(tokio::sync::mpsc::error::TrySendError::Full(_))
        ));

        let (primary, reply) = queued_upsert(message("atm:ac3-primary"));
        primary_sender
            .try_send(WriterMessage::Submit {
                op: primary.op,
                reply: primary.reply,
            })
            .expect("admit primary write while diagnostics are saturated");

        let Some(WriterWork::Primary(primary)) = runtime.block_on(receive_next_work(
            &mut primary_receiver,
            &mut diagnostic_receiver,
            false,
        )) else {
            panic!("a saturated diagnostic lane must not prevent primary selection");
        };
        process_batch(&target, &mut connection, &mut cache, vec![primary]);

        assert!(matches!(
            reply.recv().expect("primary mailbox reply"),
            Ok(WriteOpResult::UpsertMessage { inserted: true, .. })
        ));
        let persisted: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM mail_messages WHERE message_key = ?1",
                params!["atm:ac3-primary"],
                |row| row.get(0),
            )
            .expect("count persisted primary mailbox row");
        assert_eq!(persisted, 1);
    }

    #[test]
    #[ignore = "perf probe; wall-clock bound is not a correctness gate"]
    fn primary_write_latency_stays_bounded_under_a_full_prune_backlog() {
        const PRIMARY_WRITE_DEADLINE: Duration = Duration::from_millis(100);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("test runtime");
        let target = SharedDbTarget::InMemory {
            uri: format!(
                "file:writer-primary-over-prune-backlog-{}?mode=memory&cache=shared",
                NEXT_TEST_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        };
        let mut connection = open_writer_connection_for_target(&target).expect("writer connection");
        ensure_schema(&mut connection, &target).expect("schema");
        let mut cache = stmt_cache::WriterStatementCache;

        let (primary_sender, mut primary_receiver) = tokio::sync::mpsc::channel(1);
        let (diagnostic_sender, mut diagnostic_receiver) =
            tokio::sync::mpsc::channel(DIAGNOSTIC_QUEUE_BATCHES);
        for _ in 0..DIAGNOSTIC_QUEUE_BATCHES {
            let (reply, _receiver) = mpsc::sync_channel(1);
            diagnostic_sender
                .try_send(DiagnosticWriterMessage::Prune {
                    now_unix_ms: i64::MAX,
                    reply,
                })
                .expect("fill the lower-priority prune backlog");
        }
        assert!(matches!(
            diagnostic_sender.try_send(DiagnosticWriterMessage::Prune {
                now_unix_ms: i64::MAX,
                reply: mpsc::sync_channel(1).0,
            }),
            Err(tokio::sync::mpsc::error::TrySendError::Full(_))
        ));

        let (primary, reply) = queued_upsert(message("atm:primary-over-prune-backlog"));
        primary_sender
            .try_send(WriterMessage::Submit {
                op: primary.op,
                reply: primary.reply,
            })
            .expect("admit primary write while prune lane is saturated");

        let started = Instant::now();
        let Some(WriterWork::Primary(primary)) = runtime.block_on(receive_next_work(
            &mut primary_receiver,
            &mut diagnostic_receiver,
            false,
        )) else {
            panic!("a full prune backlog must not prevent primary selection");
        };
        process_batch(&target, &mut connection, &mut cache, vec![primary]);
        assert!(
            started.elapsed() <= PRIMARY_WRITE_DEADLINE,
            "primary write must complete within its hard bound while prune work is queued"
        );
        assert!(matches!(
            reply.recv().expect("primary mailbox reply"),
            Ok(WriteOpResult::UpsertMessage { inserted: true, .. })
        ));
    }

    #[test]
    fn ac7_real_diagnostic_channel_saturation_persists_a_bridged_row_after_drain() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("test runtime");
        let target = SharedDbTarget::InMemory {
            uri: format!(
                "file:writer-ac7-saturation-{}?mode=memory&cache=shared",
                NEXT_TEST_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        };
        let mut connection = open_writer_connection_for_target(&target).expect("writer connection");
        ensure_schema(&mut connection, &target).expect("schema");
        let mut cache = stmt_cache::WriterStatementCache;
        let diagnostic_stats = DiagnosticTimelinePersistenceStats::default();
        let mut rows_since_prune = 0;

        let (_primary_sender, mut primary_receiver) = tokio::sync::mpsc::channel(1);
        let (diagnostic_sender, mut diagnostic_receiver) =
            tokio::sync::mpsc::channel(DIAGNOSTIC_QUEUE_BATCHES);
        let bridged = DiagnosticEvent {
            ts_unix_ms: 42,
            level: "warn".to_owned(),
            component: "tracing.bridge.fixture".to_owned(),
            code: Some("ATM_BRIDGED_FIXTURE".to_owned()),
            correlation_id: None,
            origin: "tracing".to_owned(),
            message: "bridged diagnostic".to_owned(),
            detail: None,
            id: 0,
        };
        diagnostic_sender
            .try_send(DiagnosticWriterMessage::Records(vec![bridged.clone()]))
            .expect("enqueue bridged diagnostic on the real lower-priority channel");
        for _ in 1..DIAGNOSTIC_QUEUE_BATCHES {
            diagnostic_sender
                .try_send(DiagnosticWriterMessage::Records(vec![bridged.clone()]))
                .expect("fill the real bounded diagnostic channel");
        }
        assert!(matches!(
            diagnostic_sender.try_send(DiagnosticWriterMessage::Records(vec![bridged])),
            Err(tokio::sync::mpsc::error::TrySendError::Full(_))
        ));

        let Some(WriterWork::Diagnostics(DiagnosticWriterMessage::Records(batch))) = runtime
            .block_on(receive_next_work(
                &mut primary_receiver,
                &mut diagnostic_receiver,
                false,
            ))
        else {
            panic!("the real saturated diagnostic channel must yield its first batch when idle");
        };
        process_diagnostic_batch(
            &target,
            &mut connection,
            &mut cache,
            batch,
            &mut rows_since_prune,
            &diagnostic_stats,
        );

        assert_eq!(
            diagnostic_receiver.len(),
            DIAGNOSTIC_QUEUE_BATCHES - 1,
            "draining one real bounded-channel batch leaves the remaining backlog intact"
        );
        let persisted: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM diagnostic_events WHERE code = ?1 AND origin = ?2",
                params!["ATM_BRIDGED_FIXTURE", "tracing"],
                |row| row.get(0),
            )
            .expect("count persisted bridged row");
        assert_eq!(persisted, 1);
    }

    static NEXT_TEST_DB_ID: AtomicU64 = AtomicU64::new(1);

    fn message(key: &str) -> Message {
        let team: TeamName = "writer-test-team".parse().expect("team");
        let agent: AgentName = "writer-test-agent".parse().expect("agent");
        Message {
            team: team.clone(),
            agent: agent.clone(),
            message_key: MessageKey::new(key).expect("message key"),
            envelope: MessageEnvelope {
                from: agent,
                source_chat_id: None,
                text: format!("payload for {key}"),
                timestamp: IsoTimestamp::from_datetime(Utc::now()),
                read: false,
                source_team: Some(team),
                destination_chat_id: None,
                summary: None,
                message_id: None,
                requires_ack: false,
                pending_ack_at: None,
                acknowledged_at: None,
                acknowledges_message_id: None,
                parent_message_id: None,
                thread_mode: None,
                expires_at: None,
                task_id: None,
                extra: Map::new(),
            },
        }
    }

    fn queued_upsert(
        message: Message,
    ) -> (QueuedWrite, mpsc::Receiver<Result<WriteOpResult, AtmError>>) {
        let (reply, receiver) = mpsc::sync_channel(1);
        (
            QueuedWrite {
                op: Box::new(WriteOp::UpsertMessage(Box::new(message))),
                reply: ReplyTx::Sync(reply),
            },
            receiver,
        )
    }

    #[test]
    fn writer_runtime_builder_failure_returns_daemon_unavailable() {
        let target = Arc::new(SharedDbTarget::InMemory {
            uri: "file:writer-runtime-build-failure?mode=memory&cache=shared".to_owned(),
        });
        let serial_queue =
            Arc::new(SerialWriterQueue::open(target.as_ref()).expect("writer queue"));
        let error = SqliteWriter::start_with_runtime_builder(
            target,
            Arc::new(NullSqliteObservability),
            serial_queue,
            CHANNEL_CAPACITY,
            WRITE_OP_DEADLINE,
            WRITER_SHUTDOWN_JOIN_DEADLINE,
            || Err(std::io::Error::other("injected runtime build failure")),
        )
        .expect_err("injected writer runtime failure must be surfaced");

        assert_eq!(error.code(), AtmErrorCode::DaemonUnavailable);
        assert!(
            error.message().contains(
                "failed to initialize sqlite writer timer: injected runtime build failure"
            )
        );
    }

    fn queued_write() -> WriterMessage {
        let (reply, _receiver) = mpsc::sync_channel(1);
        WriterMessage::Submit {
            op: Box::new(WriteOp::UpsertMessages(Vec::new())),
            reply: ReplyTx::Sync(reply),
        }
    }

    fn first_queued_write(
        receiver: &mut tokio::sync::mpsc::Receiver<WriterMessage>,
    ) -> QueuedWrite {
        let WriterMessage::Submit { op, reply } = receiver.try_recv().expect("first write") else {
            panic!("test queue contains only submit messages");
        };
        QueuedWrite { op, reply }
    }

    #[tokio::test(start_paused = true)]
    async fn batch_collection_drains_all_currently_queued_writes() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(128);
        sender.try_send(queued_write()).expect("first queued write");
        for _ in 0..96 {
            sender.try_send(queued_write()).expect("queued write");
        }
        let first = first_queued_write(&mut receiver);
        let collection = tokio::spawn(async move {
            let mut batch = vec![first];
            let mut shutting_down = false;
            collect_batch(&mut receiver, &mut batch, &mut shutting_down).await;
            (batch, shutting_down)
        });

        tokio::task::yield_now().await;
        tokio::time::advance(BATCH_TIME_BUDGET).await;
        let (batch, shutting_down) = collection.await.expect("collection task");

        assert_eq!(batch.len(), 97);
        assert!(!shutting_down);
    }

    #[tokio::test(start_paused = true)]
    async fn batch_collection_includes_write_received_before_deadline() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
        sender.try_send(queued_write()).expect("first queued write");
        let first = first_queued_write(&mut receiver);
        let collection = tokio::spawn(async move {
            let mut batch = vec![first];
            let mut shutting_down = false;
            collect_batch(&mut receiver, &mut batch, &mut shutting_down).await;
            (batch, shutting_down)
        });

        tokio::task::yield_now().await;
        sender
            .try_send(queued_write())
            .expect("write within window");
        tokio::task::yield_now().await;
        tokio::time::advance(BATCH_TIME_BUDGET).await;
        let (batch, shutting_down) = collection.await.expect("collection task");

        assert_eq!(batch.len(), 2);
        assert!(!shutting_down);
    }

    #[tokio::test(start_paused = true)]
    async fn batch_collection_leaves_write_received_after_deadline_for_next_transaction() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
        sender.try_send(queued_write()).expect("first queued write");
        let first = first_queued_write(&mut receiver);
        let collection = tokio::spawn(async move {
            let mut batch = vec![first];
            let mut shutting_down = false;
            collect_batch(&mut receiver, &mut batch, &mut shutting_down).await;
            (batch, shutting_down, receiver)
        });

        tokio::task::yield_now().await;
        tokio::time::advance(BATCH_TIME_BUDGET).await;
        let (batch, shutting_down, mut receiver) = collection.await.expect("collection task");
        sender.try_send(queued_write()).expect("write after window");

        assert_eq!(batch.len(), 1);
        assert!(!shutting_down);
        assert!(matches!(
            receiver.try_recv(),
            Ok(WriterMessage::Submit { .. })
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn batch_collection_stops_waiting_when_shutdown_arrives() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
        sender.try_send(queued_write()).expect("first queued write");
        let first = first_queued_write(&mut receiver);
        let collection = tokio::spawn(async move {
            let mut batch = vec![first];
            let mut shutting_down = false;
            collect_batch(&mut receiver, &mut batch, &mut shutting_down).await;
            (batch, shutting_down)
        });

        tokio::task::yield_now().await;
        sender
            .try_send(queued_write())
            .expect("queued write before shutdown");
        sender
            .try_send(WriterMessage::Shutdown)
            .expect("shutdown signal");
        let (batch, shutting_down) = collection.await.expect("collection task");

        assert_eq!(batch.len(), 2);
        assert!(shutting_down);
    }

    #[tokio::test(start_paused = true)]
    async fn batch_collection_marks_disconnected_receiver_for_shutdown() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
        sender.try_send(queued_write()).expect("first queued write");
        let first = first_queued_write(&mut receiver);
        drop(sender);
        let mut batch = vec![first];
        let mut shutting_down = false;

        collect_batch(&mut receiver, &mut batch, &mut shutting_down).await;

        assert_eq!(batch.len(), 1);
        assert!(shutting_down);
    }

    #[tokio::test(start_paused = true)]
    async fn batch_collection_marks_shutdown_when_sender_disconnects_while_waiting() {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
        sender.try_send(queued_write()).expect("first queued write");
        let first = first_queued_write(&mut receiver);
        let collection = tokio::spawn(async move {
            let mut batch = vec![first];
            let mut shutting_down = false;
            collect_batch(&mut receiver, &mut batch, &mut shutting_down).await;
            (batch, shutting_down)
        });

        tokio::task::yield_now().await;
        drop(sender);
        let (batch, shutting_down) = collection.await.expect("collection task");

        assert_eq!(batch.len(), 1);
        assert!(shutting_down);
    }

    #[test]
    fn grouped_message_admissions_replay_individually_after_a_member_sqlite_error() {
        let target = SharedDbTarget::InMemory {
            uri: format!(
                "file:writer-group-replay-{}?mode=memory&cache=shared",
                NEXT_TEST_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        };
        let mut connection = open_writer_connection_for_target(&target).expect("writer connection");
        ensure_schema(&mut connection, &target).expect("schema");
        let mut cache = stmt_cache::WriterStatementCache;
        connection
            .execute_batch(
                "CREATE TRIGGER reject_group_member
                 BEFORE INSERT ON mail_messages
                 WHEN NEW.message_key = 'atm:group-rejected'
                 BEGIN SELECT RAISE(ABORT, 'intentional grouped-admission failure'); END;",
            )
            .expect("install deterministic writer failure trigger");

        let (first, first_reply) = queued_upsert(message("atm:group-first"));
        // The trigger forces a database error after the group savepoint has
        // admitted its first row, rather than relying on pre-enqueue input
        // validation.  The fallback must therefore undo that tentative row.
        let (invalid, invalid_reply) = queued_upsert(message("atm:group-rejected"));
        let (last, last_reply) = queued_upsert(message("atm:group-last"));

        process_batch(
            &target,
            &mut connection,
            &mut cache,
            vec![first, invalid, last],
        );

        assert!(matches!(
            first_reply.recv().expect("first reply"),
            Ok(WriteOpResult::UpsertMessage { inserted: true, .. })
        ));
        assert!(invalid_reply.recv().expect("invalid reply").is_err());
        assert!(matches!(
            last_reply.recv().expect("last reply"),
            Ok(WriteOpResult::UpsertMessage { inserted: true, .. })
        ));
        let persisted: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM mail_messages WHERE message_key IN (?1, ?2)",
                params!["atm:group-first", "atm:group-last"],
                |row| row.get(0),
            )
            .expect("count successful replay rows");
        assert_eq!(
            persisted, 2,
            "both valid members survive the fallback replay"
        );
    }
}
