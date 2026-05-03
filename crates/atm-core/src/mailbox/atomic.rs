use std::path::Path;

use serde_json::Value;

use crate::error::{AtmError, AtmErrorKind};
use crate::persistence;
use crate::schema::MessageEnvelope;
use crate::schema::inbox_message::to_shared_inbox_value;

/// Atomically replace one shared inbox file from fully serialized records.
///
/// ATM serializes every envelope into one JSON array document, fsyncs that temp
/// file, and then performs same-filesystem replacement through the shared
/// persistence helper. On Linux, a successful return means the file contents
/// and renamed directory entry were durably published after the
/// parent-directory fsync. On macOS, ATM performs the same parent-directory
/// sync call, but APFS durability semantics may still differ from Linux after
/// power loss. On Windows, the shared helper returns `Ok(())` after temp-file
/// fsync plus rename without an additional parent-directory sync because the
/// standard library does not expose a portable directory-sync operation there.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::MailboxWriteFailed`] when message
/// serialization fails or the mailbox temp-file write, fsync, rename, or
/// parent-directory durability step cannot be completed.
pub fn write_messages(path: &Path, messages: &[MessageEnvelope]) -> Result<(), AtmError> {
    let mut encoded = Vec::<Value>::with_capacity(messages.len());
    for message in messages {
        encoded.push(to_shared_inbox_value(message)?);
    }
    let mut bytes = serde_json::to_vec_pretty(&encoded)?;
    bytes.push(b'\n');

    persistence::atomic_write_bytes(
        path,
        &bytes,
        AtmErrorKind::MailboxWrite,
        "mailbox file",
        "Check that the mailbox directory is writable, has available disk space, and resides on a healthy filesystem before retrying the ATM command.",
    )
}
