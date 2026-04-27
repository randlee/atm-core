use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use crate::types::{AgentName, TeamName};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AtmConfig {
    /// Deprecated compatibility-only field for legacy `.atm.toml` parsing.
    ///
    /// # Deprecated
    ///
    /// ATM no longer uses config identity as a runtime fallback. Callers must
    /// use `ATM_IDENTITY` or an explicit sender override instead. `atm doctor`
    /// surfaces `ATM_WARNING_IDENTITY_DRIFT` when this obsolete field is still
    /// present. Migration path: remove `[atm].identity` from `.atm.toml` and
    /// inject `ATM_IDENTITY` in the active agent environment.
    pub identity: Option<String>,
    pub default_team: Option<TeamName>,
    pub team_members: Vec<String>,
    pub aliases: BTreeMap<String, String>,
    pub post_send_hooks: Vec<PostSendHookRule>,
    pub config_root: PathBuf,
    pub(crate) obsolete_identity_present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookRecipient {
    Wildcard,
    Named(AgentName),
}

impl HookRecipient {
    pub fn matches(&self, candidate: &AgentName) -> bool {
        matches!(self, Self::Wildcard) || matches!(self, Self::Named(name) if name == candidate)
    }
}

impl fmt::Display for HookRecipient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wildcard => f.write_str("*"),
            Self::Named(name) => name.fmt(f),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostSendHookRule {
    pub recipient: HookRecipient,
    pub command: Vec<String>,
}
