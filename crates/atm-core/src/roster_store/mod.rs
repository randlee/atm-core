use crate::store::{
    HostName, InsertOutcome, ProcessId, RecipientPaneId, StoreBoundary, StoreError,
};
use crate::types::{AgentName, TeamName};

/// Durable roster member row keyed by `(team_name, agent_name)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterMemberRecord {
    pub team_name: TeamName,
    pub agent_name: AgentName,
    pub role: String,
    pub transport_kind: String,
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
    ) -> Result<InsertOutcome<RosterMemberRecord>, StoreError>;

    fn load_roster(&self, team_name: &TeamName) -> Result<Vec<RosterMemberRecord>, StoreError>;

    fn update_member_pid(
        &self,
        team_name: &TeamName,
        agent_name: &AgentName,
        update: PidUpdate,
    ) -> Result<Option<RosterMemberRecord>, StoreError>;
}
