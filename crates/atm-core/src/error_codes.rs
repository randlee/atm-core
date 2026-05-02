//! Stable ATM-owned error-code registry.
//!
//! These codes are the machine-readable contract for command failures and
//! degraded-warning diagnostics emitted by ATM.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};

/// Stable ATM error and warning codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AtmErrorCode {
    /// ATM home directory could not be resolved.
    ConfigHomeUnavailable,
    /// A generic ATM config parse failed.
    ConfigParseFailed,
    /// `.atm.toml` uses a retired post-send hook key.
    ConfigRetiredHookMembersKey,
    /// `.atm.toml` uses retired flat post-send hook keys.
    ConfigRetiredLegacyHookKeys,
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
    /// The mailbox lock path lives on a read-only filesystem.
    MailboxLockReadOnlyFilesystem,
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
            Self::ConfigRetiredLegacyHookKeys => "ATM_CONFIG_RETIRED_LEGACY_HOOK_KEYS",
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
            Self::MailboxLockReadOnlyFilesystem => "ATM_MAILBOX_LOCK_READ_ONLY_FILESYSTEM",
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

impl FromStr for AtmErrorCode {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "ATM_CONFIG_HOME_UNAVAILABLE" => Ok(Self::ConfigHomeUnavailable),
            "ATM_CONFIG_PARSE_FAILED" => Ok(Self::ConfigParseFailed),
            "ATM_CONFIG_RETIRED_HOOK_MEMBERS_KEY" => Ok(Self::ConfigRetiredHookMembersKey),
            "ATM_CONFIG_RETIRED_LEGACY_HOOK_KEYS" => Ok(Self::ConfigRetiredLegacyHookKeys),
            "ATM_CONFIG_TEAM_PARSE_FAILED" => Ok(Self::ConfigTeamParseFailed),
            "ATM_CONFIG_TEAM_MISSING" => Ok(Self::ConfigTeamMissing),
            "ATM_IDENTITY_UNAVAILABLE" => Ok(Self::IdentityUnavailable),
            "ATM_ADDRESS_PARSE_FAILED" => Ok(Self::AddressParseFailed),
            "ATM_TEAM_UNAVAILABLE" => Ok(Self::TeamUnavailable),
            "ATM_TEAM_NOT_FOUND" => Ok(Self::TeamNotFound),
            "ATM_AGENT_NOT_FOUND" => Ok(Self::AgentNotFound),
            "ATM_MAILBOX_READ_FAILED" => Ok(Self::MailboxReadFailed),
            "ATM_MAILBOX_WRITE_FAILED" => Ok(Self::MailboxWriteFailed),
            "ATM_MAILBOX_LOCK_FAILED" => Ok(Self::MailboxLockFailed),
            "ATM_MAILBOX_LOCK_READ_ONLY_FILESYSTEM" => Ok(Self::MailboxLockReadOnlyFilesystem),
            "ATM_MAILBOX_LOCK_TIMEOUT" => Ok(Self::MailboxLockTimeout),
            "ATM_MESSAGE_VALIDATION_FAILED" => Ok(Self::MessageValidationFailed),
            "ATM_SERIALIZATION_FAILED" => Ok(Self::SerializationFailed),
            "ATM_FILE_POLICY_REJECTED" => Ok(Self::FilePolicyRejected),
            "ATM_FILE_REFERENCE_REWRITE_FAILED" => Ok(Self::FileReferenceRewriteFailed),
            "ATM_WAIT_TIMEOUT" => Ok(Self::WaitTimeout),
            "ATM_ACK_INVALID_STATE" => Ok(Self::AckInvalidState),
            "ATM_CLEAR_INVALID_STATE" => Ok(Self::ClearInvalidState),
            "ATM_OBSERVABILITY_EMIT_FAILED" => Ok(Self::ObservabilityEmitFailed),
            "ATM_OBSERVABILITY_QUERY_FAILED" => Ok(Self::ObservabilityQueryFailed),
            "ATM_OBSERVABILITY_FOLLOW_FAILED" => Ok(Self::ObservabilityFollowFailed),
            "ATM_OBSERVABILITY_HEALTH_FAILED" => Ok(Self::ObservabilityHealthFailed),
            "ATM_OBSERVABILITY_BOOTSTRAP_FAILED" => Ok(Self::ObservabilityBootstrapFailed),
            "ATM_OBSERVABILITY_HEALTH_OK" => Ok(Self::ObservabilityHealthOk),
            "ATM_WARNING_INVALID_TEAM_MEMBER_SKIPPED" => Ok(Self::WarningInvalidTeamMemberSkipped),
            "ATM_WARNING_MAILBOX_RECORD_SKIPPED" => Ok(Self::WarningMailboxRecordSkipped),
            "ATM_WARNING_MALFORMED_ATM_FIELD_IGNORED" => Ok(Self::WarningMalformedAtmFieldIgnored),
            "ATM_WARNING_OBSERVABILITY_HEALTH_DEGRADED" => {
                Ok(Self::WarningObservabilityHealthDegraded)
            }
            "ATM_WARNING_ORIGIN_INBOX_ENTRY_SKIPPED" => Ok(Self::WarningOriginInboxEntrySkipped),
            "ATM_WARNING_MISSING_TEAM_CONFIG_FALLBACK" => {
                Ok(Self::WarningMissingTeamConfigFallback)
            }
            "ATM_WARNING_SEND_ALERT_STATE_DEGRADED" => Ok(Self::WarningSendAlertStateDegraded),
            "ATM_WARNING_IDENTITY_DRIFT" => Ok(Self::WarningIdentityDrift),
            "ATM_WARNING_BASELINE_MEMBER_MISSING" => Ok(Self::WarningBaselineMemberMissing),
            "ATM_WARNING_RESTORE_IN_PROGRESS" => Ok(Self::WarningRestoreInProgress),
            "ATM_WARNING_STALE_MAILBOX_LOCK" => Ok(Self::WarningStaleMailboxLock),
            "ATM_WARNING_HOOK_SKIPPED" => Ok(Self::WarningHookSkipped),
            "ATM_WARNING_HOOK_EXECUTION_FAILED" => Ok(Self::WarningHookExecutionFailed),
            _ => Err("unknown ATM error code"),
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

impl<'de> Deserialize<'de> for AtmErrorCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::AtmErrorCode;

    #[test]
    fn error_code_round_trips_through_json_string() {
        let encoded =
            serde_json::to_string(&AtmErrorCode::MailboxLockReadOnlyFilesystem).expect("serialize");
        assert_eq!(encoded, "\"ATM_MAILBOX_LOCK_READ_ONLY_FILESYSTEM\"");

        let decoded: AtmErrorCode = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, AtmErrorCode::MailboxLockReadOnlyFilesystem);
    }
}
