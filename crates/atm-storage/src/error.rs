use std::backtrace::{Backtrace, BacktraceStatus};
use std::error::Error as StdError;
use std::fmt;

pub use crate::error_codes::AtmErrorCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtmErrorKind {
    Config,
    MissingDocument,
    Address,
    Identity,
    DaemonUnavailable,
    TeamNotFound,
    AgentNotFound,
    MailboxLock,
    MailboxRead,
    MailboxWrite,
    FilePolicy,
    Internal,
    Validation,
    Serialization,
    Timeout,
    ObservabilityEmit,
    ObservabilityBootstrap,
    ObservabilityQuery,
    ObservabilityFollow,
    ObservabilityHealth,
}

#[derive(Debug)]
pub struct AtmError {
    pub code: AtmErrorCode,
    pub kind: AtmErrorKind,
    pub message: String,
    pub recovery: Vec<String>,
    pub source: Option<Box<dyn StdError + Send + Sync>>,
    pub backtrace: Backtrace,
}

impl AtmError {
    pub fn new(kind: AtmErrorKind, message: impl Into<String>) -> Self {
        Self::new_with_code(kind.default_code(), kind, message)
    }

    pub fn new_with_code(
        code: AtmErrorCode,
        kind: AtmErrorKind,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            kind,
            message: message.into(),
            recovery: Vec::new(),
            source: None,
            backtrace: Backtrace::capture(),
        }
    }

    pub fn is_config(&self) -> bool {
        self.kind == AtmErrorKind::Config
    }
    pub fn is_address(&self) -> bool {
        self.kind == AtmErrorKind::Address
    }
    pub fn is_missing_document(&self) -> bool {
        self.kind == AtmErrorKind::MissingDocument
    }
    pub fn is_identity(&self) -> bool {
        self.kind == AtmErrorKind::Identity
    }
    pub fn is_team_not_found(&self) -> bool {
        self.kind == AtmErrorKind::TeamNotFound
    }
    pub fn is_daemon_unavailable(&self) -> bool {
        self.kind == AtmErrorKind::DaemonUnavailable
    }
    pub fn is_agent_not_found(&self) -> bool {
        self.kind == AtmErrorKind::AgentNotFound
    }
    pub fn is_mailbox_read(&self) -> bool {
        self.kind == AtmErrorKind::MailboxRead
    }
    pub fn is_mailbox_lock(&self) -> bool {
        self.kind == AtmErrorKind::MailboxLock
    }
    pub fn is_mailbox_write(&self) -> bool {
        self.kind == AtmErrorKind::MailboxWrite
    }
    pub fn is_file_policy(&self) -> bool {
        self.kind == AtmErrorKind::FilePolicy
    }
    pub fn is_internal(&self) -> bool {
        self.kind == AtmErrorKind::Internal
    }
    pub fn is_validation(&self) -> bool {
        self.kind == AtmErrorKind::Validation
    }
    pub fn is_serialization(&self) -> bool {
        self.kind == AtmErrorKind::Serialization
    }
    pub fn is_timeout(&self) -> bool {
        self.kind == AtmErrorKind::Timeout
    }
    pub fn is_observability_emit(&self) -> bool {
        self.kind == AtmErrorKind::ObservabilityEmit
    }
    pub fn is_observability_bootstrap(&self) -> bool {
        self.kind == AtmErrorKind::ObservabilityBootstrap
    }
    pub fn is_observability_query(&self) -> bool {
        self.kind == AtmErrorKind::ObservabilityQuery
    }
    pub fn is_observability_follow(&self) -> bool {
        self.kind == AtmErrorKind::ObservabilityFollow
    }
    pub fn is_observability_health(&self) -> bool {
        self.kind == AtmErrorKind::ObservabilityHealth
    }

    pub fn with_recovery(mut self, recovery: impl Into<String>) -> Self {
        self.recovery.push(recovery.into());
        self
    }

    pub fn primary_recovery(&self) -> Option<&str> {
        self.recovery.first().map(String::as_str)
    }

    pub fn with_source<E>(mut self, source: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        self.source = Some(Box::new(source));
        self
    }

    pub fn backtrace(&self) -> Option<&Backtrace> {
        (self.backtrace.status() == BacktraceStatus::Captured).then_some(&self.backtrace)
    }

    pub fn home_directory_unavailable() -> Self {
        Self::new_with_code(
            AtmErrorCode::ConfigHomeUnavailable,
            AtmErrorKind::Config,
            "home directory is unavailable",
        )
        .with_recovery("Set ATM_HOME or ensure the OS home directory can be resolved.")
    }

    pub fn atm_home_unresolved(message: impl Into<String>) -> Self {
        Self::new_with_code(
            AtmErrorCode::AtmHomeUnresolved,
            AtmErrorKind::Config,
            message,
        )
        .with_recovery(
            "Set ATM_HOME or ensure the OS home directory can be resolved before retrying the ATM command.",
        )
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::Config, message).with_recovery(
            "Check the active ATM configuration, runtime wiring, and local path settings before retrying.",
        )
    }

    pub fn address_parse(message: impl Into<String>) -> Self {
        Self::new(
            AtmErrorKind::Address,
            format!("address parse failed: {}", message.into()),
        )
        .with_recovery(
            "Correct the ATM address format and retry with a valid <agent> or <agent>@<team> target.",
        )
    }

    pub fn identity_unavailable() -> Self {
        Self::new_with_code(
            AtmErrorCode::IdentityUnavailable,
            AtmErrorKind::Identity,
            "identity is not configured",
        )
        .with_recovery("Set ATM_IDENTITY or provide an explicit command identity override when the command supports one.")
    }

    pub fn identity_invalid(message: impl Into<String>) -> Self {
        Self::new_with_code(
            AtmErrorCode::IdentityInvalid,
            AtmErrorKind::Identity,
            format!("caller identity is invalid: {}", message.into()),
        )
        .with_recovery(
            "Set ATM_IDENTITY or provide an explicit command identity override using a valid ATM agent name.",
        )
    }

    pub fn identity_conflict(message: impl Into<String>) -> Self {
        Self::new_with_code(
            AtmErrorCode::IdentityConflict,
            AtmErrorKind::Identity,
            message,
        )
        .with_recovery("Stop and report to the user immediately. Resolve the live pid conflict before retrying ATM activity.")
    }

    pub fn member_already_exists(member: &str, team: &str) -> Self {
        Self::new_with_code(
            AtmErrorCode::MemberAlreadyExists,
            AtmErrorKind::Validation,
            format!("member '{member}' already exists in team '{team}'"),
        )
        .with_recovery(
            "Use `atm teams update-member` to repair metadata for an existing member instead of retrying `atm teams add-member`.",
        )
    }

    pub fn member_not_found(member: &str, team: &str) -> Self {
        Self::new_with_code(
            AtmErrorCode::MemberNotFound,
            AtmErrorKind::AgentNotFound,
            format!("member '{member}' was not found in team '{team}'"),
        )
        .with_recovery(
            "Confirm the target team/member pair, create the member with `atm teams add-member` if it is genuinely missing, or retry `atm teams update-member` against an existing member row.",
        )
    }

    pub fn daemon_unavailable(message: impl Into<String>) -> Self {
        Self::new_with_code(
            AtmErrorCode::DaemonUnavailable,
            AtmErrorKind::DaemonUnavailable,
            message,
        )
        .with_recovery(
            "Ensure the atm-daemon binary is installed, the daemon socket path is reachable, and ATM_DAEMON_BIN/ATM_HOME are set correctly before retrying.",
        )
    }

    pub fn runtime_root_invalid(message: impl Into<String>) -> Self {
        Self::new_with_code(
            AtmErrorCode::RuntimeRootInvalid,
            AtmErrorKind::DaemonUnavailable,
            message,
        )
        .with_recovery(
            "Repair ATM_HOME, the derived daemon/socket/database root, or the active working directory before retrying the ATM command.",
        )
    }

    pub fn runtime_bootstrap_refused(message: impl Into<String>) -> Self {
        Self::new_with_code(
            AtmErrorCode::RuntimeBootstrapRefused,
            AtmErrorKind::DaemonUnavailable,
            message,
        )
        .with_recovery(
            "Clear the conflicting daemon runtime override or repair the canonical ATM runtime root before retrying the ATM command.",
        )
    }

    pub fn daemon_may_have_executed(message: impl Into<String>) -> Self {
        Self::new_with_code(
            AtmErrorCode::DaemonMayHaveExecuted,
            AtmErrorKind::DaemonUnavailable,
            message,
        )
        .with_recovery(
            "Check the destination mailbox or other service-side effects before retrying this same-host ATM command.",
        )
    }

    pub fn daemon_lifecycle_wedge(message: impl Into<String>) -> Self {
        Self::new_with_code(
            AtmErrorCode::DaemonLifecycleWedge,
            AtmErrorKind::DaemonUnavailable,
            message,
        )
        .with_recovery(
            "Restart the daemon after the local IPC listener and lifecycle-control state fully stop, then inspect daemon logs for the wedged shutdown path.",
        )
    }

    pub fn daemon_advisory_session_already_registered(message: impl Into<String>) -> Self {
        Self::new_with_code(
            AtmErrorCode::DaemonAdvisorySessionAlreadyRegistered,
            AtmErrorKind::DaemonUnavailable,
            message,
        )
        .with_recovery(
            "Unregister the existing advisory session or choose a fresh session id before retrying embedded advisory activation.",
        )
    }

    pub fn daemon_advisory_session_not_registered(message: impl Into<String>) -> Self {
        Self::new_with_code(
            AtmErrorCode::DaemonAdvisorySessionNotRegistered,
            AtmErrorKind::DaemonUnavailable,
            message,
        )
        .with_recovery(
            "Register the advisory session before fetching or draining daemon-owned advisory state.",
        )
    }

    pub fn daemon_advisory_session_cleanup_failed(message: impl Into<String>) -> Self {
        Self::new_with_code(
            AtmErrorCode::DaemonAdvisorySessionCleanupFailed,
            AtmErrorKind::DaemonUnavailable,
            message,
        )
        .with_recovery(
            "Inspect advisory-session lifecycle logs, clean up any orphaned graft session, and retry only after the daemon unregister path is healthy.",
        )
    }

    pub fn daemon_launch_gate_rejected(message: impl Into<String>) -> Self {
        Self::new_with_code(
            AtmErrorCode::DaemonLaunchGateRejected,
            AtmErrorKind::DaemonUnavailable,
            message,
        )
        .with_recovery(
            "Connect to the existing daemon if it is healthy, or resolve stale ownership before retrying another daemon launch.",
        )
    }

    pub fn daemon_serving_state_rejected(message: impl Into<String>) -> Self {
        Self::new_with_code(
            AtmErrorCode::DaemonServingStateRejected,
            AtmErrorKind::DaemonUnavailable,
            message,
        )
        .with_recovery(
            "Stop launching duplicate daemons and inspect the existing runtime owner before retrying startup.",
        )
    }

    pub fn daemon_stale_owner_recovery_failed(message: impl Into<String>) -> Self {
        Self::new_with_code(
            AtmErrorCode::DaemonStaleOwnerRecoveryFailed,
            AtmErrorKind::DaemonUnavailable,
            message,
        )
        .with_recovery(
            "Inspect the recorded owner, confirm no live daemon remains, repair ownership metadata, then retry startup.",
        )
    }

    pub fn daemon_auto_start_failed(message: impl Into<String>) -> Self {
        Self::new_with_code(
            AtmErrorCode::DaemonAutoStartFailed,
            AtmErrorKind::DaemonUnavailable,
            message,
        )
        .with_recovery(
            "Inspect daemon stderr/logs, fix the startup fault, and retry only after the daemon can reach serving state.",
        )
    }

    pub fn remote_delivery_outcome_unknown(message: impl Into<String>) -> Self {
        Self::new_with_code(
            AtmErrorCode::RemoteDeliveryOutcomeUnknown,
            AtmErrorKind::DaemonUnavailable,
            message,
        )
        .with_recovery(
            "Check the destination daemon or mailbox before retrying. If local durable replay is enabled, let the daemon resume the pending handoff rather than guessing success.",
        )
    }

    pub fn help_topic_not_found(message: impl Into<String>) -> Self {
        Self::new_with_code(
            AtmErrorCode::HelpTopicNotFound,
            AtmErrorKind::Validation,
            message,
        )
        .with_recovery("Use `atm help --list` to inspect available help topics and subcommands.")
    }

    pub fn test_fake_transport_injection_failed(message: impl Into<String>) -> Self {
        Self::new_with_code(
            AtmErrorCode::TestFakeTransportInjectionFailed,
            AtmErrorKind::Validation,
            message,
        )
        .with_recovery(
            "Fix the test seam configuration so it uses a valid FakeClientTransport or LoopbackClientTransport instance.",
        )
    }

    pub fn team_unavailable() -> Self {
        Self::new_with_code(
            AtmErrorCode::TeamUnavailable,
            AtmErrorKind::TeamNotFound,
            "team is not configured",
        )
        .with_recovery("Pass an explicit team in the address or configure a default team.")
    }

    pub fn team_invalid(message: impl Into<String>) -> Self {
        Self::new_with_code(
            AtmErrorCode::TeamInvalid,
            AtmErrorKind::Validation,
            format!("caller team is invalid: {}", message.into()),
        )
        .with_recovery(
            "Set ATM_TEAM or provide an explicit --team override using a valid ATM team name.",
        )
    }

    pub fn team_not_found(team: &str) -> Self {
        Self::new(
            AtmErrorKind::TeamNotFound,
            format!("team '{team}' was not found"),
        )
        .with_recovery("Create the team config or target a different team.")
    }

    pub fn agent_not_found(agent: &str, team: &str) -> Self {
        Self::new(
            AtmErrorKind::AgentNotFound,
            format!("agent '{agent}' was not found in team '{team}'"),
        )
        .with_recovery("Update the team membership or target a different recipient.")
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::Validation, message).with_recovery(
            "Correct the invalid ATM input or mailbox state, then retry the command with a valid target or argument.",
        )
    }

    pub fn self_addressed_send_invalid(message: impl Into<String>) -> Self {
        Self::new_with_code(
            AtmErrorCode::SelfAddressedSendInvalid,
            AtmErrorKind::Validation,
            message,
        )
        .with_recovery(
            "Target a different recipient or use a non-mutating mailbox inspection command instead of sending a message to yourself.",
        )
    }

    pub fn empty_nudge_template_body() -> Self {
        Self::new_with_code(
            AtmErrorCode::EmptyNudgeTemplateBody,
            AtmErrorKind::Validation,
            "built-in nudge template body must be non-empty",
        )
        .with_recovery(
            "Provide a non-empty template body, or use the explicit disable or clear nudge-template command instead of an empty string.",
        )
    }

    pub fn caller_context_request_invalid(message: impl Into<String>) -> Self {
        Self::new_with_code(
            AtmErrorCode::CallerContextRequestInvalid,
            AtmErrorKind::Validation,
            message,
        )
        .with_recovery(
            "Repair the CLI request-builder path so ATM daemon requests always include validated caller_identity and caller_team fields.",
        )
    }

    pub fn missing_document(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::MissingDocument, message).with_recovery(
            "Restore the missing ATM document or recreate it through the documented team-management workflow before retrying.",
        )
    }

    pub fn file_policy(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::FilePolicy, message).with_recovery(
            "Update the referenced file, path, or policy inputs so they satisfy ATM file-policy rules before retrying the command.",
        )
    }

    pub fn mailbox_read(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::MailboxRead, message).with_recovery(
            "Check ATM_HOME, mailbox file permissions, and mailbox JSON syntax before retrying the ATM command.",
        )
    }

    pub fn mailbox_lock(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::MailboxLock, message).with_recovery(
            "Retry after other ATM mailbox activity completes, or wait for the competing process to release its mailbox lock.",
        )
    }

    pub fn mailbox_lock_read_only_filesystem(
        operation: impl fmt::Display,
        path: &std::path::Path,
    ) -> Self {
        Self::new_with_code(
            AtmErrorCode::MailboxLockReadOnlyFilesystem,
            AtmErrorKind::MailboxLock,
            format!(
                "mailbox lock {operation} failed for {}: filesystem is read-only",
                path.display()
            ),
        )
        .with_recovery(
            "Remount the filesystem read-write or point ATM at a writable home with ATM_HOME or --home, then retry the ATM command.",
        )
    }

    pub fn mailbox_lock_timeout(path: &std::path::Path) -> Self {
        Self::new_with_code(
            AtmErrorCode::MailboxLockTimeout,
            AtmErrorKind::MailboxLock,
            format!("timed out waiting for mailbox lock on {}", path.display()),
        )
        .with_recovery(
            "Retry after the competing ATM process finishes, or investigate whether another process is holding the mailbox lock unexpectedly.",
        )
    }

    pub fn mailbox_write(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::MailboxWrite, message).with_recovery(
            "Check that the mailbox/workflow path is writable, has free space, and was not modified concurrently before retrying the ATM command.",
        )
    }

    pub fn observability_emit(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::ObservabilityEmit, message).with_recovery(
            "Verify the observability sink is writable or temporarily disable retained logging while investigating.",
        )
    }

    pub fn observability_bootstrap(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::ObservabilityBootstrap, message).with_recovery(
            "Check the configured observability backend, log directory permissions, and any local path overrides before retrying ATM commands.",
        )
    }

    pub fn observability_query(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::ObservabilityQuery, message).with_recovery(
            "Confirm retained logs exist and the observability backend supports queries for the selected sink and time range.",
        )
    }

    pub fn observability_follow(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::ObservabilityFollow, message).with_recovery(
            "Check that follow/tail is enabled for the active sink and retry with a narrower query if the stream is unavailable.",
        )
    }

    pub fn observability_health(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::ObservabilityHealth, message).with_recovery(
            "Inspect the observability backend health, file sink path, and query backend status, then rerun `atm doctor`.",
        )
    }
}

impl fmt::Display for AtmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        for recovery in &self.recovery {
            write!(f, "\n  Recovery: {recovery}")?;
        }
        if let Some(source) = self.source() {
            write!(f, "\n  Source: {source}")?;
            let mut current = source.source();
            while let Some(next) = current {
                write!(f, "\n  Caused by: {next}")?;
                current = next.source();
            }
        }
        match self.backtrace() {
            Some(backtrace) => write!(f, "\n  Backtrace:\n{backtrace}")?,
            None => write!(f, "\n  Backtrace: {:?}", self.backtrace.status())?,
        }
        Ok(())
    }
}

impl StdError for AtmError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn StdError + 'static))
    }
}

impl From<serde_json::Error> for AtmError {
    fn from(source: serde_json::Error) -> Self {
        Self::new(AtmErrorKind::Serialization, format!("json error: {source}"))
            .with_recovery(
                "Inspect the JSON payload for structural errors and verify the schema matches the expected format.",
            )
            .with_source(source)
    }
}

impl From<toml::de::Error> for AtmError {
    fn from(source: toml::de::Error) -> Self {
        Self::new(AtmErrorKind::Config, format!("toml error: {source}"))
            .with_recovery(
                "Inspect the TOML file for syntax errors and verify all required fields are present.",
            )
            .with_source(source)
    }
}

impl AtmErrorKind {
    const fn default_code(self) -> AtmErrorCode {
        match self {
            Self::Config => AtmErrorCode::ConfigParseFailed,
            Self::MissingDocument => AtmErrorCode::ConfigTeamMissing,
            Self::Address => AtmErrorCode::AddressParseFailed,
            Self::Identity => AtmErrorCode::IdentityUnavailable,
            Self::DaemonUnavailable => AtmErrorCode::DaemonUnavailable,
            Self::TeamNotFound => AtmErrorCode::TeamNotFound,
            Self::AgentNotFound => AtmErrorCode::AgentNotFound,
            Self::MailboxLock => AtmErrorCode::MailboxLockFailed,
            Self::MailboxRead => AtmErrorCode::MailboxReadFailed,
            Self::MailboxWrite => AtmErrorCode::MailboxWriteFailed,
            Self::FilePolicy => AtmErrorCode::FilePolicyRejected,
            Self::Internal => AtmErrorCode::InternalError,
            Self::Validation => AtmErrorCode::MessageValidationFailed,
            Self::Serialization => AtmErrorCode::SerializationFailed,
            Self::Timeout => AtmErrorCode::WaitTimeout,
            Self::ObservabilityEmit => AtmErrorCode::ObservabilityEmitFailed,
            Self::ObservabilityBootstrap => AtmErrorCode::ObservabilityBootstrapFailed,
            Self::ObservabilityQuery => AtmErrorCode::ObservabilityQueryFailed,
            Self::ObservabilityFollow => AtmErrorCode::ObservabilityFollowFailed,
            Self::ObservabilityHealth => AtmErrorCode::ObservabilityHealthFailed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AtmError, AtmErrorCode, AtmErrorKind};
    use std::error::Error;
    use std::fmt;

    #[derive(Debug)]
    struct LeafError(&'static str);

    impl fmt::Display for LeafError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.0)
        }
    }

    impl Error for LeafError {}

    #[derive(Debug)]
    struct ParentError {
        message: &'static str,
        source: LeafError,
    }

    impl fmt::Display for ParentError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.message)
        }
    }

    impl Error for ParentError {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(&self.source)
        }
    }

    #[test]
    fn display_includes_recovery_source_chain_and_backtrace_label() {
        let error = AtmError::new(AtmErrorKind::Validation, "storage contract failed")
            .with_recovery("Repair the contract fixture.")
            .with_source(ParentError {
                message: "parent source",
                source: LeafError("leaf source"),
            });

        let rendered = error.to_string();

        assert!(rendered.contains("storage contract failed"));
        assert!(rendered.contains("Recovery: Repair the contract fixture."));
        assert!(rendered.contains("Source: parent source"));
        assert!(rendered.contains("Caused by: leaf source"));
        assert!(rendered.contains("Backtrace:"));
    }

    #[test]
    fn member_not_found_uses_agent_not_found_kind() {
        let error = AtmError::member_not_found("test-agent", "test-team");

        assert_eq!(error.code, AtmErrorCode::MemberNotFound);
        assert!(error.is_agent_not_found());
    }
}
