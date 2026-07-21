use std::fmt;

use serde::{Deserialize, Serialize};

pub use crate::error_codes::AtmErrorCode;

/// ATM's sole serializable error contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AtmError {
    code: AtmErrorCode,
    message: String,
}

impl AtmError {
    /// Constructs the canonical error shape through the centralized catalog.
    pub fn new(code: AtmErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            message: crate::error_catalog::render(code, detail),
        }
    }

    /// Constructs the canonical error using the catalog's code-owned text.
    pub fn for_code(code: AtmErrorCode) -> Self {
        Self {
            code,
            message: crate::error_catalog::render_code(code),
        }
    }

    #[must_use]
    pub const fn code(&self) -> AtmErrorCode {
        self.code
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub fn into_message(self) -> String {
        self.message
    }

    #[must_use]
    pub fn is_validation(&self) -> bool {
        self.code == AtmErrorCode::MessageValidationFailed
    }

    #[must_use]
    pub fn is_daemon_unavailable(&self) -> bool {
        self.code == AtmErrorCode::DaemonUnavailable
    }

    #[must_use]
    pub fn is_config(&self) -> bool {
        matches!(
            self.code,
            AtmErrorCode::ConfigHomeUnavailable
                | AtmErrorCode::ConfigParseFailed
                | AtmErrorCode::ConfigRetiredHookMembersKey
                | AtmErrorCode::ConfigRetiredLegacyHookKeys
                | AtmErrorCode::ConfigTeamParseFailed
                | AtmErrorCode::ConfigTeamMissing
        )
    }

    #[must_use]
    pub fn is_observability_bootstrap(&self) -> bool {
        self.code == AtmErrorCode::ObservabilityBootstrapFailed
    }

    pub fn home_directory_unavailable() -> Self {
        Self::new(
            AtmErrorCode::ConfigHomeUnavailable,
            "home directory is unavailable",
        )
    }

    pub fn atm_home_unresolved(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::AtmHomeUnresolved, message)
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::ConfigParseFailed, message)
    }

    pub fn address_parse(message: impl Into<String>) -> Self {
        Self::new(
            AtmErrorCode::AddressParseFailed,
            format!("address parse failed: {}", message.into()),
        )
    }

    pub fn identity_unavailable() -> Self {
        Self::new(
            AtmErrorCode::IdentityUnavailable,
            "identity is not configured",
        )
    }

    pub fn identity_invalid(message: impl Into<String>) -> Self {
        Self::new(
            AtmErrorCode::IdentityInvalid,
            format!("caller identity is invalid: {}", message.into()),
        )
    }

    pub fn identity_conflict(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::IdentityConflict, message)
    }

    pub fn member_already_exists(member: &str, team: &str) -> Self {
        Self::new(
            AtmErrorCode::MemberAlreadyExists,
            format!("member '{member}' already exists in team '{team}'"),
        )
    }

    pub fn member_not_found(member: &str, team: &str) -> Self {
        Self::new(
            AtmErrorCode::MemberNotFound,
            format!("member '{member}' was not found in team '{team}'"),
        )
    }

    pub fn daemon_unavailable(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::DaemonUnavailable, message)
    }

    pub fn runtime_root_invalid(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::RuntimeRootInvalid, message)
    }

    pub fn runtime_bootstrap_refused(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::RuntimeBootstrapRefused, message)
    }

    pub fn socket_override_forbidden(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::SocketOverrideForbidden, message)
    }

    pub fn daemon_may_have_executed(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::DaemonMayHaveExecuted, message)
    }

    pub fn daemon_lifecycle_wedge(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::DaemonLifecycleWedge, message)
    }

    pub fn daemon_advisory_session_already_registered(message: impl Into<String>) -> Self {
        Self::new(
            AtmErrorCode::DaemonAdvisorySessionAlreadyRegistered,
            message,
        )
    }

    pub fn daemon_advisory_session_not_registered(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::DaemonAdvisorySessionNotRegistered, message)
    }

    pub fn daemon_advisory_session_cleanup_failed(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::DaemonAdvisorySessionCleanupFailed, message)
    }

    pub fn daemon_launch_gate_rejected(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::DaemonLaunchGateRejected, message)
    }

    pub fn daemon_serving_state_rejected(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::DaemonServingStateRejected, message)
    }

    pub fn daemon_stale_owner_recovery_failed(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::DaemonStaleOwnerRecoveryFailed, message)
    }

    pub fn daemon_auto_start_failed(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::DaemonAutoStartFailed, message)
    }

    pub fn help_topic_not_found(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::HelpTopicNotFound, message)
    }

    pub fn test_fake_transport_injection_failed(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::TestFakeTransportInjectionFailed, message)
    }

    pub fn team_unavailable() -> Self {
        Self::new(AtmErrorCode::TeamUnavailable, "team is not configured")
    }

    pub fn team_invalid(message: impl Into<String>) -> Self {
        Self::new(
            AtmErrorCode::TeamInvalid,
            format!("caller team is invalid: {}", message.into()),
        )
    }

    pub fn team_not_found(team: &str) -> Self {
        Self::new(
            AtmErrorCode::TeamNotFound,
            format!("team '{team}' was not found"),
        )
    }

    pub fn agent_not_found(agent: &str, team: &str) -> Self {
        Self::new(
            AtmErrorCode::AgentNotFound,
            format!("agent '{agent}' was not found in team '{team}'"),
        )
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::MessageValidationFailed, message)
    }

    pub fn self_addressed_send_invalid(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::SelfAddressedSendInvalid, message)
    }

    pub fn empty_nudge_template_body() -> Self {
        Self::new(
            AtmErrorCode::EmptyNudgeTemplateBody,
            "built-in nudge template body must be non-empty",
        )
    }

    pub fn caller_context_request_invalid(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::CallerContextRequestInvalid, message)
    }

    pub fn missing_document(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::ConfigTeamMissing, message)
    }

    pub fn file_policy(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::FilePolicyRejected, message)
    }

    pub fn mailbox_read(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::MailboxReadFailed, message)
    }

    pub fn mailbox_lock(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::MailboxLockFailed, message)
    }

    pub fn mailbox_lock_read_only_filesystem(
        operation: impl fmt::Display,
        path: &std::path::Path,
    ) -> Self {
        Self::new(
            AtmErrorCode::MailboxLockReadOnlyFilesystem,
            format!(
                "mailbox lock {operation} failed for {}: filesystem is read-only",
                path.display()
            ),
        )
    }

    pub fn mailbox_lock_timeout(path: &std::path::Path) -> Self {
        Self::new(
            AtmErrorCode::MailboxLockTimeout,
            format!("timed out waiting for mailbox lock on {}", path.display()),
        )
    }

    pub fn mailbox_write(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::MailboxWriteFailed, message)
    }

    pub fn observability_emit(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::ObservabilityEmitFailed, message)
    }

    pub fn observability_bootstrap(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::ObservabilityBootstrapFailed, message)
    }

    pub fn observability_query(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::ObservabilityQueryFailed, message)
    }

    pub fn observability_follow(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::ObservabilityFollowFailed, message)
    }

    pub fn observability_health(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::ObservabilityHealthFailed, message)
    }
}

impl fmt::Display for AtmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AtmError {}

impl From<serde_json::Error> for AtmError {
    fn from(source: serde_json::Error) -> Self {
        Self::new(
            AtmErrorCode::SerializationFailed,
            format!("json error: {source}"),
        )
    }
}

impl From<toml::de::Error> for AtmError {
    fn from(source: toml::de::Error) -> Self {
        Self::new(
            AtmErrorCode::ConfigParseFailed,
            format!("toml error: {source}"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{AtmError, AtmErrorCode};

    #[test]
    fn serializes_only_the_error_contract() {
        let error = AtmError::new(AtmErrorCode::MemberNotFound, "member missing");

        assert_eq!(
            serde_json::to_string(&error).expect("serialize error"),
            r#"{"code":"ATM_MEMBER_NOT_FOUND","message":"member missing\n  Recovery: Confirm the target team and member before retrying."}"#
        );
    }

    #[test]
    fn member_not_found_preserves_its_stable_code() {
        let error = AtmError::member_not_found("test-agent", "test-team");

        assert_eq!(error.code(), AtmErrorCode::MemberNotFound);
    }
}
