//! Atomic informational message emitted to a task's former assignee.

use atm_storage::contract::{Message, MessageKey};
use atm_storage::{AgentName, AtmError, AtmMessageId, TaskId};

use super::ops::insert_message_canonical;
use super::stmt_cache::WriterStatementCache;
use crate::shared_db::{SharedDbTarget, SqliteConnection};

pub(super) fn insert_reassign_notice(
    record: &Message,
    task_id: &TaskId,
    old_assignee: AgentName,
    connection: &SqliteConnection,
    cache: &mut WriterStatementCache,
    target: &SharedDbTarget,
) -> Result<Message, AtmError> {
    let (message_id, timestamp) = AtmMessageId::new_with_timestamp();
    let mut envelope = record.envelope.clone();
    envelope.text = format!("task {task_id} was reassigned to {}", record.agent);
    envelope.timestamp = timestamp;
    envelope.read = false;
    envelope.destination_chat_id = None;
    envelope.summary = Some(format!("task_closed:{task_id}"));
    envelope.message_id = Some(message_id);
    envelope.requires_ack = false;
    envelope.pending_ack_at = None;
    envelope.acknowledged_at = None;
    envelope.acknowledges_message_id = None;
    envelope.parent_message_id = None;
    envelope.thread_mode = None;
    envelope.expires_at = None;
    envelope.task_id = Some(task_id.clone());
    envelope.placement = None;
    envelope.task_op = None;
    envelope.task_complete = None;
    envelope.extra.clear();
    let notice = Message {
        team: record.team.clone(),
        agent: old_assignee,
        message_key: MessageKey::from(message_id),
        envelope,
    };
    if !insert_message_canonical(&notice, connection, cache, target)? {
        return Err(AtmError::mailbox_write(
            "fresh task reassignment notice collided with an existing message key",
        ));
    }
    Ok(notice)
}
