use std::backtrace::{Backtrace, BacktraceStatus};
use std::error::Error as StdError;
use std::fmt;

pub use crate::error_codes::AtmErrorCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AtmErrorKind {
    Config,
    MissingDocument,
    Address,
    Identity,
    TeamNotFound,
    AgentNotFound,
    MailboxLock,
    MailboxRead,
    MailboxWrite,
    FilePolicy,
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
    pub(crate) kind: AtmErrorKind,
    pub message: String,
    pub recovery: Option<String>,
    pub source: Option<Box<dyn StdError + Send + Sync>>,
    pub backtrace: Backtrace,
}

impl AtmError {
    pub(crate) fn new(kind: AtmErrorKind, message: impl Into<String>) -> Self {
        Self::new_with_code(kind.default_code(), kind, message)
    }

    pub(crate) fn new_with_code(
        code: AtmErrorCode,
        kind: AtmErrorKind,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            kind,
            message: message.into(),
            recovery: None,
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
        self.recovery = Some(recovery.into());
        self
    }

    pub fn with_source<E>(mut self, source: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        self.source = Some(Box::new(source));
        self
    }

    /// Return the captured backtrace when one is available.
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

    pub fn address_parse(message: impl Into<String>) -> Self {
        Self::new(
            AtmErrorKind::Address,
            format!("address parse failed: {}", message.into()),
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

    pub fn team_unavailable() -> Self {
        Self::new_with_code(
            AtmErrorCode::TeamUnavailable,
            AtmErrorKind::TeamNotFound,
            "team is not configured",
        )
        .with_recovery("Pass an explicit team in the address or configure a default team.")
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
            format!(
                "timed out waiting for mailbox lock on {}",
                path.display()
            ),
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
        if let Some(recovery) = &self.recovery {
            write!(f, "\n  Recovery: {recovery}")?;
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
        Self::new(AtmErrorKind::Serialization, format!("json error: {source}")).with_source(source)
    }
}

impl From<toml::de::Error> for AtmError {
    fn from(source: toml::de::Error) -> Self {
        Self::new(AtmErrorKind::Config, format!("toml error: {source}")).with_source(source)
    }
}

impl AtmErrorKind {
    const fn default_code(self) -> AtmErrorCode {
        match self {
            Self::Config => AtmErrorCode::ConfigParseFailed,
            Self::MissingDocument => AtmErrorCode::ConfigTeamMissing,
            Self::Address => AtmErrorCode::AddressParseFailed,
            Self::Identity => AtmErrorCode::IdentityUnavailable,
            Self::TeamNotFound => AtmErrorCode::TeamNotFound,
            Self::AgentNotFound => AtmErrorCode::AgentNotFound,
            Self::MailboxLock => AtmErrorCode::MailboxLockFailed,
            Self::MailboxRead => AtmErrorCode::MailboxReadFailed,
            Self::MailboxWrite => AtmErrorCode::MailboxWriteFailed,
            Self::FilePolicy => AtmErrorCode::FilePolicyRejected,
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
    use std::backtrace::Backtrace;

    use super::{AtmError, AtmErrorCode};

    #[test]
    fn observability_error_helpers_use_expected_codes() {
        assert_eq!(
            AtmError::observability_emit("emit failed").code,
            AtmErrorCode::ObservabilityEmitFailed
        );
        assert_eq!(
            AtmError::observability_bootstrap("bootstrap failed").code,
            AtmErrorCode::ObservabilityBootstrapFailed
        );
        assert_eq!(
            AtmError::observability_query("query failed").code,
            AtmErrorCode::ObservabilityQueryFailed
        );
        assert_eq!(
            AtmError::observability_follow("follow failed").code,
            AtmErrorCode::ObservabilityFollowFailed
        );
        assert_eq!(
            AtmError::observability_health("health failed").code,
            AtmErrorCode::ObservabilityHealthFailed
        );
    }

    #[test]
    fn mailbox_write_helper_includes_recovery_guidance() {
        let error = AtmError::mailbox_write("write failed");

        assert!(error.is_mailbox_write());
        assert!(
            error
                .recovery
                .as_deref()
                .is_some_and(|value| value.contains("writable"))
        );
    }

    #[test]
    fn display_remains_concise_when_backtrace_is_captured() {
        let mut error = AtmError::validation("boom");
        error.backtrace = Backtrace::force_capture();

        let rendered = error.to_string();
        assert!(rendered.contains("boom"));
        assert!(!rendered.contains("Backtrace:"));
        assert!(error.backtrace().is_some());
    }

    #[test]
    fn display_handles_absent_backtrace() {
        let mut error = AtmError::validation("boom");
        error.backtrace = Backtrace::disabled();

        let rendered = error.to_string();
        assert!(rendered.contains("boom"));
        assert!(!rendered.contains("Backtrace:"));
        assert!(error.backtrace().is_none());
    }
}
