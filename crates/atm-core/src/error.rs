use std::backtrace::Backtrace;
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
        .with_recovery(
            "Set ATM_IDENTITY, configure identity in .atm.toml, or pass --from once that flag is available.",
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
        Self::new(AtmErrorKind::Validation, message)
    }

    pub fn missing_document(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::MissingDocument, message)
    }

    pub fn file_policy(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::FilePolicy, message)
    }

    pub fn mailbox_read(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::MailboxRead, message)
    }

    pub fn mailbox_write(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::MailboxWrite, message)
    }

    pub fn observability_emit(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::ObservabilityEmit, message)
    }

    pub fn observability_bootstrap(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::ObservabilityBootstrap, message)
    }

    pub fn observability_query(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::ObservabilityQuery, message)
    }

    pub fn observability_follow(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::ObservabilityFollow, message)
    }

    pub fn observability_health(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::ObservabilityHealth, message)
    }
}

impl fmt::Display for AtmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(recovery) = &self.recovery {
            write!(f, " Recovery: {recovery}")?;
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
    use super::{AtmError, AtmErrorCode};

    #[test]
    fn observability_error_helpers_use_expected_codes() {
        assert_eq!(
            AtmError::observability_emit("emit").code,
            AtmErrorCode::ObservabilityEmitFailed
        );
        assert_eq!(
            AtmError::observability_query("query").code,
            AtmErrorCode::ObservabilityQueryFailed
        );
        assert_eq!(
            AtmError::observability_follow("follow").code,
            AtmErrorCode::ObservabilityFollowFailed
        );
        assert_eq!(
            AtmError::observability_health("health").code,
            AtmErrorCode::ObservabilityHealthFailed
        );
        assert_eq!(
            AtmError::observability_bootstrap("bootstrap").code,
            AtmErrorCode::ObservabilityBootstrapFailed
        );
    }
}
