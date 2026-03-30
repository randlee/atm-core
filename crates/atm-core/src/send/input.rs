use std::io::Read;

use crate::error::AtmError;

pub fn read_message_from_stdin() -> Result<String, AtmError> {
    let mut buffer = String::new();
    std::io::stdin()
        .read_to_string(&mut buffer)
        .map_err(|error| AtmError::validation(format!("failed to read stdin: {error}")))?;
    validate_message_text(buffer)
}

pub fn validate_message_text(message: impl Into<String>) -> Result<String, AtmError> {
    let message = message.into();
    if message.trim().is_empty() {
        return Err(AtmError::validation("message text cannot be empty"));
    }

    Ok(message)
}

pub fn validate_task_id(task_id: Option<String>) -> Result<Option<String>, AtmError> {
    match task_id {
        Some(task_id) if task_id.trim().is_empty() => {
            Err(AtmError::validation("task id must not be blank"))
        }
        Some(task_id) => Ok(Some(task_id)),
        None => Ok(None),
    }
}
