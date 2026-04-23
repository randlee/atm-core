//! Mailbox owner-layer write boundaries for the Claude-owned inbox surface.

use std::path::Path;

use crate::error::AtmError;
use crate::mailbox::atomic;
use crate::mailbox::source::SourceFile;
use crate::schema::MessageEnvelope;

/// Commit one mailbox file through the mailbox-layer write boundary.
///
/// The mailbox layer owns writes to the Claude-owned inbox compatibility
/// surface. Callers should express mailbox intent here instead of reaching
/// down to low-level atomic replacement directly.
pub(crate) fn commit_mailbox_state(
    path: &Path,
    messages: &[MessageEnvelope],
) -> Result<(), AtmError> {
    atomic::write_messages(path, messages)
}

/// Commit one already-loaded multi-source mailbox set through the mailbox layer.
pub(crate) fn commit_source_files(source_files: &[SourceFile]) -> Result<(), AtmError> {
    for source in source_files {
        commit_mailbox_state(&source.path, &source.messages)?;
    }
    Ok(())
}
