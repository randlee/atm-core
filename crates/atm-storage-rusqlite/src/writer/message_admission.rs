//! Canonical message admission and governed task metadata propagation.

use atm_storage::{AtmError, Message, MessageWriteOrigin};

use super::ops::{
    MAX_ENVELOPE_JSON_BYTES, WriteOpResult, insert_message_canonical, load_existing_message,
};
use super::ops_envelope::StorageEnvelope;
use super::stmt_cache::WriterStatementCache;
use super::task_ops::{TaskMessageResult, apply_task_message};
use crate::shared_db::{SharedDbTarget, SqliteConnection, serialize_json};
use rusqlite::OptionalExtension;

pub(crate) fn validate_upsert_message_request(record: &Message) -> Result<(), AtmError> {
    let envelope_json = serialize_json(
        &StorageEnvelope::new(&record.envelope),
        "mail-store envelope",
    )?;
    if envelope_json.len() > MAX_ENVELOPE_JSON_BYTES {
        return Err(AtmError::validation(format!(
            "mail-store envelope JSON exceeded the writer lane limit of {MAX_ENVELOPE_JSON_BYTES} bytes"
        )));
    }
    Ok(())
}

pub(super) fn execute_upsert_message(
    record: &Message,
    provenance: MessageWriteOrigin,
    connection: &SqliteConnection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<WriteOpResult, AtmError> {
    validate_cron_generation_fence(provenance, connection)?;
    let inserted = insert_message_canonical(record, connection, cache, target)?;
    let existing = if inserted {
        None
    } else {
        Some(Box::new(load_existing_message(record, connection, target)?))
    };
    let mut already_closed = None;
    let mut task_assignee = None;
    let mut queued_position = None;
    let mut reassign_notice = None;
    let mut task_rejection = None;
    let mut task_events = Vec::new();
    if inserted && provenance.is_local() {
        match apply_task_message(record, connection, cache, target)? {
            TaskMessageResult::Applied {
                already_closed: closed,
                task_assignee: assignee,
                queued_position: position,
                reassign_notice: notice,
                task_events: events,
            } => {
                already_closed = closed;
                task_assignee = assignee;
                queued_position = position;
                reassign_notice = notice;
                task_events = events;
            }
            TaskMessageResult::RejectedReportDelivered { error, event } => {
                task_rejection = Some(error);
                task_events = vec![event];
            }
        }
    }
    Ok(WriteOpResult::UpsertMessage {
        inserted,
        existing,
        already_closed,
        task_assignee,
        queued_position,
        reassign_notice,
        task_rejection,
        task_events,
    })
}

/// Validates the cron run fence before looking up a duplicate or writing any
/// mailbox/task state. The caller runs this inside the same immediate writer
/// transaction as canonical admission and task effects.
pub(super) fn validate_cron_generation_fence(
    provenance: MessageWriteOrigin,
    connection: &SqliteConnection,
) -> Result<(), AtmError> {
    let MessageWriteOrigin::CronRun {
        schedule_id,
        generation,
    } = provenance
    else {
        return Ok(());
    };
    let schedule_id = i64::try_from(schedule_id.get())
        .map_err(|_| AtmError::validation("cron schedule id exceeds the sqlite integer range"))?;
    let expected_generation = i64::from(generation.get());
    let actual: Option<(i64, i64)> = connection
        .query_row(
            "SELECT generation, active FROM cron_schedule WHERE id = ?1",
            [schedule_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| {
            AtmError::new(
                atm_storage::AtmErrorCode::StoreQueryFailed,
                format!("failed to verify cron message generation fence: {error}"),
            )
        })?;
    if actual == Some((expected_generation, 1)) {
        Ok(())
    } else {
        Err(AtmError::validation_with_recovery(
            "cron action permit is stale or its schedule is no longer active",
            "allow the current cron run to stop and let the scheduler start a fresh generation",
        ))
    }
}
