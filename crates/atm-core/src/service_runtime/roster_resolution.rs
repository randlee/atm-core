use super::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::boundary::RosterEntry;
use crate::types::{AgentName, TeamName};

pub(super) fn resolve_retained_roster_member_at_ingress<T: RetainedServiceRuntime + ?Sized>(
    runtime: &T,
    addressed_team: &TeamName,
    candidate: &AgentName,
    allow_database_wide_alias: bool,
) -> Option<(TeamName, AgentName)> {
    let addressed_roster = runtime.load_team_roster(addressed_team);
    let all_rosters = if allow_database_wide_alias {
        runtime
            .list_roster_teams()
            .into_iter()
            .flat_map(|team| runtime.load_team_roster(&team))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let (team, canonical) = crate::caller_context::resolve_roster_alias_with_owner(
        candidate,
        addressed_team,
        &addressed_roster,
        &all_rosters,
        allow_database_wide_alias,
    );
    runtime
        .load_roster_member(&team, &canonical)
        .map(|_| (team, canonical))
}

impl LocalServiceRuntime {
    /// Reads one roster member from the RAM roster mirror. Never issues a
    /// durable roster read. Infallible: the RAM mirror is always populated
    /// (construction fails closed on a hydration error), so there is no
    /// error case left to report.
    pub fn load_roster_member(&self, team: &TeamName, agent: &AgentName) -> Option<RosterEntry> {
        self.roster_runtime.load_roster_member(team, agent)
    }

    /// Reads one team's roster from the RAM roster mirror. Never issues a
    /// durable roster read. Infallible for the same reason as
    /// [`Self::load_roster_member`].
    pub fn load_team_roster(&self, team: &TeamName) -> Vec<RosterEntry> {
        self.roster_runtime.load_team_roster(team)
    }

    /// Enumerates every team the RAM roster mirror currently holds. Never
    /// issues a durable roster read.
    pub fn list_roster_teams(&self) -> Vec<TeamName> {
        self.roster_runtime.list_teams()
    }

    /// Validates one ingress member token against the daemon's immutable RAM
    /// roster and returns its owning team plus canonical name. A bare alias
    /// may select its owner globally; an explicit team remains local.
    #[must_use]
    pub fn resolve_roster_member_at_ingress(
        &self,
        addressed_team: &TeamName,
        candidate: &AgentName,
        allow_database_wide_alias: bool,
    ) -> Option<(TeamName, AgentName)> {
        resolve_retained_roster_member_at_ingress(
            self,
            addressed_team,
            candidate,
            allow_database_wide_alias,
        )
    }
}
