use std::fs;
use std::io::Write;
use std::path::Path;

use crate::error::{AtmError, AtmErrorKind};
use crate::mailbox::temp_file_suffix;
use crate::schema::MessageEnvelope;

pub fn write_messages(path: &Path, messages: &[MessageEnvelope]) -> Result<(), AtmError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AtmError::new(
                AtmErrorKind::MailboxWrite,
                format!("failed to create mailbox directory: {error}"),
            )
            .with_recovery(
                "Check mailbox directory permissions and available disk space, then retry the ATM command.",
            )
            .with_source(error)
        })?;
    }

    let temp_path = path.with_file_name(format!(
        "{}.{}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("mailbox"),
        temp_file_suffix()
    ));

    {
        let mut file = fs::File::create(&temp_path).map_err(|error| {
            AtmError::new(
                AtmErrorKind::MailboxWrite,
                format!("failed to create mailbox temp file: {error}"),
            )
            .with_recovery(
                "Check mailbox directory permissions and available disk space, then retry the ATM command.",
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
                .with_recovery(
                    "Check available disk space and mailbox file permissions, then retry the ATM command.",
                )
                .with_source(error)
            })?;
        }
        file.sync_all().map_err(|error| {
            AtmError::new(
                AtmErrorKind::MailboxWrite,
                format!("failed to fsync mailbox temp file: {error}"),
            )
            .with_recovery(
                "Check disk health and filesystem permissions, then retry the ATM command after the mailbox temp file can be synced successfully.",
            )
            .with_source(error)
        })?;
    }

    fs::rename(&temp_path, path).map_err(|error| {
        AtmError::new(
            AtmErrorKind::MailboxWrite,
            format!("failed to replace mailbox file: {error}"),
        )
        .with_recovery(
            "Check that the mailbox directory is writable and on a healthy filesystem, then retry the ATM command.",
        )
        .with_source(error)
    })?;
    Ok(())
}
