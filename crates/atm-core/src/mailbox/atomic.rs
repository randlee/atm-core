use std::path::Path;

use crate::error::{AtmError, AtmErrorKind};
use crate::persistence;
use crate::schema::MessageEnvelope;

pub fn write_messages(path: &Path, messages: &[MessageEnvelope]) -> Result<(), AtmError> {
    let mut bytes = Vec::new();
    for message in messages {
        serde_json::to_writer(&mut bytes, message)?;
        bytes.push(b'\n');
    }

    persistence::atomic_write_bytes(
        path,
        &bytes,
        AtmErrorKind::MailboxWrite,
        "mailbox file",
        "Check that the mailbox directory is writable, has available disk space, and resides on a healthy filesystem before retrying the ATM command.",
    )
}
