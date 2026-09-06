use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn writer_loop(
    target: Arc<SharedDbTarget>,
    serial_queue: Arc<SerialWriterQueue>,
    mut receiver: tokio::sync::mpsc::Receiver<WriterMessage>,
    mut diagnostic_receiver: tokio::sync::mpsc::Receiver<DiagnosticWriterMessage>,
    diagnostic_stats: Arc<DiagnosticTimelinePersistenceStats>,
    observability: Arc<dyn SqliteObservability>,
    runtime: tokio::runtime::Runtime,
    batch_time_budget: Duration,
) {
    let mut cache = stmt_cache::WriterStatementCache;
    let mut shutting_down = false;
    let mut diagnostic_rows_since_prune = 0_usize;
    loop {
        let Some(work) = runtime.block_on(receive_next_work(
            &mut receiver,
            &mut diagnostic_receiver,
            shutting_down,
        )) else {
            break;
        };
        match work {
            WriterWork::Primary(first) => {
                let mut batch = vec![first];
                runtime.block_on(collect_batch(
                    &mut receiver,
                    &mut batch,
                    &mut shutting_down,
                    batch_time_budget,
                ));
                serial_queue.with_connection(|connection| {
                    process_batch(&target, connection, &mut cache, batch);
                });
            }
            WriterWork::Diagnostics(message) => match message {
                DiagnosticWriterMessage::Records(batch) => {
                    serial_queue.with_connection(|connection| {
                        process_diagnostic_batch(
                            &target,
                            connection,
                            &mut cache,
                            batch,
                            &mut diagnostic_rows_since_prune,
                            diagnostic_stats.as_ref(),
                        );
                    })
                }
                DiagnosticWriterMessage::Prune { now_unix_ms, reply } => {
                    let result = serial_queue.with_connection(|connection| {
                        process_diagnostic_prune(&target, connection, &mut cache, now_unix_ms)
                    });
                    let _ = reply.send(result);
                }
            },
        }
    }
    serial_queue.with_connection(|connection| {
        checkpoint_writer_connection(target.as_ref(), connection, observability.as_ref());
    });
}

pub(crate) enum WriterWork {
    Primary(QueuedWrite),
    Diagnostics(DiagnosticWriterMessage),
}

pub(crate) async fn receive_next_work(
    receiver: &mut tokio::sync::mpsc::Receiver<WriterMessage>,
    diagnostic_receiver: &mut tokio::sync::mpsc::Receiver<DiagnosticWriterMessage>,
    shutting_down: bool,
) -> Option<WriterWork> {
    if shutting_down {
        match receiver.try_recv() {
            Ok(WriterMessage::Submit { op, reply }) => {
                Some(WriterWork::Primary(QueuedWrite { op, reply }))
            }
            Ok(WriterMessage::Shutdown) => {
                drain_submit_replies(receiver);
                None
            }
            Err(
                tokio::sync::mpsc::error::TryRecvError::Empty
                | tokio::sync::mpsc::error::TryRecvError::Disconnected,
            ) => None,
        }
    } else {
        match receiver.try_recv() {
            Ok(WriterMessage::Submit { op, reply }) => {
                Some(WriterWork::Primary(QueuedWrite { op, reply }))
            }
            Ok(WriterMessage::Shutdown) => {
                drain_submit_replies(receiver);
                None
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => None,
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                tokio::select! {
                    biased;
                    message = receiver.recv() => match message {
                        Some(WriterMessage::Submit { op, reply }) => Some(WriterWork::Primary(QueuedWrite { op, reply })),
                        Some(WriterMessage::Shutdown) => {
                            drain_submit_replies(receiver);
                            None
                        }
                        None => None,
                    },
                    batch = diagnostic_receiver.recv(), if receiver.is_empty() => batch.map(WriterWork::Diagnostics),
                }
            }
        }
    }
}

pub(crate) fn process_diagnostic_batch(
    target: &SharedDbTarget,
    connection: &mut SqliteConnection,
    cache: &mut stmt_cache::WriterStatementCache,
    batch: Vec<DiagnosticEvent>,
    diagnostic_rows_since_prune: &mut usize,
    diagnostic_stats: &DiagnosticTimelinePersistenceStats,
) {
    let batch_len = batch.len();
    let should_prune =
        diagnostic_rows_since_prune.saturating_add(batch_len) >= DIAGNOSTIC_PRUNE_CHECK_EVERY;
    let operation = WriteOp::RecordDiagnostics(batch);
    let result = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to open diagnostic timeline transaction",
                error,
            )
        })
        .and_then(|transaction| {
            let result = ops::execute(&operation, &transaction, cache, target);
            match result {
                Ok(_) => {
                    if should_prune {
                        let _ = ops::execute(
                            &WriteOp::PruneDiagnostics {
                                now_unix_ms: chrono::Utc::now().timestamp_millis(),
                            },
                            &transaction,
                            cache,
                            target,
                        )?;
                    }
                    transaction.commit().map_err(|error| {
                        sqlite_error(target, "failed to commit diagnostic timeline batch", error)
                    })
                }
                Err(error) => Err(error),
            }
        });
    if let Err(error) = result {
        diagnostic_stats
            .persist_error_total
            .fetch_add(batch_len as u64, Ordering::Relaxed);
        tracing::warn!(origin = "timeline", code = "ATM_DIAGNOSTIC_PERSIST_FAILED", error = %error, "diagnostic timeline batch dropped");
    } else if should_prune {
        diagnostic_stats
            .written_total
            .fetch_add(batch_len as u64, Ordering::Relaxed);
        *diagnostic_rows_since_prune = 0;
    } else {
        diagnostic_stats
            .written_total
            .fetch_add(batch_len as u64, Ordering::Relaxed);
        *diagnostic_rows_since_prune += batch_len;
    }
}

fn process_diagnostic_prune(
    target: &SharedDbTarget,
    connection: &mut SqliteConnection,
    cache: &mut stmt_cache::WriterStatementCache,
    now_unix_ms: i64,
) -> Result<u64, AtmError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| {
            sqlite_error(target, "failed to open diagnostic prune transaction", error)
        })?;
    let result = ops::execute(
        &WriteOp::PruneDiagnostics { now_unix_ms },
        &transaction,
        cache,
        target,
    )?;
    transaction
        .commit()
        .map_err(|error| sqlite_error(target, "failed to commit diagnostic prune", error))?;
    match result {
        WriteOpResult::DiagnosticsPruned(deleted) => Ok(deleted),
        other => Err(AtmError::daemon_unavailable(format!(
            "sqlite writer returned the wrong result for diagnostic prune: {other:?}"
        ))),
    }
}

pub(crate) async fn collect_batch(
    receiver: &mut tokio::sync::mpsc::Receiver<WriterMessage>,
    batch: &mut Vec<QueuedWrite>,
    shutting_down: &mut bool,
    budget: Duration,
) {
    let deadline = tokio::time::Instant::now() + budget;
    // Drain an already-queued bounded burst without delay. Between arrivals,
    // yield cooperatively until the fixed deadline instead of parking on an OS
    // timer: the batching contract stays platform-neutral and a one-millisecond
    // window is not expanded by host scheduler granularity.
    loop {
        match receiver.try_recv() {
            Ok(WriterMessage::Submit { op, reply }) => {
                batch.push(QueuedWrite { op, reply });
                continue;
            }
            Ok(WriterMessage::Shutdown) => {
                *shutting_down = true;
                continue;
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                *shutting_down = true;
                return;
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                if *shutting_down {
                    return;
                }
                if tokio::time::Instant::now() >= deadline {
                    return;
                }
                tokio::task::yield_now().await;
            }
        }
    }
}

pub(crate) fn process_batch(
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
    let mut queued_writes = batch.into_iter().peekable();
    while let Some(queued) = queued_writes.next() {
        if !is_batchable_message_admission(&queued) {
            replies.push(process_queued_write(
                target,
                &mut transaction,
                cache,
                queued,
            ));
            continue;
        }

        let mut admissions = vec![queued];
        while queued_writes
            .peek()
            .is_some_and(is_batchable_message_admission)
        {
            admissions.push(
                queued_writes
                    .next()
                    .expect("peeked queued message admission must remain available"),
            );
        }
        if admissions.len() == 1 {
            replies.push(process_queued_write(
                target,
                &mut transaction,
                cache,
                admissions.pop().expect("one admission is present"),
            ));
        } else {
            replies.extend(process_message_admission_group(
                target,
                &mut transaction,
                cache,
                admissions,
            ));
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
                Ok(_) => Err(copy_error(target, error)),
                Err(existing) => Err(existing),
            }
        } else {
            result
        };
        reply.send(final_result);
    }
}

/// Ordinary immutable message admissions are the hot path.  They are fully
/// contained in the outer writer transaction, so a contiguous admitted burst
/// can share one inner savepoint.  If any member reports an error or panics,
/// the shared savepoint is dropped and the whole group is replayed through the
/// established one-savepoint-per-operation path.  The fallback therefore
/// retains the existing per-operation rollback and reply semantics; only an
/// all-success group avoids redundant `SAVEPOINT` / `RELEASE` round trips.
fn is_batchable_message_admission(queued: &QueuedWrite) -> bool {
    matches!(&*queued.op, WriteOp::UpsertMessage { .. })
}

pub(crate) fn process_message_admission_group(
    target: &SharedDbTarget,
    transaction: &mut rusqlite::Transaction<'_>,
    cache: &mut stmt_cache::WriterStatementCache,
    admissions: Vec<QueuedWrite>,
) -> Vec<(ReplyTx, Result<WriteOpResult, AtmError>)> {
    let savepoint = match transaction.savepoint() {
        Ok(savepoint) => savepoint,
        Err(error) => {
            let error = sqlite_error(
                target,
                "failed to open sqlite writer message-admission savepoint",
                error,
            );
            return admissions
                .into_iter()
                .map(|queued| (queued.reply, Err(copy_error(target, &error))))
                .collect();
        }
    };

    let mut results = Vec::with_capacity(admissions.len());
    for queued in &admissions {
        let result = catch_unwind(AssertUnwindSafe(|| {
            ops::execute(&queued.op, &savepoint, cache, target)
        }));
        match result {
            Ok(Ok(result)) => results.push(result),
            Ok(Err(_)) | Err(_) => {
                // Roll back every tentative row before replaying.  A replay is
                // intentionally conservative: it preserves the established
                // operation-by-operation error isolation for rare failures.
                drop(savepoint);
                return admissions
                    .into_iter()
                    .map(|queued| process_queued_write(target, transaction, cache, queued))
                    .collect();
            }
        }
    }

    match savepoint.commit() {
        Ok(()) => admissions
            .into_iter()
            .zip(results)
            .map(|(queued, result)| (queued.reply, Ok(result)))
            .collect(),
        Err(error) => {
            let error = sqlite_error(
                target,
                "failed to commit sqlite writer message-admission savepoint",
                error,
            );
            admissions
                .into_iter()
                .map(|queued| (queued.reply, Err(copy_error(target, &error))))
                .collect()
        }
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

pub(crate) fn process_queued_write(
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
    let result = match result {
        Ok(Err(error)) => {
            drop(savepoint);
            match task_ops::append_rejected_task_event(&queued.op, transaction, target, &error) {
                Ok(()) => Err(error),
                Err(audit_error) => Err(audit_error),
            }
        }
        result => finalize_queued_write(target, savepoint, result),
    };
    (reply, result)
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
