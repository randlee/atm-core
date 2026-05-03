use std::error::Error as StdError;
use std::fmt;
use std::ops::Deref;
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};

use crate::error::AtmErrorCode;
use crate::schema::{AtmMessageId, LegacyMessageId};
use crate::types::{AgentName, TeamName};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageKey(String);

impl MessageKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn source_kind(&self) -> MessageKeySource {
        match self.0.split_once(':').map(|(prefix, _)| prefix) {
            Some("atm") => MessageKeySource::Atm,
            Some("legacy") => MessageKeySource::Legacy,
            _ => MessageKeySource::External,
        }
    }
}

impl MessageKey {
    pub fn from_atm_message_id(message_id: AtmMessageId) -> Self {
        Self(format!("atm:{message_id}"))
    }

    pub fn from_legacy_message_id(message_id: LegacyMessageId) -> Self {
        Self(format!("legacy:{message_id}"))
    }

    pub fn from_source_fingerprint(fingerprint: &SourceFingerprint) -> Self {
        Self(format!("ext:{fingerprint}"))
    }
}

impl FromStr for MessageKey {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        let Some((prefix, suffix)) = trimmed.split_once(':') else {
            return Err("message_key must use '<kind>:<value>' format");
        };
        if suffix.trim().is_empty() {
            return Err("message_key suffix must not be blank");
        }
        match prefix {
            "atm" => {
                suffix
                    .parse::<AtmMessageId>()
                    .map_err(|_| "atm message_key suffix must be a valid ULID")?;
            }
            "legacy" => {
                suffix
                    .parse::<LegacyMessageId>()
                    .map_err(|_| "legacy message_key suffix must be a valid UUID")?;
            }
            "ext" => {
                validate_external_token(suffix)
                    .map_err(|_| "ext message_key suffix must be a stable external fingerprint")?;
            }
            _ => return Err("message_key prefix must be one of: atm, legacy, ext"),
        }
        Ok(Self(trimmed.to_string()))
    }
}

impl fmt::Display for MessageKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKeySource {
    Atm,
    Legacy,
    External,
}

impl MessageKeySource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Atm => "atm",
            Self::Legacy => "legacy",
            Self::External => "ext",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SourceFingerprint(String);

impl SourceFingerprint {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for SourceFingerprint {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        validate_external_token(trimmed)
            .map_err(|_| "source_fingerprint must use a stable ASCII token without separators")?;
        Ok(Self(trimmed.to_string()))
    }
}

impl<'de> Deserialize<'de> for SourceFingerprint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for SourceFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Deref for SourceFingerprint {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct HostName(String);

impl HostName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for HostName {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        if trimmed.is_empty() || trimmed.contains('/') || trimmed.contains('\\') {
            return Err("host_name must be a non-empty logical host token");
        }
        Ok(Self(trimmed.to_string()))
    }
}

impl<'de> Deserialize<'de> for HostName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for HostName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct RecipientPaneId(String);

impl RecipientPaneId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for RecipientPaneId {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err("recipient_pane_id must not be blank");
        }
        Ok(Self(trimmed.to_string()))
    }
}

impl<'de> Deserialize<'de> for RecipientPaneId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for RecipientPaneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProcessId(i64);

impl ProcessId {
    pub fn new(value: i64) -> Result<Self, &'static str> {
        if value <= 0 {
            return Err("pid must be positive");
        }
        Ok(Self(value))
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BusyTimeoutMs(u16);

impl BusyTimeoutMs {
    pub const DEFAULT: Self = Self(1500);

    pub fn new(value: u16) -> Result<Self, &'static str> {
        if value == 0 {
            return Err("busy_timeout_ms must be greater than zero");
        }
        Ok(Self(value))
    }

    pub fn get(self) -> u16 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SqliteHandleBudget(u8);

impl SqliteHandleBudget {
    pub const DEFAULT: Self = Self(1);

    pub fn new(value: u8) -> Result<Self, &'static str> {
        if !(1..=4).contains(&value) {
            return Err("sqlite_handle_budget must be in the range 1..=4");
        }
        Ok(Self(value))
    }

    pub fn get(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreBootstrapReport {
    pub database_path: PathBuf,
    pub schema_version: i64,
    pub wal_enabled: bool,
    pub foreign_keys_enabled: bool,
    pub busy_timeout_ms: BusyTimeoutMs,
    pub handle_budget: SqliteHandleBudget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreHealth {
    pub database_path: PathBuf,
    pub ready: bool,
    pub schema_version: i64,
}

/// Shared bootstrap/health surface implemented by every durable store boundary.
pub trait StoreBoundary {
    fn bootstrap_report(&self) -> Result<StoreBootstrapReport, StoreError>;

    fn health(&self) -> Result<StoreHealth, StoreError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertOutcome<T> {
    Inserted(T),
    Duplicate(StoreDuplicateIdentity),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreDuplicateIdentity {
    MessageKey(MessageKey),
    LegacyMessageId(LegacyMessageId),
    AtmMessageId(AtmMessageId),
    TaskId(crate::types::TaskId),
    RosterMember {
        team_name: TeamName,
        agent_name: AgentName,
    },
    IngestFingerprint {
        team_name: TeamName,
        recipient_agent: AgentName,
        source_fingerprint: SourceFingerprint,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreErrorKind {
    Open,
    Bootstrap,
    Migration,
    Query,
    Busy,
    Constraint,
    Transaction,
}

#[derive(Debug)]
pub struct StoreError {
    pub code: AtmErrorCode,
    pub kind: StoreErrorKind,
    pub message: String,
    pub recovery: Option<String>,
    pub source: Option<Box<dyn StdError + Send + Sync>>,
}

impl StoreError {
    pub fn new(code: AtmErrorCode, kind: StoreErrorKind, message: impl Into<String>) -> Self {
        Self {
            code,
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

    pub fn open(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::StoreOpenFailed, StoreErrorKind::Open, message).with_recovery(
            "Verify that .atm-state exists, the database path is writable, and SQLite can create the mail.db file.",
        )
    }

    pub fn bootstrap(message: impl Into<String>) -> Self {
        Self::new(
            AtmErrorCode::StoreBootstrapFailed,
            StoreErrorKind::Bootstrap,
            message,
        )
        .with_recovery(
            "Repair the SQLite bootstrap inputs or local filesystem permissions, then retry store initialization.",
        )
    }

    pub fn migration(message: impl Into<String>) -> Self {
        Self::new(
            AtmErrorCode::StoreMigrationFailed,
            StoreErrorKind::Migration,
            message,
        )
        .with_recovery(
            "Inspect the mail.db schema state and migration logic, then rerun bootstrap once the mismatch is repaired.",
        )
    }

    pub fn query(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::StoreQueryFailed, StoreErrorKind::Query, message).with_recovery(
            "Retry the store query after checking schema bootstrap and verifying the SQLite database is healthy.",
        )
    }

    pub fn busy(message: impl Into<String>) -> Self {
        Self::new(AtmErrorCode::StoreBusy, StoreErrorKind::Busy, message).with_recovery(
            "Retry after the competing SQLite transaction completes or reduce concurrent store mutation pressure.",
        )
    }

    pub fn constraint(message: impl Into<String>) -> Self {
        Self::new(
            AtmErrorCode::StoreConstraintViolation,
            StoreErrorKind::Constraint,
            message,
        )
        .with_recovery(
            "Use a unique message/task/roster identity before retrying the operation or handle the duplicate result explicitly.",
        )
    }

    pub fn transaction(message: impl Into<String>) -> Self {
        Self::new(
            AtmErrorCode::StoreTransactionFailed,
            StoreErrorKind::Transaction,
            message,
        )
        .with_recovery(
            "Retry after checking the prior store mutation result and ensure the transaction inputs remain internally consistent.",
        )
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.recovery {
            Some(recovery) => write!(f, "{} (recovery: {recovery})", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl StdError for StoreError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source
            .as_ref()
            .map(|source| source.as_ref() as &(dyn StdError + 'static))
    }
}

fn validate_external_token(value: &str) -> Result<(), &'static str> {
    if value.is_empty() || value.contains('/') || value.contains('\\') || value.contains(':') {
        return Err("invalid external token");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        BusyTimeoutMs, HostName, MessageKey, MessageKeySource, ProcessId, RecipientPaneId,
        SourceFingerprint, SqliteHandleBudget,
    };
    use crate::schema::{AtmMessageId, LegacyMessageId};

    #[test]
    fn message_key_accepts_atm_legacy_and_external_forms() {
        let atm = MessageKey::from_atm_message_id(AtmMessageId::new());
        assert_eq!(atm.source_kind(), MessageKeySource::Atm);
        assert_eq!(atm.to_string().parse::<MessageKey>().expect("atm key"), atm);

        let legacy = MessageKey::from_legacy_message_id(LegacyMessageId::new());
        assert_eq!(legacy.source_kind(), MessageKeySource::Legacy);
        assert_eq!(
            legacy
                .to_string()
                .parse::<MessageKey>()
                .expect("legacy key"),
            legacy
        );

        let fingerprint: SourceFingerprint = "sha256-abc123".parse().expect("fingerprint");
        let external = MessageKey::from_source_fingerprint(&fingerprint);
        assert_eq!(external.source_kind(), MessageKeySource::External);
        assert_eq!(
            external
                .to_string()
                .parse::<MessageKey>()
                .expect("external key"),
            external
        );
    }

    #[test]
    fn wrappers_reject_invalid_values() {
        assert!("bad/name".parse::<SourceFingerprint>().is_err());
        assert!("".parse::<HostName>().is_err());
        assert!("   ".parse::<RecipientPaneId>().is_err());
        assert!(ProcessId::new(0).is_err());
        assert!(BusyTimeoutMs::new(0).is_err());
        assert!(SqliteHandleBudget::new(0).is_err());
        assert!(SqliteHandleBudget::new(5).is_err());
    }
}
