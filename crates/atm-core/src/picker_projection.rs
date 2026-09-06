//! Picker member projection (`atm teams --json --members`), PRD §4.2/§5a.
//!
//! This is the read side of ADR-055 decision (e): it projects the roster's
//! registered `host` metadata and each member's live [`RuntimeMemberState`]
//! into the flat per-member shape a picker UI or script consumes, and which
//! `--from-json` (`crate::send_to::PickerOutput`) expects its `recipients`
//! array to name (`id` is the exact `agent@team` shape
//! [`crate::send_to::resolve_picker_recipient`] parses).

use std::collections::BTreeMap;

use serde::Serialize;

use crate::protocol::RuntimeMemberState;
use crate::send_to::PICKER_OUTPUT_SCHEMA_VERSION;
use crate::team_admin::MemberSummary;
use crate::types::{AgentName, HostName, TeamName};

/// The picker projection's own schema version. Shares
/// [`PICKER_OUTPUT_SCHEMA_VERSION`]'s value: the projection this command
/// emits and the `PickerOutput` document `--from-json` consumes are the two
/// halves of one round trip, and must be versioned together.
pub const PICKER_MEMBERS_SCHEMA_VERSION: u64 = PICKER_OUTPUT_SCHEMA_VERSION;

/// One member entry in a picker projection (PRD §4.2).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PickerMember {
    /// The `agent@team` shape `resolve_picker_recipient` and `--from-json`
    /// `recipients` entries use.
    pub id: String,
    pub name: AgentName,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<HostName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    pub status: PickerMemberStatus,
}

/// A picker-consumable member liveness projection (PRD §4.2's normative
/// mapping): `Active` -> `active`, `Idle` -> `idle`, every other
/// [`RuntimeMemberState`] (`Offline`, `Unknown`, `IdentityConflict`, or no
/// observation at all) -> `dead`.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PickerMemberStatus {
    Active,
    Idle,
    Dead,
}

impl From<RuntimeMemberState> for PickerMemberStatus {
    fn from(state: RuntimeMemberState) -> Self {
        match state {
            RuntimeMemberState::Active => Self::Active,
            RuntimeMemberState::Idle => Self::Idle,
            RuntimeMemberState::Offline
            | RuntimeMemberState::Unknown
            | RuntimeMemberState::IdentityConflict
            | RuntimeMemberState::Blocked => Self::Dead,
        }
    }
}

/// The full `atm teams --json --members` document.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PickerMembersProjection {
    pub schema_version: u64,
    pub team: TeamName,
    pub members: Vec<PickerMember>,
}

/// Builds the picker projection from an already-loaded roster summary and an
/// optional map of live runtime observations.
///
/// A member absent from `runtime_states` (no observation yet, e.g. right
/// after daemon startup) projects as `dead`, the same as an explicit
/// `Offline`/`Unknown`/`IdentityConflict` observation -- never guessed as
/// `active`/`idle`.
#[must_use]
pub fn build_picker_members_projection(
    team: &TeamName,
    roster_members: &[MemberSummary],
    runtime_states: &BTreeMap<AgentName, RuntimeMemberState>,
) -> PickerMembersProjection {
    let members = roster_members
        .iter()
        .map(|member| {
            let state = runtime_states
                .get(&member.name)
                .copied()
                .unwrap_or(RuntimeMemberState::Unknown);
            PickerMember {
                id: format!("{}@{team}", member.name),
                name: member.name.clone(),
                host: member.host.clone(),
                cwd: member.live_cwd.clone(),
                status: PickerMemberStatus::from(state),
            }
        })
        .collect();
    PickerMembersProjection {
        schema_version: PICKER_MEMBERS_SCHEMA_VERSION,
        team: team.clone(),
        members,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boundary::RosterHarness;
    use crate::schema::HomeDirPath;

    fn member(name: &str, host: Option<&str>, cwd: Option<&str>) -> MemberSummary {
        MemberSummary {
            name: name.parse().expect("valid agent name"),
            agent_id: format!("{name}@test-team"),
            agent_type: "worker".to_string(),
            harness: RosterHarness::ClaudeCode,
            model: Default::default(),
            joined_at: None,
            tmux_pane_id: None,
            backend: None,
            herdr_session: None,
            local_backend: None,
            home_dir: HomeDirPath::from(std::path::PathBuf::from("/home/worker")),
            live_cwd: cwd.map(str::to_string),
            host: host.map(|value| value.parse().expect("valid host")),
            extra: serde_json::Map::new(),
        }
    }

    fn team() -> TeamName {
        "test-team".parse().expect("team")
    }

    #[test]
    fn projects_id_host_cwd_and_active_status() {
        let mut states = BTreeMap::new();
        states.insert(
            "sender-a".parse().expect("agent"),
            RuntimeMemberState::Active,
        );
        let projection = build_picker_members_projection(
            &team(),
            &[member("sender-a", Some("rand-m5.local"), Some("/repo"))],
            &states,
        );

        assert_eq!(projection.schema_version, PICKER_MEMBERS_SCHEMA_VERSION);
        assert_eq!(projection.team.as_str(), "test-team");
        assert_eq!(projection.members.len(), 1);
        let picked = &projection.members[0];
        assert_eq!(picked.id, "sender-a@test-team");
        assert_eq!(
            picked.host.as_ref().map(|host| host.as_str()),
            Some("rand-m5.local")
        );
        assert_eq!(picked.cwd.as_deref(), Some("/repo"));
        assert_eq!(picked.status, PickerMemberStatus::Active);
    }

    #[test]
    fn maps_idle_to_idle_and_offline_unknown_identity_conflict_to_dead() {
        for (state, expected) in [
            (RuntimeMemberState::Idle, PickerMemberStatus::Idle),
            (RuntimeMemberState::Offline, PickerMemberStatus::Dead),
            (RuntimeMemberState::Unknown, PickerMemberStatus::Dead),
            (
                RuntimeMemberState::IdentityConflict,
                PickerMemberStatus::Dead,
            ),
        ] {
            assert_eq!(PickerMemberStatus::from(state), expected, "{state:?}");
        }
    }

    #[test]
    fn a_member_with_no_runtime_observation_projects_as_dead() {
        let projection = build_picker_members_projection(
            &team(),
            &[member("sender-a", None, None)],
            &BTreeMap::new(),
        );
        assert_eq!(projection.members[0].status, PickerMemberStatus::Dead);
    }

    #[test]
    fn a_member_without_a_registered_host_projects_host_as_none() {
        let projection = build_picker_members_projection(
            &team(),
            &[member("sender-a", None, None)],
            &BTreeMap::new(),
        );
        assert_eq!(projection.members[0].host, None);
    }

    #[test]
    fn serializes_status_as_lowercase() {
        let json = serde_json::to_value(PickerMemberStatus::Active).expect("serializes");
        assert_eq!(json, serde_json::json!("active"));
    }
}
