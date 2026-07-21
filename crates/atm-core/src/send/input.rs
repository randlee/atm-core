use std::io::Read;

use crate::error::{AtmError, AtmErrorCode};
const MAX_STDIN_MESSAGE_BYTES: usize = 256 * 1024;

/// Read a message body from stdin.
///
/// This is a synchronous CLI boundary. ATM caps the total stdin payload so the
/// command cannot buffer an unbounded message into memory.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::MailboxReadFailed`] when stdin cannot
/// be read or decoded as UTF-8 text, and
/// [`crate::error_codes::AtmErrorCode::MessageValidationFailed`] when stdin is
/// empty, whitespace-only, or exceeds the inline/stdin byte limit.
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
/// message body is empty, whitespace-only, or exceeds the inline/stdin byte
/// limit.
pub fn validate_message_text(message: impl Into<String>) -> Result<String, AtmError> {
    let message = message.into();
    if message.trim().is_empty() {
        return Err(AtmError::validation("message text cannot be empty"));
    }
    if message.len() > MAX_STDIN_MESSAGE_BYTES {
        return Err(AtmError::validation(format!(
            "message text exceeds the {}-byte limit",
            MAX_STDIN_MESSAGE_BYTES
        )));
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
                AtmErrorCode::MailboxReadFailed,
                format!("failed to read stdin: {error}"),
            )
        })?;

    if bytes.len() > MAX_STDIN_MESSAGE_BYTES {
        return Err(AtmError::validation(format!(
            "stdin message exceeds the {}-byte limit",
            MAX_STDIN_MESSAGE_BYTES
        )));
    }

    let buffer = String::from_utf8(bytes).map_err(|error| {
        AtmError::new(
            AtmErrorCode::MailboxReadFailed,
            format!("failed to read stdin as UTF-8 text: {error}"),
        )
    })?;
    validate_message_text(buffer)
}

#[cfg(test)]
mod tests {
    use std::io::{self, Cursor, Read};

    use super::{MAX_STDIN_MESSAGE_BYTES, read_message_from_reader};
    use crate::error_codes::AtmErrorCode;

    struct Unreadable;

    impl Read for Unreadable {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("stdin device failed"))
        }
    }

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

        assert!(error.code() == crate::error_codes::AtmErrorCode::MessageValidationFailed);
        assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
        assert!(error.message().contains("stdin message exceeds"));
        assert!(error.message().contains("Recovery:"));
    }

    #[test]
    fn validate_message_text_rejects_oversized_inline_input() {
        let oversized = "a".repeat(MAX_STDIN_MESSAGE_BYTES + 1);

        let error = super::validate_message_text(oversized).expect_err("oversized inline message");

        assert!(error.message().contains("message text exceeds"));
    }

    #[test]
    fn read_message_from_reader_rejects_empty_and_whitespace_input() {
        for input in ["", " \n\t "] {
            let error = read_message_from_reader(Cursor::new(input)).expect_err("empty stdin");
            assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
            assert!(error.message().contains("cannot be empty"));
        }
    }

    #[test]
    fn read_message_from_reader_rejects_non_utf8_input() {
        let error = read_message_from_reader(Cursor::new(vec![0xff])).expect_err("non UTF-8 stdin");

        assert_eq!(error.code(), AtmErrorCode::MailboxReadFailed);
        assert!(error.message().contains("UTF-8"));
    }

    #[test]
    fn read_message_from_reader_reports_unreadable_input() {
        let error = read_message_from_reader(Unreadable).expect_err("unreadable stdin");

        assert_eq!(error.code(), AtmErrorCode::MailboxReadFailed);
        assert!(error.message().contains("failed to read stdin"));
    }
}
