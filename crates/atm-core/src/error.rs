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
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

pub type Error = AtmError;

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
        E: std::error::Error + Send + Sync + 'static,
    {
        self.source = Some(Box::new(source));
        self
    }

    pub fn home_directory_unavailable() -> Self {
        Self::new(AtmErrorKind::Config, "home directory is unavailable")
            .with_recovery("Set ATM_HOME or ensure the OS home directory can be resolved.")
    }

    pub fn address_parse(message: impl Into<String>) -> Self {
        Self::new(
            AtmErrorKind::Address,
            format!("address parse failed: {}", message.into()),
        )
    }

    pub fn identity_unavailable() -> Self {
        Self::new(AtmErrorKind::Identity, "identity is not configured")
            .with_recovery("Set ATM_IDENTITY, configure identity in .atm.toml, or pass --from once that flag is available.")
    }

    pub fn team_unavailable() -> Self {
        Self::new(AtmErrorKind::TeamNotFound, "team is not configured")
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

impl std::error::Error for AtmError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|error| error as &(dyn std::error::Error + 'static))
    }
}

impl From<std::io::Error> for AtmError {
    fn from(source: std::io::Error) -> Self {
        Self::new(AtmErrorKind::MailboxWrite, format!("io error: {source}")).with_source(source)
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
