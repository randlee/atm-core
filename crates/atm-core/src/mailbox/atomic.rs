use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use crate::error::{AtmError, AtmErrorCode};

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
