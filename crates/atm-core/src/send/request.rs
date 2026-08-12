use std::path::Path;

use crate::error::AtmError;
use crate::schema::{AtmMessageId, InboxMessage};
use crate::threading::{ThreadIndex, canonical_sender_identity, is_ephemeral};
use crate::types::TeamName;

use super::{SendMessageSource, file_policy, input};

pub(super) fn resolve_message_body(
    source: &SendMessageSource,
    current_dir: &Path,
    home_dir: &Path,
    team_name: &TeamName,
    max_message_bytes: usize,
) -> Result<String, AtmError> {
    match source {
        SendMessageSource::Inline(message) => {
            input::validate_message_text_with_limit(message.clone(), max_message_bytes)
        }
        SendMessageSource::File { path, message } => input::validate_message_text_with_limit(
            file_policy::process_file_reference(
                path,
                message.as_deref(),
                team_name,
                current_dir,
                home_dir,
            )?,
            max_message_bytes,
        ),
        SendMessageSource::Template(_) => Err(AtmError::validation(
            "templated sends must be resolved through the template-aware async admission path",
        )),
    }
}
pub(super) fn prepare_threaded_message(
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
        )),
        (Some(parent_id), Some(_), None) => {
            validate_thread_append(envelope, inbox_messages, parent_id)
        }
        (Some(_), None, _) | (None, Some(_), _) => Err(AtmError::validation(
            "thread updates must set both parent_message_id and thread_mode",
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
    })?;
    if is_ephemeral(parent) {
        return Err(AtmError::validation(
            "ephemeral messages may not be updated or superseded",
        ));
    }
    let root_id = index.root_id(parent_id).ok_or_else(|| {
        AtmError::validation(format!(
            "thread root could not be resolved for parent message {}",
            parent_id
        ))
    })?;
    let root = index.message(root_id).ok_or_else(|| {
        AtmError::validation(format!(
            "thread root message {} was not found in the recipient inbox",
            root_id
        ))
    })?;
    if canonical_sender_identity(root) != canonical_sender_identity(envelope) {
        return Err(AtmError::validation(
            "only the original sender may append details or supersede a message thread",
        ));
    }
    if index.has_successor(parent_id) {
        return Err(AtmError::validation(format!(
            "message {} already has a successor; ATM threads are strictly linear",
            parent_id
        )));
    }
    let requires_ack = index.thread_requires_ack(parent_id);
    envelope.requires_ack = requires_ack;
    envelope.pending_ack_at = requires_ack.then_some(envelope.timestamp);
    envelope.acknowledged_at = None;
    Ok(())
}
