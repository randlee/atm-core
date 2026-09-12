//! Plain-mail fallback for a governed task report rejected after admission.

use super::ops_envelope::StorageEnvelope;
use crate::shared_db::{SharedDbTarget, sqlite_error};
use atm_storage::{AtmError, Message};
use rusqlite::{Connection, params};

pub(super) fn drop_task_link_from_mail(
    record: &Message,
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let mut envelope = record.envelope.clone();
    envelope.task_id = None;
    envelope.task_op = None;
    envelope.task_complete = None;
    envelope.placement = None;
    let json = serde_json::to_string(&StorageEnvelope::new(&envelope))
        .map_err(|error| AtmError::mailbox_write(error.to_string()))?;
    connection
        .execute(
            "UPDATE mail_messages SET envelope_json=?4 WHERE team=?1 AND agent=?2 AND message_key=?3",
            params![record.team.as_str(), record.agent.as_str(), record.message_key.as_str(), json],
        )
        .map_err(|error| sqlite_error(target, "failed to detach rejected task report", error))?;
    Ok(())
}
