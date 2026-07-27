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
    AtmHomeUnresolved,
    ConfigParseFailed,
    ConfigRetiredHookMembersKey,
    ConfigRetiredLegacyHookKeys,
    ConfigTeamParseFailed,
    ConfigTeamMissing,
    IdentityUnavailable,
    IdentityInvalid,
    IdentityConflict,
    MemberAlreadyExists,
    MemberNotFound,
    DaemonUnavailable,
    RuntimeRootInvalid,
    RuntimeBootstrapRefused,
    SocketOverrideForbidden,
    DaemonMayHaveExecuted,
    DaemonLifecycleWedge,
    DaemonLaunchGateRejected,
    DaemonServingStateRejected,
    DaemonStaleOwnerRecoveryFailed,
    DaemonAutoStartFailed,
    DaemonConnectionSaturated,
    RemoteDeliveryUnconfirmed,
    PeerConfigValidationFailed,
    CertificateOperationFailed,
    BindPreflightFailed,
    ClientDaemonVersionIncompatible,
    DaemonAdvisorySessionAlreadyRegistered,
    DaemonAdvisorySessionNotRegistered,
    DaemonAdvisorySessionCleanupFailed,
    AddressParseFailed,
    TeamUnavailable,
    TeamInvalid,
    TeamNotFound,
    AgentNotFound,
    MailboxReadFailed,
    MailboxWriteFailed,
    MailboxLockFailed,
    MailboxLockReadOnlyFilesystem,
    MailboxLockTimeout,
    InternalError,
    MessageValidationFailed,
    LocalHttpCapabilityInvalid,
    LocalHttpEndpointSchemaUnsupported,
    LocalHttpEndpointMissing,
    LocalHttpEndpointNonLoopback,
    LocalHttpRuntimeDirectoryMissing,
    LocalHttpCapabilityRevoked,
    MessageIdConflict,
    SelfAddressedSendInvalid,
    EmptyNudgeTemplateBody,
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
    PostSendGraftUnavailable,
    PostSendAdvisoryDeliveryFailed,
    TestFakeTransportInjectionFailed,
    HelpTopicNotFound,
}

impl AtmErrorCode {
    pub fn as_str(self) -> &'static str {
        if let Some(value) = self.config_or_identity_str() {
            return value;
        }
        if let Some(value) = self.daemon_or_address_str() {
            return value;
        }
        if let Some(value) = self.mailbox_or_validation_str() {
            return value;
        }
        if let Some(value) = self.observability_or_warning_str() {
            return value;
        }
        self.post_send_or_misc_str()
    }

    fn config_or_identity_str(self) -> Option<&'static str> {
        Some(match self {
            Self::ConfigHomeUnavailable => "ATM_CONFIG_HOME_UNAVAILABLE",
            Self::AtmHomeUnresolved => "ATM_HOME_UNRESOLVED",
            Self::ConfigParseFailed => "ATM_CONFIG_PARSE_FAILED",
            Self::ConfigRetiredHookMembersKey => "ATM_CONFIG_RETIRED_HOOK_MEMBERS_KEY",
            Self::ConfigRetiredLegacyHookKeys => "ATM_CONFIG_RETIRED_LEGACY_HOOK_KEYS",
            Self::ConfigTeamParseFailed => "ATM_CONFIG_TEAM_PARSE_FAILED",
            Self::ConfigTeamMissing => "ATM_CONFIG_TEAM_MISSING",
            Self::IdentityUnavailable => "ATM_IDENTITY_UNAVAILABLE",
            Self::IdentityInvalid => "ATM_IDENTITY_INVALID",
            Self::IdentityConflict => "ATM_IDENTITY_CONFLICT",
            Self::MemberAlreadyExists => "ATM_MEMBER_ALREADY_EXISTS",
            Self::MemberNotFound => "ATM_MEMBER_NOT_FOUND",
            _ => return None,
        })
    }

    fn daemon_or_address_str(self) -> Option<&'static str> {
        Some(match self {
            Self::DaemonUnavailable => "ATM_DAEMON_UNAVAILABLE",
            Self::RuntimeRootInvalid => "ATM_RUNTIME_ROOT_INVALID",
            Self::RuntimeBootstrapRefused => "ATM_RUNTIME_BOOTSTRAP_REFUSED",
            Self::SocketOverrideForbidden => "ATM_SOCKET_OVERRIDE_FORBIDDEN",
            Self::DaemonMayHaveExecuted => "ATM_DAEMON_MAY_HAVE_EXECUTED",
            Self::DaemonLifecycleWedge => "ATM_DAEMON_LIFECYCLE_WEDGE",
            Self::DaemonLaunchGateRejected => "ATM_DAEMON_LAUNCH_GATE_REJECTED",
            Self::DaemonServingStateRejected => "ATM_DAEMON_SERVING_STATE_REJECTED",
            Self::DaemonStaleOwnerRecoveryFailed => "ATM_DAEMON_STALE_OWNER_RECOVERY_FAILED",
            Self::DaemonAutoStartFailed => "ATM_DAEMON_AUTO_START_FAILED",
            Self::DaemonConnectionSaturated => "ATM_DAEMON_CONNECTION_SATURATED",
            Self::RemoteDeliveryUnconfirmed => "REMOTE_DELIVERY_UNCONFIRMED",
            Self::PeerConfigValidationFailed => "ATM_PEER_CONFIG_VALIDATION_FAILED",
            Self::CertificateOperationFailed => "ATM_CERTIFICATE_OPERATION_FAILED",
            Self::BindPreflightFailed => "ATM_BIND_PREFLIGHT_FAILED",
            Self::ClientDaemonVersionIncompatible => "ATM_CLIENT_DAEMON_VERSION_INCOMPATIBLE",
            Self::DaemonAdvisorySessionAlreadyRegistered => {
                "ATM_DAEMON_ADVISORY_SESSION_ALREADY_REGISTERED"
            }
            Self::DaemonAdvisorySessionNotRegistered => {
                "ATM_DAEMON_ADVISORY_SESSION_NOT_REGISTERED"
            }
            Self::DaemonAdvisorySessionCleanupFailed => {
                "ATM_DAEMON_ADVISORY_SESSION_CLEANUP_FAILED"
            }
            Self::AddressParseFailed => "ATM_ADDRESS_PARSE_FAILED",
            Self::TeamUnavailable => "ATM_TEAM_UNAVAILABLE",
            Self::TeamInvalid => "ATM_TEAM_INVALID",
            Self::TeamNotFound => "ATM_TEAM_NOT_FOUND",
            Self::AgentNotFound => "ATM_AGENT_NOT_FOUND",
            _ => return None,
        })
    }

    fn mailbox_or_validation_str(self) -> Option<&'static str> {
        Some(match self {
            Self::MailboxReadFailed => "ATM_MAILBOX_READ_FAILED",
            Self::MailboxWriteFailed => "ATM_MAILBOX_WRITE_FAILED",
            Self::MailboxLockFailed => "ATM_MAILBOX_LOCK_FAILED",
            Self::MailboxLockReadOnlyFilesystem => "ATM_MAILBOX_LOCK_READ_ONLY_FILESYSTEM",
            Self::MailboxLockTimeout => "ATM_MAILBOX_LOCK_TIMEOUT",
            Self::InternalError => "ATM_INTERNAL_ERROR",
            Self::MessageValidationFailed => "ATM_MESSAGE_VALIDATION_FAILED",
            Self::LocalHttpCapabilityInvalid => "ATM_LOCAL_HTTP_CAPABILITY_INVALID",
            Self::LocalHttpEndpointSchemaUnsupported => {
                "ATM_LOCAL_HTTP_ENDPOINT_SCHEMA_UNSUPPORTED"
            }
            Self::LocalHttpEndpointMissing => "ATM_LOCAL_HTTP_ENDPOINT_MISSING",
            Self::LocalHttpEndpointNonLoopback => "ATM_LOCAL_HTTP_ENDPOINT_NON_LOOPBACK",
            Self::LocalHttpRuntimeDirectoryMissing => "ATM_LOCAL_HTTP_RUNTIME_DIRECTORY_MISSING",
            Self::LocalHttpCapabilityRevoked => "ATM_LOCAL_HTTP_CAPABILITY_REVOKED",
            Self::MessageIdConflict => "ATM_MESSAGE_ID_CONFLICT",
            Self::SelfAddressedSendInvalid => "ATM_SELF_ADDRESSED_SEND_INVALID",
            Self::EmptyNudgeTemplateBody => "ATM_NUDGE_TEMPLATE_BODY_EMPTY",
            Self::CallerContextRequestInvalid => "ATM_CALLER_CONTEXT_REQUEST_INVALID",
            Self::SerializationFailed => "ATM_SERIALIZATION_FAILED",
            Self::FilePolicyRejected => "ATM_FILE_POLICY_REJECTED",
            Self::FileReferenceRewriteFailed => "ATM_FILE_REFERENCE_REWRITE_FAILED",
            Self::WaitTimeout => "ATM_WAIT_TIMEOUT",
            Self::AckInvalidState => "ATM_ACK_INVALID_STATE",
            Self::ClearInvalidState => "ATM_CLEAR_INVALID_STATE",
            _ => return None,
        })
    }

    fn observability_or_warning_str(self) -> Option<&'static str> {
        Some(match self {
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
            _ => return None,
        })
    }

    fn post_send_or_misc_str(self) -> &'static str {
        match self {
            Self::PostSendPaneMissing => "ATM_POST_SEND_PANE_MISSING",
            Self::PostSendTmuxSendFailed => "ATM_POST_SEND_TMUX_SEND_FAILED",
            Self::PostSendGraftUnavailable => "ATM_POST_SEND_GRAFT_UNAVAILABLE",
            Self::PostSendAdvisoryDeliveryFailed => "ATM_POST_SEND_ADVISORY_DELIVERY_FAILED",
            Self::TestFakeTransportInjectionFailed => "ATM_TEST_FAKE_TRANSPORT_INJECTION_FAILED",
            Self::HelpTopicNotFound => "ATM_HELP_TOPIC_NOT_FOUND",
            _ => unreachable!("all AtmErrorCode variants must be covered by as_str helpers"),
        }
    }
}

impl FromStr for AtmErrorCode {
    type Err = UnknownAtmErrorCode;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        parse_known_error_code(value).ok_or_else(|| UnknownAtmErrorCode(value.to_owned()))
    }
}

fn parse_known_error_code(value: &str) -> Option<AtmErrorCode> {
    parse_config_or_identity_code(value)
        .or_else(|| parse_daemon_or_address_code(value))
        .or_else(|| parse_mailbox_or_validation_code(value))
        .or_else(|| parse_observability_or_warning_code(value))
        .or_else(|| parse_post_send_or_misc_code(value))
}

fn parse_config_or_identity_code(value: &str) -> Option<AtmErrorCode> {
    Some(match value {
        "ATM_CONFIG_HOME_UNAVAILABLE" => AtmErrorCode::ConfigHomeUnavailable,
        "ATM_HOME_UNRESOLVED" => AtmErrorCode::AtmHomeUnresolved,
        "ATM_CONFIG_PARSE_FAILED" => AtmErrorCode::ConfigParseFailed,
        "ATM_CONFIG_RETIRED_HOOK_MEMBERS_KEY" => AtmErrorCode::ConfigRetiredHookMembersKey,
        "ATM_CONFIG_RETIRED_LEGACY_HOOK_KEYS" => AtmErrorCode::ConfigRetiredLegacyHookKeys,
        "ATM_CONFIG_TEAM_PARSE_FAILED" => AtmErrorCode::ConfigTeamParseFailed,
        "ATM_CONFIG_TEAM_MISSING" => AtmErrorCode::ConfigTeamMissing,
        "ATM_IDENTITY_UNAVAILABLE" => AtmErrorCode::IdentityUnavailable,
        "ATM_IDENTITY_INVALID" => AtmErrorCode::IdentityInvalid,
        "ATM_IDENTITY_CONFLICT" => AtmErrorCode::IdentityConflict,
        "ATM_MEMBER_ALREADY_EXISTS" => AtmErrorCode::MemberAlreadyExists,
        "ATM_MEMBER_NOT_FOUND" => AtmErrorCode::MemberNotFound,
        _ => return None,
    })
}

fn parse_daemon_or_address_code(value: &str) -> Option<AtmErrorCode> {
    Some(match value {
        "ATM_DAEMON_UNAVAILABLE" => AtmErrorCode::DaemonUnavailable,
        "ATM_RUNTIME_ROOT_INVALID" => AtmErrorCode::RuntimeRootInvalid,
        "ATM_RUNTIME_BOOTSTRAP_REFUSED" => AtmErrorCode::RuntimeBootstrapRefused,
        "ATM_SOCKET_OVERRIDE_FORBIDDEN" => AtmErrorCode::SocketOverrideForbidden,
        "ATM_DAEMON_MAY_HAVE_EXECUTED" => AtmErrorCode::DaemonMayHaveExecuted,
        "ATM_DAEMON_LIFECYCLE_WEDGE" => AtmErrorCode::DaemonLifecycleWedge,
        "ATM_DAEMON_LAUNCH_GATE_REJECTED" => AtmErrorCode::DaemonLaunchGateRejected,
        "ATM_DAEMON_SERVING_STATE_REJECTED" => AtmErrorCode::DaemonServingStateRejected,
        "ATM_DAEMON_STALE_OWNER_RECOVERY_FAILED" => AtmErrorCode::DaemonStaleOwnerRecoveryFailed,
        "ATM_DAEMON_AUTO_START_FAILED" => AtmErrorCode::DaemonAutoStartFailed,
        "ATM_DAEMON_CONNECTION_SATURATED" => AtmErrorCode::DaemonConnectionSaturated,
        "REMOTE_DELIVERY_UNCONFIRMED" => AtmErrorCode::RemoteDeliveryUnconfirmed,
        "ATM_PEER_CONFIG_VALIDATION_FAILED" => AtmErrorCode::PeerConfigValidationFailed,
        "ATM_CERTIFICATE_OPERATION_FAILED" => AtmErrorCode::CertificateOperationFailed,
        "ATM_BIND_PREFLIGHT_FAILED" => AtmErrorCode::BindPreflightFailed,
        "ATM_CLIENT_DAEMON_VERSION_INCOMPATIBLE" => AtmErrorCode::ClientDaemonVersionIncompatible,
        "ATM_DAEMON_ADVISORY_SESSION_ALREADY_REGISTERED" => {
            AtmErrorCode::DaemonAdvisorySessionAlreadyRegistered
        }
        "ATM_DAEMON_ADVISORY_SESSION_NOT_REGISTERED" => {
            AtmErrorCode::DaemonAdvisorySessionNotRegistered
        }
        "ATM_DAEMON_ADVISORY_SESSION_CLEANUP_FAILED" => {
            AtmErrorCode::DaemonAdvisorySessionCleanupFailed
        }
        "ATM_ADDRESS_PARSE_FAILED" => AtmErrorCode::AddressParseFailed,
        "ATM_TEAM_UNAVAILABLE" => AtmErrorCode::TeamUnavailable,
        "ATM_TEAM_INVALID" => AtmErrorCode::TeamInvalid,
        "ATM_TEAM_NOT_FOUND" => AtmErrorCode::TeamNotFound,
        "ATM_AGENT_NOT_FOUND" => AtmErrorCode::AgentNotFound,
        _ => return None,
    })
}

fn parse_mailbox_or_validation_code(value: &str) -> Option<AtmErrorCode> {
    Some(match value {
        "ATM_MAILBOX_READ_FAILED" => AtmErrorCode::MailboxReadFailed,
        "ATM_MAILBOX_WRITE_FAILED" => AtmErrorCode::MailboxWriteFailed,
        "ATM_MAILBOX_LOCK_FAILED" => AtmErrorCode::MailboxLockFailed,
        "ATM_MAILBOX_LOCK_READ_ONLY_FILESYSTEM" => AtmErrorCode::MailboxLockReadOnlyFilesystem,
        "ATM_MAILBOX_LOCK_TIMEOUT" => AtmErrorCode::MailboxLockTimeout,
        "ATM_INTERNAL_ERROR" => AtmErrorCode::InternalError,
        "ATM_MESSAGE_VALIDATION_FAILED" => AtmErrorCode::MessageValidationFailed,
        "ATM_LOCAL_HTTP_CAPABILITY_INVALID" => AtmErrorCode::LocalHttpCapabilityInvalid,
        "ATM_LOCAL_HTTP_ENDPOINT_SCHEMA_UNSUPPORTED" => {
            AtmErrorCode::LocalHttpEndpointSchemaUnsupported
        }
        "ATM_LOCAL_HTTP_ENDPOINT_MISSING" => AtmErrorCode::LocalHttpEndpointMissing,
        "ATM_LOCAL_HTTP_ENDPOINT_NON_LOOPBACK" => AtmErrorCode::LocalHttpEndpointNonLoopback,
        "ATM_LOCAL_HTTP_RUNTIME_DIRECTORY_MISSING" => {
            AtmErrorCode::LocalHttpRuntimeDirectoryMissing
        }
        "ATM_LOCAL_HTTP_CAPABILITY_REVOKED" => AtmErrorCode::LocalHttpCapabilityRevoked,
        "ATM_MESSAGE_ID_CONFLICT" => AtmErrorCode::MessageIdConflict,
        "ATM_SELF_ADDRESSED_SEND_INVALID" => AtmErrorCode::SelfAddressedSendInvalid,
        "ATM_NUDGE_TEMPLATE_BODY_EMPTY" => AtmErrorCode::EmptyNudgeTemplateBody,
        "ATM_CALLER_CONTEXT_REQUEST_INVALID" => AtmErrorCode::CallerContextRequestInvalid,
        "ATM_SERIALIZATION_FAILED" => AtmErrorCode::SerializationFailed,
        "ATM_FILE_POLICY_REJECTED" => AtmErrorCode::FilePolicyRejected,
        "ATM_FILE_REFERENCE_REWRITE_FAILED" => AtmErrorCode::FileReferenceRewriteFailed,
        "ATM_WAIT_TIMEOUT" => AtmErrorCode::WaitTimeout,
        "ATM_ACK_INVALID_STATE" => AtmErrorCode::AckInvalidState,
        "ATM_CLEAR_INVALID_STATE" => AtmErrorCode::ClearInvalidState,
        _ => return None,
    })
}

fn parse_observability_or_warning_code(value: &str) -> Option<AtmErrorCode> {
    Some(match value {
        "ATM_OBSERVABILITY_EMIT_FAILED" => AtmErrorCode::ObservabilityEmitFailed,
        "ATM_OBSERVABILITY_QUERY_FAILED" => AtmErrorCode::ObservabilityQueryFailed,
        "ATM_OBSERVABILITY_FOLLOW_FAILED" => AtmErrorCode::ObservabilityFollowFailed,
        "ATM_OBSERVABILITY_HEALTH_FAILED" => AtmErrorCode::ObservabilityHealthFailed,
        "ATM_OBSERVABILITY_BOOTSTRAP_FAILED" => AtmErrorCode::ObservabilityBootstrapFailed,
        "ATM_OBSERVABILITY_HEALTH_OK" => AtmErrorCode::ObservabilityHealthOk,
        "ATM_WARNING_INVALID_TEAM_MEMBER_SKIPPED" => AtmErrorCode::WarningInvalidTeamMemberSkipped,
        "ATM_WARNING_MAILBOX_RECORD_SKIPPED" => AtmErrorCode::WarningMailboxRecordSkipped,
        "ATM_WARNING_MALFORMED_ATM_FIELD_IGNORED" => AtmErrorCode::WarningMalformedAtmFieldIgnored,
        "ATM_WARNING_OBSERVABILITY_HEALTH_DEGRADED" => {
            AtmErrorCode::WarningObservabilityHealthDegraded
        }
        "ATM_WARNING_SQLITE_HEALTH_DEGRADED" => AtmErrorCode::WarningSqliteHealthDegraded,
        "ATM_WARNING_ORIGIN_INBOX_ENTRY_SKIPPED" => AtmErrorCode::WarningOriginInboxEntrySkipped,
        "ATM_WARNING_MISSING_TEAM_CONFIG_FALLBACK" => {
            AtmErrorCode::WarningMissingTeamConfigFallback
        }
        "ATM_WARNING_SEND_ALERT_STATE_DEGRADED" => AtmErrorCode::WarningSendAlertStateDegraded,
        "ATM_WARNING_IDENTITY_DRIFT" => AtmErrorCode::WarningIdentityDrift,
        "ATM_WARNING_ROSTER_DRIFT" => AtmErrorCode::WarningRosterDrift,
        "ATM_WARNING_BASELINE_MEMBER_MISSING" => AtmErrorCode::WarningBaselineMemberMissing,
        "ATM_WARNING_RESTORE_IN_PROGRESS" => AtmErrorCode::WarningRestoreInProgress,
        "ATM_WARNING_STALE_MAILBOX_LOCK" => AtmErrorCode::WarningStaleMailboxLock,
        "ATM_WARNING_HOOK_SKIPPED" => AtmErrorCode::WarningHookSkipped,
        "ATM_WARNING_HOOK_EXECUTION_FAILED" => AtmErrorCode::WarningHookExecutionFailed,
        _ => return None,
    })
}

fn parse_post_send_or_misc_code(value: &str) -> Option<AtmErrorCode> {
    Some(match value {
        "ATM_POST_SEND_PANE_MISSING" => AtmErrorCode::PostSendPaneMissing,
        "ATM_POST_SEND_TMUX_SEND_FAILED" => AtmErrorCode::PostSendTmuxSendFailed,
        "ATM_POST_SEND_GRAFT_UNAVAILABLE" => AtmErrorCode::PostSendGraftUnavailable,
        "ATM_POST_SEND_ADVISORY_DELIVERY_FAILED" => AtmErrorCode::PostSendAdvisoryDeliveryFailed,
        "ATM_TEST_FAKE_TRANSPORT_INJECTION_FAILED" => {
            AtmErrorCode::TestFakeTransportInjectionFailed
        }
        "ATM_HELP_TOPIC_NOT_FOUND" => AtmErrorCode::HelpTopicNotFound,
        _ => return None,
    })
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
