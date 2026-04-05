use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AtmErrorCode {
    ConfigHomeUnavailable,
    ConfigParseFailed,
    ConfigTeamParseFailed,
    ConfigTeamMissing,
    IdentityUnavailable,
    AddressParseFailed,
    TeamUnavailable,
    TeamNotFound,
    AgentNotFound,
    MailboxReadFailed,
    MailboxWriteFailed,
    MailboxRecordSkipped,
    MessageValidationFailed,
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
    WarningInvalidTeamMemberSkipped,
    WarningMailboxRecordSkipped,
    WarningMalformedAtmFieldIgnored,
    WarningOriginInboxEntrySkipped,
    WarningMissingTeamConfigFallback,
    WarningSendAlertStateDegraded,
}

impl AtmErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ConfigHomeUnavailable => "ATM_CONFIG_HOME_UNAVAILABLE",
            Self::ConfigParseFailed => "ATM_CONFIG_PARSE_FAILED",
            Self::ConfigTeamParseFailed => "ATM_CONFIG_TEAM_PARSE_FAILED",
            Self::ConfigTeamMissing => "ATM_CONFIG_TEAM_MISSING",
            Self::IdentityUnavailable => "ATM_IDENTITY_UNAVAILABLE",
            Self::AddressParseFailed => "ATM_ADDRESS_PARSE_FAILED",
            Self::TeamUnavailable => "ATM_TEAM_UNAVAILABLE",
            Self::TeamNotFound => "ATM_TEAM_NOT_FOUND",
            Self::AgentNotFound => "ATM_AGENT_NOT_FOUND",
            Self::MailboxReadFailed => "ATM_MAILBOX_READ_FAILED",
            Self::MailboxWriteFailed => "ATM_MAILBOX_WRITE_FAILED",
            Self::MailboxRecordSkipped => "ATM_MAILBOX_RECORD_SKIPPED",
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
            Self::WarningInvalidTeamMemberSkipped => "ATM_WARNING_INVALID_TEAM_MEMBER_SKIPPED",
            Self::WarningMailboxRecordSkipped => "ATM_WARNING_MAILBOX_RECORD_SKIPPED",
            Self::WarningMalformedAtmFieldIgnored => "ATM_WARNING_MALFORMED_ATM_FIELD_IGNORED",
            Self::WarningOriginInboxEntrySkipped => "ATM_WARNING_ORIGIN_INBOX_ENTRY_SKIPPED",
            Self::WarningMissingTeamConfigFallback => "ATM_WARNING_MISSING_TEAM_CONFIG_FALLBACK",
            Self::WarningSendAlertStateDegraded => "ATM_WARNING_SEND_ALERT_STATE_DEGRADED",
        }
    }
}

impl fmt::Display for AtmErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
