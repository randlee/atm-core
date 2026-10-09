//! Canonical message admission and governed task metadata propagation.

use atm_storage::{AtmError, Message, MessageWriteOrigin};

use super::ops::{
    MAX_ENVELOPE_JSON_BYTES, WriteOpResult, insert_message_canonical, load_existing_message,
};
use super::ops_envelope::StorageEnvelope;
use super::stmt_cache::WriterStatementCache;
use super::task_ops::{TaskMessageResult, apply_task_message};
use crate::shared_db::{SharedDbTarget, SqliteConnection, serialize_json};

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
    if inserted && provenance == MessageWriteOrigin::Local {
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
