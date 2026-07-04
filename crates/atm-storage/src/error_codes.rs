//! Stable ATM-owned error-code registry.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownAtmErrorCode(pub String);

impl fmt::Display for UnknownAtmErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown ATM error code: {}", self.0)
    }
}

impl std::error::Error for UnknownAtmErrorCode {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AtmErrorCode {
    ConfigHomeUnavailable,
    ConfigParseFailed,
    ConfigRetiredHookMembersKey,
    ConfigRetiredLegacyHookKeys,
    ConfigTeamParseFailed,
    ConfigTeamMissing,
    IdentityUnavailable,
    IdentityInvalid,
    IdentityConflict,
    DaemonUnavailable,
    DaemonMayHaveExecuted,
    DaemonLifecycleWedge,
    DaemonLaunchGateRejected,
    DaemonServingStateRejected,
    DaemonStaleOwnerRecoveryFailed,
    DaemonAutoStartFailed,
    DaemonAdvisorySessionAlreadyRegistered,
    DaemonAdvisorySessionNotRegistered,
    RemoteDeliveryOutcomeUnknown,
    AddressParseFailed,
    TeamUnavailable,
    TeamInvalid,
    TeamNotFound,
    AgentNotFound,
    MailboxReadFailed,
    MailboxRecoveredMessageSetTooLarge,
    MailboxWriteFailed,
    MailboxLockFailed,
    MailboxLockReadOnlyFilesystem,
    MailboxLockTimeout,
    InternalError,
    MessageValidationFailed,
    CallerContextRequestInvalid,
    SerializationFailed,
    FilePolicyRejected,
    FileReferenceRewriteFailed,
    WaitTimeout,
    AckInvalidState,
    ClearInvalidState,
    ObservabilityEmitFailed,
    ObservabilityQueryFailed,
    ObservabilityFollowFailed,
    ObservabilityHealthFailed,
    ObservabilityBootstrapFailed,
    ObservabilityHealthOk,
    WarningInvalidTeamMemberSkipped,
    WarningMailboxRecordSkipped,
    WarningMalformedAtmFieldIgnored,
    WarningObservabilityHealthDegraded,
    WarningSqliteHealthDegraded,
    WarningOriginInboxEntrySkipped,
    WarningMissingTeamConfigFallback,
    WarningSendAlertStateDegraded,
    WarningIdentityDrift,
    WarningRosterDrift,
    WarningBaselineMemberMissing,
    WarningRestoreInProgress,
    WarningStaleMailboxLock,
    WarningHookSkipped,
    WarningHookExecutionFailed,
    PostSendPaneMissing,
    PostSendTmuxSendFailed,
    TestFakeTransportInjectionFailed,
    HelpTopicNotFound,
}

impl AtmErrorCode {
    const ALL: [Self; 64] = [
        Self::ConfigHomeUnavailable,
        Self::ConfigParseFailed,
        Self::ConfigRetiredHookMembersKey,
        Self::ConfigRetiredLegacyHookKeys,
        Self::ConfigTeamParseFailed,
        Self::ConfigTeamMissing,
        Self::IdentityUnavailable,
        Self::IdentityInvalid,
        Self::IdentityConflict,
        Self::DaemonUnavailable,
        Self::DaemonMayHaveExecuted,
        Self::DaemonLifecycleWedge,
        Self::DaemonLaunchGateRejected,
        Self::DaemonServingStateRejected,
        Self::DaemonStaleOwnerRecoveryFailed,
        Self::DaemonAutoStartFailed,
        Self::DaemonAdvisorySessionAlreadyRegistered,
        Self::DaemonAdvisorySessionNotRegistered,
        Self::RemoteDeliveryOutcomeUnknown,
        Self::AddressParseFailed,
        Self::TeamUnavailable,
        Self::TeamInvalid,
        Self::TeamNotFound,
        Self::AgentNotFound,
        Self::MailboxReadFailed,
        Self::MailboxRecoveredMessageSetTooLarge,
        Self::MailboxWriteFailed,
        Self::MailboxLockFailed,
        Self::MailboxLockReadOnlyFilesystem,
        Self::MailboxLockTimeout,
        Self::InternalError,
        Self::MessageValidationFailed,
        Self::CallerContextRequestInvalid,
        Self::SerializationFailed,
        Self::FilePolicyRejected,
        Self::FileReferenceRewriteFailed,
        Self::WaitTimeout,
        Self::AckInvalidState,
        Self::ClearInvalidState,
        Self::ObservabilityEmitFailed,
        Self::ObservabilityQueryFailed,
        Self::ObservabilityFollowFailed,
        Self::ObservabilityHealthFailed,
        Self::ObservabilityBootstrapFailed,
        Self::ObservabilityHealthOk,
        Self::WarningInvalidTeamMemberSkipped,
        Self::WarningMailboxRecordSkipped,
        Self::WarningMalformedAtmFieldIgnored,
        Self::WarningObservabilityHealthDegraded,
        Self::WarningSqliteHealthDegraded,
        Self::WarningOriginInboxEntrySkipped,
        Self::WarningMissingTeamConfigFallback,
        Self::WarningSendAlertStateDegraded,
        Self::WarningIdentityDrift,
        Self::WarningRosterDrift,
        Self::WarningBaselineMemberMissing,
        Self::WarningRestoreInProgress,
        Self::WarningStaleMailboxLock,
        Self::WarningHookSkipped,
        Self::WarningHookExecutionFailed,
        Self::PostSendPaneMissing,
        Self::PostSendTmuxSendFailed,
        Self::TestFakeTransportInjectionFailed,
        Self::HelpTopicNotFound,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ConfigHomeUnavailable => "ATM_CONFIG_HOME_UNAVAILABLE",
            Self::ConfigParseFailed => "ATM_CONFIG_PARSE_FAILED",
            Self::ConfigRetiredHookMembersKey => "ATM_CONFIG_RETIRED_HOOK_MEMBERS_KEY",
            Self::ConfigRetiredLegacyHookKeys => "ATM_CONFIG_RETIRED_LEGACY_HOOK_KEYS",
            Self::ConfigTeamParseFailed => "ATM_CONFIG_TEAM_PARSE_FAILED",
            Self::ConfigTeamMissing => "ATM_CONFIG_TEAM_MISSING",
            Self::IdentityUnavailable => "ATM_IDENTITY_UNAVAILABLE",
            Self::IdentityInvalid => "ATM_IDENTITY_INVALID",
            Self::IdentityConflict => "ATM_IDENTITY_CONFLICT",
            Self::DaemonUnavailable => "ATM_DAEMON_UNAVAILABLE",
            Self::DaemonMayHaveExecuted => "ATM_DAEMON_MAY_HAVE_EXECUTED",
            Self::DaemonLifecycleWedge => "ATM_DAEMON_LIFECYCLE_WEDGE",
            Self::DaemonLaunchGateRejected => "ATM_DAEMON_LAUNCH_GATE_REJECTED",
            Self::DaemonServingStateRejected => "ATM_DAEMON_SERVING_STATE_REJECTED",
            Self::DaemonStaleOwnerRecoveryFailed => "ATM_DAEMON_STALE_OWNER_RECOVERY_FAILED",
            Self::DaemonAutoStartFailed => "ATM_DAEMON_AUTO_START_FAILED",
            Self::DaemonAdvisorySessionAlreadyRegistered => {
                "ATM_DAEMON_ADVISORY_SESSION_ALREADY_REGISTERED"
            }
            Self::DaemonAdvisorySessionNotRegistered => {
                "ATM_DAEMON_ADVISORY_SESSION_NOT_REGISTERED"
            }
            Self::RemoteDeliveryOutcomeUnknown => "ATM_REMOTE_OUTCOME_UNKNOWN",
            Self::AddressParseFailed => "ATM_ADDRESS_PARSE_FAILED",
            Self::TeamUnavailable => "ATM_TEAM_UNAVAILABLE",
            Self::TeamInvalid => "ATM_TEAM_INVALID",
            Self::TeamNotFound => "ATM_TEAM_NOT_FOUND",
            Self::AgentNotFound => "ATM_AGENT_NOT_FOUND",
            Self::MailboxReadFailed => "ATM_MAILBOX_READ_FAILED",
            Self::MailboxRecoveredMessageSetTooLarge => {
                "ATM_MAILBOX_RECOVERED_MESSAGE_SET_TOO_LARGE"
            }
            Self::MailboxWriteFailed => "ATM_MAILBOX_WRITE_FAILED",
            Self::MailboxLockFailed => "ATM_MAILBOX_LOCK_FAILED",
            Self::MailboxLockReadOnlyFilesystem => "ATM_MAILBOX_LOCK_READ_ONLY_FILESYSTEM",
            Self::MailboxLockTimeout => "ATM_MAILBOX_LOCK_TIMEOUT",
            Self::InternalError => "ATM_INTERNAL_ERROR",
            Self::MessageValidationFailed => "ATM_MESSAGE_VALIDATION_FAILED",
            Self::CallerContextRequestInvalid => "ATM_CALLER_CONTEXT_REQUEST_INVALID",
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
            Self::WarningSqliteHealthDegraded => "ATM_WARNING_SQLITE_HEALTH_DEGRADED",
            Self::WarningOriginInboxEntrySkipped => "ATM_WARNING_ORIGIN_INBOX_ENTRY_SKIPPED",
            Self::WarningMissingTeamConfigFallback => "ATM_WARNING_MISSING_TEAM_CONFIG_FALLBACK",
            Self::WarningSendAlertStateDegraded => "ATM_WARNING_SEND_ALERT_STATE_DEGRADED",
            Self::WarningIdentityDrift => "ATM_WARNING_IDENTITY_DRIFT",
            Self::WarningRosterDrift => "ATM_WARNING_ROSTER_DRIFT",
            Self::WarningBaselineMemberMissing => "ATM_WARNING_BASELINE_MEMBER_MISSING",
            Self::WarningRestoreInProgress => "ATM_WARNING_RESTORE_IN_PROGRESS",
            Self::WarningStaleMailboxLock => "ATM_WARNING_STALE_MAILBOX_LOCK",
            Self::WarningHookSkipped => "ATM_WARNING_HOOK_SKIPPED",
            Self::WarningHookExecutionFailed => "ATM_WARNING_HOOK_EXECUTION_FAILED",
            Self::PostSendPaneMissing => "ATM_POST_SEND_PANE_MISSING",
            Self::PostSendTmuxSendFailed => "ATM_POST_SEND_TMUX_SEND_FAILED",
            Self::TestFakeTransportInjectionFailed => "ATM_TEST_FAKE_TRANSPORT_INJECTION_FAILED",
            Self::HelpTopicNotFound => "ATM_HELP_TOPIC_NOT_FOUND",
        }
    }
}

impl FromStr for AtmErrorCode {
    type Err = UnknownAtmErrorCode;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|code| code.as_str() == value)
            .ok_or_else(|| UnknownAtmErrorCode(value.to_owned()))
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
