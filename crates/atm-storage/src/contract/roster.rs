use crate::error::AtmError;
use crate::types::{AgentName, IsoTimestamp, ModelName, PaneId, TeamName};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RosterMemberKind {
    Permanent,
    Ephemeral,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RosterHarness {
    ClaudeCode,
    CodexCli,
    GeminiCli,
    Opencode,
    Hermes,
    PythonGraft,
}

impl RosterHarness {
    /// Return the stable CLI/JSON spelling for this roster harness.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::CodexCli => "codex-cli",
            Self::GeminiCli => "gemini-cli",
            Self::Opencode => "opencode",
            Self::Hermes => "hermes",
            Self::PythonGraft => "python-graft",
        }
    }
}

impl fmt::Display for RosterHarness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentType {
    GeneralPurpose,
    Plan,
    Lead,
    Qa,
    Worker,
    Unknown(String),
}

impl Default for AgentType {
    fn default() -> Self {
        Self::Unknown(String::new())
    }
}

impl From<String> for AgentType {
    fn from(value: String) -> Self {
        match value.as_str() {
            "general-purpose" => Self::GeneralPurpose,
            "plan" => Self::Plan,
            "lead" => Self::Lead,
            "qa" => Self::Qa,
            "worker" => Self::Worker,
            _ => Self::Unknown(value),
        }
    }
}

impl AgentType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::GeneralPurpose => "general-purpose",
            Self::Plan => "plan",
            Self::Lead => "lead",
            Self::Qa => "qa",
            Self::Worker => "worker",
            Self::Unknown(value) => value,
        }
    }
}

impl From<AgentType> for String {
    fn from(value: AgentType) -> Self {
        value.as_str().to_string()
    }
}

impl Serialize for AgentType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl fmt::Display for AgentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AgentType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self::from(String::deserialize(deserializer)?))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterMember {
    pub team_name: TeamName,
    pub agent_name: AgentName,
    pub member_kind: RosterMemberKind,
    pub harness: RosterHarness,
    #[serde(default)]
    pub agent_type: AgentType,
    #[serde(default)]
    pub model: ModelName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipient_pane_id: Option<PaneId>,
    #[serde(default)]
    pub metadata_json: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterSnapshot {
    pub team_name: TeamName,
    pub members: Vec<RosterMember>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refreshed_at: Option<IsoTimestamp>,
}

/// One durable roster identity in the database-wide Herdr namespace.
///
/// `unique_name` is the trimmed roster alias when present, otherwise the
/// canonical member name.  It deliberately remains a `String`: old databases
/// must remain readable even when a legacy alias needs remediation on the
/// next roster write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterUniqueName {
    pub team_name: TeamName,
    pub agent_name: AgentName,
    pub unique_name: String,
}

impl RosterUniqueName {
    #[must_use]
    pub fn from_member(member: &RosterMember) -> Self {
        let unique_name = member
            .metadata_json
            .get("alias")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|alias| !alias.is_empty())
            .unwrap_or(member.agent_name.as_str())
            .to_owned();
        Self {
            team_name: member.team_name.clone(),
            agent_name: member.agent_name.clone(),
            unique_name,
        }
    }
}

/// Returns every member in a collision group, ordered by effective name.
///
/// Keeping this scan in the shared storage contract makes preflight,
/// transactional enforcement, and doctor diagnostics agree on the exact
/// `alias ?? canonical` rule while remaining usable by non-SQLite stores.
#[must_use]
pub fn roster_unique_name_collisions(names: &[RosterUniqueName]) -> Vec<RosterUniqueName> {
    let mut sorted = names.to_vec();
    sorted.sort_by(|left, right| {
        left.unique_name
            .cmp(&right.unique_name)
            .then_with(|| left.team_name.cmp(&right.team_name))
            .then_with(|| left.agent_name.cmp(&right.agent_name))
    });

    let mut collisions = Vec::new();
    let mut start = 0;
    while start < sorted.len() {
        let end = sorted[start + 1..]
            .iter()
            .position(|entry| entry.unique_name != sorted[start].unique_name)
            .map_or(sorted.len(), |offset| start + offset + 1);
        if end - start > 1 {
            collisions.extend_from_slice(&sorted[start..end]);
        }
        start = end;
    }
    collisions
}

/// Returns proposed effective names that differ from the persisted roster.
///
/// A write is allowed to preserve legacy collisions among untouched rows. The
/// returned delta is therefore the only part of a proposed roster whose
/// collisions can reject the write.
#[must_use]
pub fn roster_write_delta(
    persisted: &[RosterUniqueName],
    proposed: &[RosterUniqueName],
) -> Vec<RosterUniqueName> {
    proposed
        .iter()
        .filter(|proposed| {
            persisted
                .iter()
                .find(|persisted| {
                    persisted.team_name == proposed.team_name
                        && persisted.agent_name == proposed.agent_name
                })
                .is_none_or(|persisted| persisted.unique_name != proposed.unique_name)
        })
        .cloned()
        .collect()
}

/// Narrows a database-wide collision set to the groups `team` participates in.
///
/// A roster write is scoped to one team, so it is answerable only for the
/// collisions its own proposed roster is part of.  Collisions between two
/// other teams are pre-existing data the write neither creates nor can
/// repair; rejecting the write for them makes every roster write in the
/// database fail once any duplicate exists anywhere, with no CLI path back
/// out.  The whole colliding group is retained so the diagnostic still names
/// the other team the caller has to avoid.
#[must_use]
pub fn team_scoped_roster_unique_name_collisions(
    collisions: &[RosterUniqueName],
    team: &TeamName,
) -> Vec<RosterUniqueName> {
    let scoped = collisions
        .iter()
        .filter(|collision| collision.team_name == *team)
        .map(|collision| collision.unique_name.as_str())
        .collect::<BTreeSet<_>>();
    collisions
        .iter()
        .filter(|collision| scoped.contains(collision.unique_name.as_str()))
        .cloned()
        .collect()
}

/// Builds the single operator-facing diagnostic for durable roster identity
/// collisions.  Both preflight and transaction enforcement use this wording
/// so a race cannot change the error contract seen by CLI or HTTP callers.
#[must_use]
pub fn roster_unique_name_collision_error(collisions: &[RosterUniqueName]) -> AtmError {
    let mut by_name = BTreeMap::<&str, Vec<(&TeamName, &AgentName)>>::new();
    for collision in collisions {
        by_name
            .entry(&collision.unique_name)
            .or_default()
            .push((&collision.team_name, &collision.agent_name));
    }
    let details = by_name
        .into_iter()
        .map(|(unique_name, owners)| {
            let owners = owners
                .into_iter()
                .map(|(team, member)| format!("({team}, {member})"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("`{unique_name}`: {owners}")
        })
        .collect::<Vec<_>>()
        .join("; ");
    AtmError::validation(format!(
        "roster unique-name collision(s): {details}; choose a distinct --alias for one conflicting member"
    ))
}
