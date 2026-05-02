use std::io::Read;

use crate::error::{AtmError, AtmErrorKind};

/// Read a message body from stdin.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::MessageValidationFailed`] when stdin
/// cannot be read.
pub fn read_message_from_stdin() -> Result<String, AtmError> {
    let mut buffer = String::new();
    std::io::stdin()
        .read_to_string(&mut buffer)
        .map_err(|error| {
            AtmError::new(
                AtmErrorKind::MailboxRead,
                format!("failed to read stdin: {error}"),
            )
            .with_source(error)
        })?;
    validate_message_text(buffer)
}

/// Validate that a message body is non-empty after trimming.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::MessageValidationFailed`] when the
/// message body is empty or whitespace-only.
pub fn validate_message_text(message: impl Into<String>) -> Result<String, AtmError> {
    let message = message.into();
    if message.trim().is_empty() {
        return Err(AtmError::validation("message text cannot be empty"));
    }

    Ok(message)
}

/// Validate an optional task id for send/ack workflows.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::MessageValidationFailed`] when a task id
/// is present but blank.
pub fn validate_task_id(task_id: Option<String>) -> Result<Option<String>, AtmError> {
    match task_id {
        Some(task_id) if task_id.trim().is_empty() => {
            Err(AtmError::validation("task id must not be blank"))
        }
        Some(task_id) => Ok(Some(task_id)),
        None => Ok(None),
    }
}
