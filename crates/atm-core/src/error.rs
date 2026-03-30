use std::error::Error as StdError;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtmErrorKind {
    Config,
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
    ObservabilityQuery,
    ObservabilityHealth,
}

#[derive(Debug)]
pub struct AtmError {
    pub kind: AtmErrorKind,
    pub message: String,
    pub recovery: Option<String>,
    pub source: Option<Box<dyn StdError + Send + Sync>>,
}

impl AtmError {
    pub fn new(kind: AtmErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            recovery: None,
            source: None,
        }
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
        Self::new(AtmErrorKind::Config, "home directory is unavailable")
            .with_recovery("set ATM_HOME or ensure HOME/USERPROFILE is available")
    }

    pub fn address_parse(message: impl Into<String>) -> Self {
        Self::new(AtmErrorKind::Address, message)
            .with_recovery("use agent or agent@team with non-empty segments")
    }

    pub fn identity_unavailable() -> Self {
        Self::new(AtmErrorKind::Identity, "identity is not configured")
            .with_recovery("set ATM_IDENTITY or configure identity in .atm.toml")
    }

    pub fn team_unavailable() -> Self {
        Self::new(AtmErrorKind::TeamNotFound, "team is not configured")
            .with_recovery("pass --team, set ATM_TEAM, or configure default_team in .atm.toml")
    }
}

impl fmt::Display for AtmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl StdError for AtmError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn StdError + 'static))
    }
}

impl From<std::io::Error> for AtmError {
    fn from(error: std::io::Error) -> Self {
        AtmError::new(AtmErrorKind::MailboxWrite, format!("io error: {error}")).with_source(error)
    }
}

impl From<serde_json::Error> for AtmError {
    fn from(error: serde_json::Error) -> Self {
        AtmError::new(AtmErrorKind::Serialization, format!("json error: {error}"))
            .with_source(error)
    }
}

impl From<toml::de::Error> for AtmError {
    fn from(error: toml::de::Error) -> Self {
        AtmError::new(AtmErrorKind::Config, format!("toml error: {error}")).with_source(error)
    }
}
