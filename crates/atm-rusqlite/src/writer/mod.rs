mod ops;
mod stmt_cache;

pub(crate) use ops::{WriteOp, WriteOpResult};

use crate::shared_db::{
    SharedDbTarget, SqliteConnection, ensure_schema, open_connection_for_target, sqlite_error,
};
use atm_core::error::{AtmError, AtmErrorCode};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub(crate) const CHANNEL_CAPACITY: usize = 256;
pub(crate) const BATCH_SIZE_MAX: usize = 64;
pub(crate) const BATCH_TIME_BUDGET: Duration = Duration::from_millis(2);

type ReplyTx = SyncSender<Result<WriteOpResult, AtmError>>;

enum WriterMessage {
    Submit { op: Box<WriteOp>, reply: ReplyTx },
    Shutdown,
}

struct QueuedWrite {
    op: Box<WriteOp>,
    reply: ReplyTx,
}

#[derive(Debug)]
pub(crate) struct SqliteWriter {
    sender: SyncSender<WriterMessage>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl SqliteWriter {
    pub(crate) fn start(target: Arc<SharedDbTarget>) -> Result<Self, AtmError> {
        let mut connection = open_connection_for_target(target.as_ref())?;
        ensure_schema(&mut connection, target.as_ref())?;

        let (sender, receiver) = mpsc::sync_channel(CHANNEL_CAPACITY);
        let worker = thread::Builder::new()
            .name("atm-sqlite-writer".to_string())
            .spawn(move || writer_loop(target, connection, receiver))
            .map_err(|error| {
                AtmError::daemon_unavailable(format!(
                    "failed to start sqlite writer thread: {error}"
                ))
                .with_recovery(
                    "Inspect process thread limits or host resource exhaustion before retrying sqlite writer startup.",
                )
                .with_source(error)
            })?;
        Ok(Self {
            sender,
            worker: Mutex::new(Some(worker)),
        })
    }

    pub(crate) fn submit(&self, op: WriteOp) -> Result<WriteOpResult, AtmError> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::Submit {
                op: Box::new(op),
                reply: reply_tx,
            })
            .map_err(|_| {
                AtmError::daemon_unavailable("sqlite writer submission channel closed")
                    .with_recovery(
                        "Restart the ATM daemon or reopen the sqlite boundary assembly before retrying the write.",
                    )
            })?;
        reply_rx.recv().map_err(|_| {
            AtmError::daemon_unavailable("sqlite writer reply channel closed")
                .with_recovery(
                    "Restart the ATM daemon or reopen the sqlite boundary assembly before retrying the write.",
                )
        })?
    }
}

impl Drop for SqliteWriter {
    fn drop(&mut self) {
        let _ = self.sender.send(WriterMessage::Shutdown);
        if let Ok(mut worker) = self.worker.lock()
            && let Some(handle) = worker.take()
        {
            let _ = handle.join();
        }
    }
}

fn writer_loop(
    target: Arc<SharedDbTarget>,
    mut connection: SqliteConnection,
    receiver: Receiver<WriterMessage>,
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
}

fn receive_first_message(
    receiver: &Receiver<WriterMessage>,
    shutting_down: bool,
) -> Option<QueuedWrite> {
    if shutting_down {
        match receiver.try_recv() {
            Ok(WriterMessage::Submit { op, reply }) => Some(QueuedWrite { op, reply }),
            Ok(WriterMessage::Shutdown) => None,
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
        }
    } else {
        match receiver.recv() {
            Ok(WriterMessage::Submit { op, reply }) => Some(QueuedWrite { op, reply }),
            Ok(WriterMessage::Shutdown) | Err(_) => None,
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
    let mut transaction: rusqlite::Transaction<'_> = match connection.transaction() {
        Ok(transaction) => transaction,
        Err(error) => {
            let error = sqlite_error(
                target,
                "failed to open sqlite writer batch transaction",
                error,
            );
            for queued in batch {
                let _ = queued.reply.send(Err(copy_error(&error)));
            }
            return;
        }
    };

    let mut replies = Vec::with_capacity(batch.len());
    for queued in batch {
        let savepoint = match transaction.savepoint() {
            Ok(savepoint) => savepoint,
            Err(error) => {
                replies.push((
                    queued.reply,
                    Err(sqlite_error(
                        target,
                        "failed to open sqlite writer savepoint",
                        error,
                    )),
                ));
                continue;
            }
        };

        let result = catch_unwind(AssertUnwindSafe(|| {
            ops::execute(&queued.op, &savepoint, cache, target)
        }));
        match result {
            Ok(Ok(op_result)) => match savepoint.commit() {
                Ok(()) => replies.push((queued.reply, Ok(op_result))),
                Err(error) => replies.push((
                    queued.reply,
                    Err(sqlite_error(
                        target,
                        "failed to commit sqlite writer savepoint",
                        error,
                    )),
                )),
            },
            Ok(Err(error)) => {
                drop(savepoint);
                replies.push((queued.reply, Err(error)));
            }
            Err(_) => {
                drop(savepoint);
                replies.push((
                    queued.reply,
                    Err(
                        AtmError::daemon_unavailable("sqlite writer operation panicked")
                            .with_recovery(
                                "Inspect the sqlite writer hot-path operation and retry after correcting the panic source.",
                            ),
                    ),
                ));
            }
        }
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
                Ok(_) => Err(copy_error(error)),
                Err(existing) => Err(existing),
            }
        } else {
            result
        };
        let _ = reply.send(final_result);
    }
}

fn copy_error(error: &AtmError) -> AtmError {
    let mut copied = match error.code {
        AtmErrorCode::DaemonUnavailable => AtmError::daemon_unavailable(error.message.clone()),
        _ => AtmError::mailbox_write(error.message.clone()),
    };
    copied.code = error.code;
    copied.recovery = error.recovery.clone();
    copied
}

#[cfg(test)]
mod tests {
    use super::*;
    use atm_core::boundary;
    use atm_core::schema::MessageEnvelope;
    use atm_core::types::{AgentName, IsoTimestamp, TeamName};
    use rusqlite::OptionalExtension;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use tempfile::TempDir;

    fn team() -> TeamName {
        "test-team".parse().expect("team")
    }

    fn agent() -> AgentName {
        "test-agent".parse().expect("agent")
    }

    fn actor() -> AgentName {
        "test-actor".parse().expect("actor")
    }

    fn message_key(value: &str) -> boundary::MessageKey {
        boundary::MessageKey::new(value).expect("message key")
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
            pending_ack_at: Some(IsoTimestamp::now()),
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            stale_at: None,
            task_id: None,
            extra: serde_json::Map::new(),
        }
    }

    fn message_record(key: &str, text: &str) -> boundary::MailStoreMessageRecord {
        boundary::MailStoreMessageRecord {
            team: team(),
            agent: agent(),
            message_key: message_key(key),
            envelope: envelope(text),
            imported_from: None,
            recorded_at: Some(IsoTimestamp::now()),
        }
    }

    fn temp_db_target() -> (TempDir, Arc<SharedDbTarget>, PathBuf) {
        let tempdir = TempDir::new().expect("tempdir");
        let db_path = tempdir.path().join("mail.db");
        (
            tempdir,
            Arc::new(SharedDbTarget::Path(db_path.clone())),
            db_path,
        )
    }

    #[test]
    fn invalid_message_row_does_not_poison_other_rows_in_same_batch() {
        let (_tempdir, target, db_path) = temp_db_target();
        let mut connection = open_connection_for_target(target.as_ref()).expect("open connection");
        ensure_schema(&mut connection, target.as_ref()).expect("ensure schema");
        let mut cache = stmt_cache::WriterStatementCache;

        let (invalid_tx, invalid_rx) = mpsc::sync_channel(1);
        let (valid_tx, valid_rx) = mpsc::sync_channel(1);
        process_batch(
            target.as_ref(),
            &mut connection,
            &mut cache,
            vec![
                QueuedWrite {
                    op: Box::new(WriteOp::UpsertMessage(
                        boundary::MailStoreUpsertMessageRequest {
                            record: message_record("bad-key", "invalid"),
                        },
                    )),
                    reply: invalid_tx,
                },
                QueuedWrite {
                    op: Box::new(WriteOp::UpsertMessage(
                        boundary::MailStoreUpsertMessageRequest {
                            record: message_record("atm:valid", "valid"),
                        },
                    )),
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

        drop(connection);
        let verify = open_connection_for_target(target.as_ref()).expect("reopen");
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
        assert!(
            db_path.exists(),
            "writer batch should persist to the target database"
        );
    }
}
