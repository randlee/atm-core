use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::types::{AgentName, TeamName};

pub const DEFAULT_CLAUDE_JSONL_BODY_EXPORT_MAX_BYTES: u64 = 128 * 1024;
pub const MAX_CLAUDE_JSONL_BODY_EXPORT_MAX_BYTES: u64 = 1_048_576;
pub const MAX_POST_SEND_HOOKS: usize = 64;
pub const MAX_POST_SEND_HOOK_COMMAND_PATH_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ByteCount(u64);

impl ByteCount {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub fn as_usize(self) -> Option<usize> {
        usize::try_from(self.0).ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraftConfig {
    pub enabled: bool,
}

impl Default for GraftConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtmConfig {
    /// Deprecated compatibility-only field for legacy `.atm.toml` parsing.
    ///
    /// # Deprecated
    ///
    /// ATM no longer uses config identity as a runtime fallback. Callers must
    /// use `ATM_IDENTITY` or an explicit sender override instead. `atm doctor`
    /// surfaces `ATM_WARNING_IDENTITY_DRIFT` when an obsolete config identity
    /// field is still present. Migration path: remove `[atm].identity` or the
    /// legacy top-level `identity` key from `.atm.toml` and inject
    /// `ATM_IDENTITY` in the active agent environment. This field
    /// intentionally remains `Option<String>` because ATM preserves the raw
    /// deprecated token only for compatibility reporting, not runtime identity
    /// resolution.
    pub obsolete_identity: Option<String>,
    pub default_team: Option<TeamName>,
    pub team_members: Vec<TeamName>,
    /// Alias destination values are free-form routing strings; no domain constraint is applied at
    /// the config layer, so no newtype wrapper is needed here.
    pub aliases: BTreeMap<String, String>,
    pub post_send_hooks: Vec<PostSendHookRule>,
    pub claude_jsonl_body_export_max_bytes: ByteCount,
    pub graft: GraftConfig,
    pub config_root: PathBuf,
}

impl Default for AtmConfig {
    fn default() -> Self {
        Self {
            obsolete_identity: None,
            default_team: None,
            team_members: Vec::new(),
            aliases: BTreeMap::new(),
            post_send_hooks: Vec::new(),
            claude_jsonl_body_export_max_bytes: ByteCount::new(
                DEFAULT_CLAUDE_JSONL_BODY_EXPORT_MAX_BYTES,
            ),
            graft: GraftConfig::default(),
            config_root: PathBuf::new(),
        }
    }
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

impl Serialize for HookRecipient {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for HookRecipient {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let recipient = String::deserialize(deserializer)?;
        if recipient == "*" {
            Ok(Self::Wildcard)
        } else {
            recipient
                .parse()
                .map(Self::Named)
                .map_err(serde::de::Error::custom)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostSendHookRule {
    pub recipient: HookRecipient,
    pub command: Vec<String>,
}
