use crate::error::AtmError;
use crate::schema::{AtmMessageId, InboxMessage};
use crate::threading::{ThreadIndex, canonical_sender_identity, is_ephemeral};

pub(crate) fn prepare_threaded_message(
    envelope: &mut InboxMessage,
    inbox_messages: &[InboxMessage],
) -> Result<(), AtmError> {
    match (
        envelope.parent_message_id,
        envelope.thread_mode,
        envelope.expires_at,
    ) {
        (None, None, _) => Ok(()),
        (Some(_), Some(_), Some(_)) => Err(AtmError::validation(
            "ephemeral messages may not participate in a message thread",
        )
        .with_recovery(
            "Send the message either as a standalone ephemeral note or as a non-ephemeral thread update.",
        )),
        (Some(parent_id), Some(_), None) => {
            validate_thread_append(envelope, inbox_messages, parent_id)
        }
        (Some(_), None, _) | (None, Some(_), _) => Err(AtmError::validation(
            "thread updates must set both parent_message_id and thread_mode",
        )
        .with_recovery(
            "Provide both the parent message id and either add-details or supersede when appending to an existing thread.",
        )),
    }
}

fn validate_thread_append(
    envelope: &mut InboxMessage,
    inbox_messages: &[InboxMessage],
    parent_id: AtmMessageId,
) -> Result<(), AtmError> {
    let index = ThreadIndex::new(inbox_messages);
    let parent = index.message(parent_id).ok_or_else(|| {
        AtmError::validation(format!(
            "thread parent message {} was not found in the recipient inbox",
            parent_id
        ))
        .with_recovery(
            "Refresh the recipient inbox state and retry the update against a message id that still exists in that thread.",
        )
    })?;

    if is_ephemeral(parent) {
        return Err(AtmError::validation(
            "ephemeral messages may not be updated or superseded",
        )
        .with_recovery(
            "Send a fresh standalone message instead of trying to append to an ephemeral message.",
        ));
    }

    let Some(root_id) = index.root_id(parent_id) else {
        return Err(AtmError::validation(format!(
            "thread root could not be resolved for parent message {}",
            parent_id
        ))
        .with_recovery(
            "Repair the malformed message thread or resend the correction as a fresh standalone message.",
        ));
    };
    let root = index.message(root_id).ok_or_else(|| {
        AtmError::validation(format!(
            "thread root message {} was not found in the recipient inbox",
            root_id
        ))
        .with_recovery(
            "Repair the malformed message thread or resend the correction as a fresh standalone message.",
        )
    })?;

    if canonical_sender_identity(root) != canonical_sender_identity(envelope) {
        return Err(AtmError::validation(
            "only the original sender may append details or supersede a message thread",
        )
        .with_recovery(
            "Send a new message instead of appending to a thread you did not originate.",
        ));
    }

    if index.has_successor(parent_id) {
        return Err(AtmError::validation(format!(
            "message {} already has a successor; ATM threads are strictly linear",
            parent_id
        ))
        .with_recovery(
            "Append to the current terminal message in the thread instead of branching from an older message.",
        ));
    }

    let thread_requires_ack = index.thread_requires_ack(parent_id);
    envelope.requires_ack = thread_requires_ack;
    envelope.pending_ack_at = thread_requires_ack.then_some(envelope.timestamp);
    envelope.acknowledged_at = None;
    Ok(())
}
