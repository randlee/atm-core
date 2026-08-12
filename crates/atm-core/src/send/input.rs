use std::io::Read;

use crate::config::types::DEFAULT_MAX_MESSAGE_BYTES;
use crate::error::{AtmError, AtmErrorCode};

/// Default bounded message payload policy when no workspace config is present.
pub const DEFAULT_MESSAGE_MAX_BYTES: usize = DEFAULT_MAX_MESSAGE_BYTES as usize;

/// Validate a caller-selected payload policy before it crosses a process or
/// host boundary.
///
/// The daemon treats [`crate::send::WriteRequest::max_message_bytes`] as
/// untrusted wire data: a client may lower the configured limit, but cannot
/// use the field to widen the server's fixed admission budget.
pub fn validate_message_size_limit(max_message_bytes: usize) -> Result<(), AtmError> {
    if max_message_bytes == 0 || max_message_bytes > DEFAULT_MESSAGE_MAX_BYTES {
        return Err(AtmError::validation(format!(
            "message byte limit must be between 1 and {DEFAULT_MESSAGE_MAX_BYTES} bytes"
        )));
    }

    Ok(())
}

/// Serde default for additive write requests from older clients.
#[must_use]
pub const fn default_message_max_bytes() -> usize {
    DEFAULT_MESSAGE_MAX_BYTES
}

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
    read_message_from_stdin_with_limit(DEFAULT_MESSAGE_MAX_BYTES)
}

/// Materialize stdin under the caller-selected bounded message policy.
pub fn read_message_from_stdin_with_limit(max_message_bytes: usize) -> Result<String, AtmError> {
    read_message_from_reader_with_limit(std::io::stdin(), max_message_bytes)
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
    validate_message_text_with_limit(message, DEFAULT_MESSAGE_MAX_BYTES)
}

/// Validate one inline or stdin message under the shared configured limit.
pub fn validate_message_text_with_limit(
    message: impl Into<String>,
    max_message_bytes: usize,
) -> Result<String, AtmError> {
    validate_message_size_limit(max_message_bytes)?;
    let message = message.into();
    if message.trim().is_empty() {
        return Err(AtmError::validation("message text cannot be empty"));
    }
    if message.len() > max_message_bytes {
        return Err(AtmError::validation(format!(
            "message text exceeds the {}-byte limit",
            max_message_bytes
        )));
    }

    Ok(message)
}

fn read_message_from_reader_with_limit(
    reader: impl Read,
    max_message_bytes: usize,
) -> Result<String, AtmError> {
    validate_message_size_limit(max_message_bytes)?;
    let mut bytes = Vec::new();
    reader
        .take((max_message_bytes + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            AtmError::new(
                AtmErrorCode::MailboxReadFailed,
                format!("failed to read stdin: {error}"),
            )
        })?;

    if bytes.len() > max_message_bytes {
        return Err(AtmError::validation(format!(
            "stdin message exceeds the {}-byte limit",
            max_message_bytes
        )));
    }

    let buffer = String::from_utf8(bytes).map_err(|error| {
        AtmError::new(
            AtmErrorCode::MailboxReadFailed,
            format!("failed to read stdin as UTF-8 text: {error}"),
        )
    })?;
    validate_message_text_with_limit(buffer, max_message_bytes)
}

#[cfg(test)]
mod tests {
    use std::io::{self, Cursor, Read};

    use super::{
        DEFAULT_MESSAGE_MAX_BYTES, read_message_from_reader_with_limit, validate_message_size_limit,
    };
    use crate::error_codes::AtmErrorCode;

    struct Unreadable;

    impl Read for Unreadable {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("stdin device failed"))
        }
    }

    #[test]
    fn read_message_from_reader_accepts_small_utf8_input() {
        let message = read_message_from_reader_with_limit(
            Cursor::new("hello from stdin"),
            DEFAULT_MESSAGE_MAX_BYTES,
        )
        .expect("stdin message");

        assert_eq!(message, "hello from stdin");
    }

    #[test]
    fn read_message_from_reader_rejects_oversized_input() {
        let oversized = "a".repeat(DEFAULT_MESSAGE_MAX_BYTES + 1);

        let error =
            read_message_from_reader_with_limit(Cursor::new(oversized), DEFAULT_MESSAGE_MAX_BYTES)
                .expect_err("oversized stdin");

        assert!(error.code() == crate::error_codes::AtmErrorCode::MessageValidationFailed);
        assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
        assert!(error.message().contains("stdin message exceeds"));
        assert!(error.message().contains("Recovery:"));
    }

    #[test]
    fn validate_message_text_rejects_oversized_inline_input() {
        let oversized = "a".repeat(DEFAULT_MESSAGE_MAX_BYTES + 1);

        let error = super::validate_message_text(oversized).expect_err("oversized inline message");

        assert!(error.message().contains("message text exceeds"));
    }

    #[test]
    fn read_message_from_reader_rejects_empty_and_whitespace_input() {
        for input in ["", " \n\t "] {
            let error =
                read_message_from_reader_with_limit(Cursor::new(input), DEFAULT_MESSAGE_MAX_BYTES)
                    .expect_err("empty stdin");
            assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
            assert!(error.message().contains("cannot be empty"));
        }
    }

    #[test]
    fn read_message_from_reader_rejects_non_utf8_input() {
        let error =
            read_message_from_reader_with_limit(Cursor::new(vec![0xff]), DEFAULT_MESSAGE_MAX_BYTES)
                .expect_err("non UTF-8 stdin");

        assert_eq!(error.code(), AtmErrorCode::MailboxReadFailed);
        assert!(error.message().contains("UTF-8"));
    }

    #[test]
    fn read_message_from_reader_reports_unreadable_input() {
        let error = read_message_from_reader_with_limit(Unreadable, DEFAULT_MESSAGE_MAX_BYTES)
            .expect_err("unreadable stdin");

        assert_eq!(error.code(), AtmErrorCode::MailboxReadFailed);
        assert!(error.message().contains("failed to read stdin"));
    }

    #[test]
    fn caller_cannot_widen_or_disable_the_message_size_policy() {
        for limit in [0, DEFAULT_MESSAGE_MAX_BYTES + 1] {
            let error = validate_message_size_limit(limit).expect_err("invalid message policy");

            assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
            assert!(error.message().contains("message byte limit"));
        }
    }
}
