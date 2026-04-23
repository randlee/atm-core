//! Stable ATM-owned error-code registry.
//!
//! These codes are the machine-readable contract for command failures and
//! degraded-warning diagnostics emitted by ATM.

use std::fmt;

use serde::Serialize;

/// Stable ATM error and warning codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AtmErrorCode {
    /// ATM home directory could not be resolved.
    ConfigHomeUnavailable,
    /// A generic ATM config parse failed.
    ConfigParseFailed,
    /// `.atm.toml` uses a retired post-send hook key.
    ConfigRetiredHookMembersKey,
    /// Team config parsing failed.
    ConfigTeamParseFailed,
    /// Team config document is missing.
    ConfigTeamMissing,
    /// Sender identity could not be resolved.
    IdentityUnavailable,
    /// Address parsing failed.
    AddressParseFailed,
    /// Team could not be resolved from config or input.
    TeamUnavailable,
    /// The requested team does not exist.
    TeamNotFound,
    /// The requested agent does not exist in the target team.
    AgentNotFound,
    /// Reading a mailbox failed.
    MailboxReadFailed,
    /// Writing a mailbox failed.
    MailboxWriteFailed,
    /// Acquiring or releasing a mailbox lock failed.
    MailboxLockFailed,
    /// Acquiring a mailbox lock timed out.
    MailboxLockTimeout,
    /// Message validation failed.
    MessageValidationFailed,
    /// Serialization or deserialization failed.
    SerializationFailed,
    /// File-policy enforcement rejected the operation.
    FilePolicyRejected,
    /// Rewriting a file reference failed.
    FileReferenceRewriteFailed,
    /// A wait/read timed out.
    WaitTimeout,
    /// Ack was attempted from an invalid state.
    AckInvalidState,
    /// Clear was attempted from an invalid state.
    ClearInvalidState,
    /// Emitting an observability event failed.
    ObservabilityEmitFailed,
    /// Querying retained observability records failed.
    ObservabilityQueryFailed,
    /// Starting or polling an observability follow session failed.
    ObservabilityFollowFailed,
    /// Observability health evaluation failed.
    ObservabilityHealthFailed,
    /// Observability bootstrap/initialization failed.
    ObservabilityBootstrapFailed,
    /// Observability health is healthy.
    ObservabilityHealthOk,
    /// A malformed team member record was skipped.
    WarningInvalidTeamMemberSkipped,
    /// A mailbox record was skipped during degraded recovery.
    WarningMailboxRecordSkipped,
    /// A malformed ATM-owned field was ignored.
    WarningMalformedAtmFieldIgnored,
    /// Observability health is degraded.
    WarningObservabilityHealthDegraded,
    /// An origin inbox entry was skipped.
    WarningOriginInboxEntrySkipped,
    /// Send fell back because the team config was missing.
    WarningMissingTeamConfigFallback,
    /// Send alert state degraded but the command continued.
    WarningSendAlertStateDegraded,
    /// Obsolete .atm.toml identity config is still present.
    WarningIdentityDrift,
    /// A baseline team member declared in .atm.toml is missing from config.json.
    WarningBaselineMemberMissing,
    /// A restore operation left a stale in-progress marker behind.
    WarningRestoreInProgress,
    /// A mailbox lock sentinel persisted for the full doctor run.
    WarningStaleMailboxLock,
    /// A configured post-send hook was skipped because no filter matched.
    WarningHookSkipped,
    /// A configured post-send hook failed during best-effort execution.
    WarningHookExecutionFailed,
}

impl AtmErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ConfigHomeUnavailable => "ATM_CONFIG_HOME_UNAVAILABLE",
            Self::ConfigParseFailed => "ATM_CONFIG_PARSE_FAILED",
            Self::ConfigRetiredHookMembersKey => "ATM_CONFIG_RETIRED_HOOK_MEMBERS_KEY",
            Self::ConfigTeamParseFailed => "ATM_CONFIG_TEAM_PARSE_FAILED",
            Self::ConfigTeamMissing => "ATM_CONFIG_TEAM_MISSING",
            Self::IdentityUnavailable => "ATM_IDENTITY_UNAVAILABLE",
            Self::AddressParseFailed => "ATM_ADDRESS_PARSE_FAILED",
            Self::TeamUnavailable => "ATM_TEAM_UNAVAILABLE",
            Self::TeamNotFound => "ATM_TEAM_NOT_FOUND",
            Self::AgentNotFound => "ATM_AGENT_NOT_FOUND",
            Self::MailboxReadFailed => "ATM_MAILBOX_READ_FAILED",
            Self::MailboxWriteFailed => "ATM_MAILBOX_WRITE_FAILED",
            Self::MailboxLockFailed => "ATM_MAILBOX_LOCK_FAILED",
            Self::MailboxLockTimeout => "ATM_MAILBOX_LOCK_TIMEOUT",
            Self::MessageValidationFailed => "ATM_MESSAGE_VALIDATION_FAILED",
            Self::SerializationFailed => "ATM_SERIALIZATION_FAILED",
            Self::FilePolicyRejected => "ATM_FILE_POLICY_REJECTED",
            Self::FileReferenceRewriteFailed => "ATM_FILE_REFERENCE_REWRITE_FAILED",
            Self::WaitTimeout => "ATM_WAIT_TIMEOUT",
            Self::AckInvalidState => "ATM_ACK_INVALID_STATE",
            Self::ClearInvalidState => "ATM_CLEAR_INVALID_STATE",
            Self::ObservabilityEmitFailed => "ATM_OBSERVABILITY_EMIT_FAILED",
            Self::ObservabilityQueryFailed => "ATM_OBSERVABILITY_QUERY_FAILED",
            Self::ObservabilityFollowFailed => "ATM_OBSERVABILITY_FOLLOW_FAILED",
            Self::ObservabilityHealthFailed => "ATM_OBSERVABILITY_HEALTH_FAILED",
            Self::ObservabilityBootstrapFailed => "ATM_OBSERVABILITY_BOOTSTRAP_FAILED",
            Self::ObservabilityHealthOk => "ATM_OBSERVABILITY_HEALTH_OK",
            Self::WarningInvalidTeamMemberSkipped => "ATM_WARNING_INVALID_TEAM_MEMBER_SKIPPED",
            Self::WarningMailboxRecordSkipped => "ATM_WARNING_MAILBOX_RECORD_SKIPPED",
            Self::WarningMalformedAtmFieldIgnored => "ATM_WARNING_MALFORMED_ATM_FIELD_IGNORED",
            Self::WarningObservabilityHealthDegraded => "ATM_WARNING_OBSERVABILITY_HEALTH_DEGRADED",
            Self::WarningOriginInboxEntrySkipped => "ATM_WARNING_ORIGIN_INBOX_ENTRY_SKIPPED",
            Self::WarningMissingTeamConfigFallback => "ATM_WARNING_MISSING_TEAM_CONFIG_FALLBACK",
            Self::WarningSendAlertStateDegraded => "ATM_WARNING_SEND_ALERT_STATE_DEGRADED",
            Self::WarningIdentityDrift => "ATM_WARNING_IDENTITY_DRIFT",
            Self::WarningBaselineMemberMissing => "ATM_WARNING_BASELINE_MEMBER_MISSING",
            Self::WarningRestoreInProgress => "ATM_WARNING_RESTORE_IN_PROGRESS",
            Self::WarningStaleMailboxLock => "ATM_WARNING_STALE_MAILBOX_LOCK",
            Self::WarningHookSkipped => "ATM_WARNING_HOOK_SKIPPED",
            Self::WarningHookExecutionFailed => "ATM_WARNING_HOOK_EXECUTION_FAILED",
        }
    }
}

impl fmt::Display for AtmErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for AtmErrorCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}
