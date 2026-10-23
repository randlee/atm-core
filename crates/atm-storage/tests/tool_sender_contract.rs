//! Contract checks for stable signed tool-sender admission failures.

use atm_storage::{AtmErrorCode, ToolSenderFailure};

#[test]
fn tool_sender_failure_vocabulary_is_distinct_and_actionable() {
    let cases = [
        (
            ToolSenderFailure::SenderUnregistered,
            AtmErrorCode::ToolSenderUnregistered,
        ),
        (
            ToolSenderFailure::SenderReserved,
            AtmErrorCode::ToolSenderReserved,
        ),
        (
            ToolSenderFailure::SenderUnauthorized,
            AtmErrorCode::ToolSenderUnauthorized,
        ),
        (ToolSenderFailure::KeyUnknown, AtmErrorCode::ToolKeyUnknown),
        (ToolSenderFailure::KeyRevoked, AtmErrorCode::ToolKeyRevoked),
        (
            ToolSenderFailure::NonceReplay,
            AtmErrorCode::ToolNonceReplay,
        ),
        (
            ToolSenderFailure::HostMismatch,
            AtmErrorCode::ToolHostMismatch,
        ),
        (
            ToolSenderFailure::GenerationConflict,
            AtmErrorCode::ToolGenerationConflict,
        ),
        (
            ToolSenderFailure::SignatureInvalid,
            AtmErrorCode::ToolSignatureInvalid,
        ),
        (
            ToolSenderFailure::RequestExpired,
            AtmErrorCode::ToolRequestExpired,
        ),
        (
            ToolSenderFailure::CommitUncertain,
            AtmErrorCode::ToolCommitUncertain,
        ),
    ];

    for (failure, expected_code) in cases {
        let error = failure.into_error("controlled contract fixture");
        assert_eq!(error.code(), expected_code);
        assert!(error.message().contains("Recovery:"), "{expected_code}");
        assert_ne!(expected_code, AtmErrorCode::MessageValidationFailed);
    }
}
