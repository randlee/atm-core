use std::fs;
use std::io::Write;
use std::path::Path;

use crate::error::{AtmError, AtmErrorKind};
use crate::schema::MessageEnvelope;
use uuid::Uuid;

pub fn write_messages(path: &Path, messages: &[MessageEnvelope]) -> Result<(), AtmError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AtmError::new(
                AtmErrorKind::MailboxWrite,
                format!("failed to create mailbox directory: {error}"),
            )
            .with_source(error)
        })?;
    }

    let temp_path = path.with_file_name(format!(
        "{}.{}.{}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("mailbox"),
        std::process::id(),
        Uuid::new_v4()
    ));

    {
        let mut file = fs::File::create(&temp_path).map_err(|error| {
            AtmError::new(
                AtmErrorKind::MailboxWrite,
                format!("failed to create mailbox temp file: {error}"),
            )
            .with_source(error)
        })?;
        for message in messages {
            serde_json::to_writer(&mut file, message)?;
            file.write_all(b"\n").map_err(|error| {
                AtmError::new(
                    AtmErrorKind::MailboxWrite,
                    format!("failed to write mailbox record: {error}"),
                )
                .with_source(error)
            })?;
        }
        file.sync_all().map_err(|error| {
            AtmError::new(
                AtmErrorKind::MailboxWrite,
                format!("failed to fsync mailbox temp file: {error}"),
            )
            .with_source(error)
        })?;
    }

    fs::rename(&temp_path, path).map_err(|error| {
        AtmError::new(
            AtmErrorKind::MailboxWrite,
            format!("failed to replace mailbox file: {error}"),
        )
        .with_source(error)
    })?;
    Ok(())
}
