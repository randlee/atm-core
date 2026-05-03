use crate::store::{HostName, ProcessId, RecipientPaneId, StoreBoundary, StoreError};
use crate::types::{AgentName, TeamName};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

/// Phase Q keeps roster role strings flexible because the durable roster must
/// mirror current team config/provider values without introducing a second
/// translation table before daemon/runtime migration is complete.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RosterRole(String);

impl RosterRole {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for RosterRole {
    type Err = StoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(StoreError::query(
                "invalid store data for roster role: value must not be blank",
            ));
        }
        Ok(Self(trimmed.to_string()))
    }
}

impl Deref for RosterRole {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for RosterRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Durable transport selector for a roster member.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TransportKind(String);

impl TransportKind {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for TransportKind {
    type Err = StoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(StoreError::query(
                "invalid store data for transport kind: value must not be blank",
            ));
        }
        Ok(Self(trimmed.to_string()))
    }
}

impl Deref for TransportKind {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for TransportKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Durable roster member row keyed by `(team_name, agent_name)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterMemberRecord {
    pub team_name: TeamName,
    pub agent_name: AgentName,
    pub role: RosterRole,
    pub transport_kind: TransportKind,
    pub host_name: HostName,
    pub recipient_pane_id: Option<RecipientPaneId>,
    pub pid: Option<ProcessId>,
    pub metadata_json: Option<String>,
}

/// Explicit PID transition request. Implementations must reject normal identity
/// replacement when the prior PID is still alive; takeover remains an
/// admin-only path outside this boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PidUpdate {
    pub pid: ProcessId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterStoreHealth {
    pub team_roster_ready: bool,
}

/// Durable roster-store boundary.
pub trait RosterStore: StoreBoundary {
    fn replace_roster(
        &self,
        team_name: &TeamName,
        members: &[RosterMemberRecord],
    ) -> Result<(), StoreError>;

    fn upsert_roster_member(
        &self,
        member: &RosterMemberRecord,
    ) -> Result<RosterMemberRecord, StoreError>;

    fn load_roster(&self, team_name: &TeamName) -> Result<Vec<RosterMemberRecord>, StoreError>;

    fn update_member_pid(
        &self,
        team_name: &TeamName,
        agent_name: &AgentName,
        update: PidUpdate,
    ) -> Result<Option<RosterMemberRecord>, StoreError>;

    fn roster_health(&self) -> Result<RosterStoreHealth, StoreError>;
}
