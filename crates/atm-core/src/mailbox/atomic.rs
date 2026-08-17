use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

#[cfg(test)]
use serde_json::Value;

use crate::error::{AtmError, AtmErrorCode};
#[cfg(test)]
use crate::persistence;
use crate::schema::InboxMessage;
use crate::schema::inbox_message::{SharedAppendPolicy, to_shared_inbox_value_with_policy};

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
///
/// Repair/rebuild only — not reachable from normal runtime send or ack paths.
#[cfg(test)]
pub fn write_messages(
    path: &Path,
    messages: &[InboxMessage],
    export_policy: SharedAppendPolicy,
    render: &dyn Fn(&InboxMessage) -> Result<InboxMessage, AtmError>,
) -> Result<(), AtmError> {
    write_message_iter_with_renderer(path, messages.iter(), export_policy, render)
}

#[cfg(test)]
pub fn write_message_iter<'a, I>(
    path: &Path,
    messages: I,
    export_policy: SharedAppendPolicy,
) -> Result<(), AtmError>
where
    I: IntoIterator<Item = &'a InboxMessage>,
{
    write_message_iter_with_renderer(path, messages, export_policy, &identity)
}

#[cfg(test)]
fn write_message_iter_with_renderer<'a, I>(
    path: &Path,
    messages: I,
    export_policy: SharedAppendPolicy,
    render: &dyn Fn(&InboxMessage) -> Result<InboxMessage, AtmError>,
) -> Result<(), AtmError>
where
    I: IntoIterator<Item = &'a InboxMessage>,
{
    let iterator = messages.into_iter();
    let mut encoded = Vec::<Value>::new();
    for message in iterator {
        let rendered = render(message)?;
        encoded.push(to_shared_inbox_value_with_policy(&rendered, export_policy)?);
    }
    let mut bytes = serde_json::to_vec(&encoded)?;
    bytes.push(b'\n');

    persistence::atomic_write_bytes(
        path,
        &bytes,
        AtmErrorCode::MailboxWriteFailed,
        "mailbox file",
        "Check that the mailbox directory is writable, has available disk space, and resides on a healthy filesystem before retrying the ATM command.",
    )
}

#[cfg(test)]
fn identity(message: &InboxMessage) -> Result<InboxMessage, AtmError> {
    Ok(message.clone())
}

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "JSONL append helper remains test-only after compat retirement"
    )
)]
pub fn append_message(
    path: &Path,
    message: &InboxMessage,
    export_policy: SharedAppendPolicy,
) -> Result<(), AtmError> {
    let encoded = to_shared_inbox_value_with_policy(message, export_policy)?;
    append_jsonl_record(path, &encoded)
}

pub fn append_jsonl_record<T: serde::Serialize>(path: &Path, record: &T) -> Result<(), AtmError> {
    let mut bytes = serde_json::to_vec(record)?;
    bytes.push(b'\n');

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| {
            AtmError::new(
                AtmErrorCode::MailboxWriteFailed,
                format!(
                    "failed to open mailbox file {} for append: {error}",
                    path.display()
                ),
            )
        })?;
    file.write_all(&bytes).map_err(|error| {
        AtmError::new(
            AtmErrorCode::MailboxWriteFailed,
            format!(
                "failed to append mailbox record {}: {error}",
                path.display()
            ),
        )
    })?;
    file.sync_data().map_err(|error| {
        AtmError::new(
            AtmErrorCode::MailboxWriteFailed,
            format!(
                "failed to sync appended mailbox record {}: {error}",
                path.display()
            ),
        )
    })
}
