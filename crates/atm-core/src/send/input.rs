use std::io::Read;

use crate::error::{AtmError, AtmErrorKind};
const MAX_STDIN_MESSAGE_BYTES: usize = 256 * 1024;

/// Read a message body from stdin.
///
/// This is a synchronous CLI boundary. ATM caps the total stdin payload so the
/// command cannot buffer an unbounded message into memory.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::MessageValidationFailed`] when stdin
/// cannot be read.
pub fn read_message_from_stdin() -> Result<String, AtmError> {
    read_message_from_reader(std::io::stdin())
}

/// Validate that a message body is non-empty after trimming.
///
/// ATM uses one size limit for inline and stdin-backed message bodies so the
/// synchronous send path has a bounded memory contract regardless of input mode.
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
    if message.len() > MAX_STDIN_MESSAGE_BYTES {
        return Err(AtmError::validation(format!(
            "message text exceeds the {}-byte limit",
            MAX_STDIN_MESSAGE_BYTES
        ))
        .with_recovery(
            "Use a shorter inline/stdin message or send large content with --file so ATM can preserve the message boundary safely.",
        ));
    }

    Ok(message)
}

fn read_message_from_reader(reader: impl Read) -> Result<String, AtmError> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_STDIN_MESSAGE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            AtmError::new(
                AtmErrorKind::MailboxRead,
                format!("failed to read stdin: {error}"),
            )
            .with_source(error)
        })?;

    if bytes.len() > MAX_STDIN_MESSAGE_BYTES {
        return Err(AtmError::validation(format!(
            "stdin message exceeds the {}-byte limit",
            MAX_STDIN_MESSAGE_BYTES
        ))
        .with_recovery(
            "Use a shorter inline/stdin message or send large content with --file so ATM can preserve the message boundary safely.",
        ));
    }

    let buffer = String::from_utf8(bytes).map_err(|error| {
        AtmError::new(
            AtmErrorKind::MailboxRead,
            format!("failed to read stdin as UTF-8 text: {error}"),
        )
        .with_source(error)
    })?;
    validate_message_text(buffer)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{MAX_STDIN_MESSAGE_BYTES, read_message_from_reader};

    #[test]
    fn read_message_from_reader_accepts_small_utf8_input() {
        let message =
            read_message_from_reader(Cursor::new("hello from stdin")).expect("stdin message");

        assert_eq!(message, "hello from stdin");
    }

    #[test]
    fn read_message_from_reader_rejects_oversized_input() {
        let oversized = "a".repeat(MAX_STDIN_MESSAGE_BYTES + 1);

        let error = read_message_from_reader(Cursor::new(oversized)).expect_err("oversized stdin");

        assert!(error.is_validation());
        assert!(error.message.contains("stdin message exceeds"));
        assert!(
            error
                .recovery
                .as_deref()
                .is_some_and(|value| value.contains("--file"))
        );
    }

    #[test]
    fn validate_message_text_rejects_oversized_inline_input() {
        let oversized = "a".repeat(MAX_STDIN_MESSAGE_BYTES + 1);

        let error = super::validate_message_text(oversized).expect_err("oversized inline message");

        assert!(error.message.contains("message text exceeds"));
    }
}
