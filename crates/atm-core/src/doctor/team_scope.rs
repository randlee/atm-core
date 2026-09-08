use super::{DoctorFinding, DoctorSeverity, push_doctor_error, roster_names};
use crate::boundary::RosterEntry;
use crate::delivery_channel::local_message_received_backend;
use crate::error_codes::AtmErrorCode;
use crate::service_runtime::LocalServiceRuntime;
use crate::team_admin::{MembersList, ordered_roster_member_summaries};
use crate::types::{AgentName, TeamName};
use std::path::Path;

type LoadedRosters = Vec<(TeamName, Vec<RosterEntry>)>;

/// The effective team scope for one doctor run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DoctorTeamScope {
    Single(TeamName),
    AllTeams { resolved_none: bool },
}

impl Default for DoctorTeamScope {
    fn default() -> Self {
        Self::AllTeams {
            resolved_none: false,
        }
    }
}

impl DoctorTeamScope {
    pub(super) fn report_name(&self) -> &'static str {
        match self {
            Self::Single(_) => "single",
            Self::AllTeams { .. } => "all_teams",
        }
    }

    pub fn is_all_teams(&self) -> bool {
        matches!(self, Self::AllTeams { .. })
    }
}

pub(super) fn team_message(team: &TeamName, message: impl AsRef<str>) -> String {
    format!("team {team}: {}", message.as_ref())
}

pub(super) fn teams_for_scope(
    runtime: &LocalServiceRuntime,
    scope: &DoctorTeamScope,
    findings: &mut Vec<DoctorFinding>,
) -> Vec<TeamName> {
    let mut teams = match scope {
        DoctorTeamScope::Single(team) => vec![team.clone()],
        DoctorTeamScope::AllTeams { resolved_none } => {
            if *resolved_none {
                findings.push(DoctorFinding {
                    severity: DoctorSeverity::Info,
                    code: AtmErrorCode::ObservabilityHealthOk,
                    message: "no team resolved from --team or ATM_TEAM; inspecting all canonical roster teams"
                        .to_owned(),
                    remediation: None,
                });
            }
            runtime.list_roster_teams()
        }
    };
    teams.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    teams.dedup();
    teams
}

pub(super) fn load_scoped_rosters(
    runtime: &LocalServiceRuntime,
    teams: &[TeamName],
    scope: &DoctorTeamScope,
    caller_identity: Option<&AgentName>,
    live_cwd: Option<&Path>,
    findings: &mut Vec<DoctorFinding>,
) -> (Option<MembersList>, Vec<MembersList>) {
    let team_context = scope.is_all_teams();
    let mut all_rosters = runtime
        .list_roster_teams()
        .into_iter()
        .map(|team| {
            let roster = runtime.load_team_roster(&team);
            (team, roster)
        })
        .collect::<LoadedRosters>();
    let rosters = teams
        .iter()
        .filter_map(|team| {
            load_member_roster(
                runtime,
                team,
                caller_identity,
                live_cwd,
                team_context,
                &mut all_rosters,
                findings,
            )
        })
        .collect::<Vec<_>>();
    if team_context {
        (None, rosters)
    } else {
        (rosters.into_iter().next(), Vec::new())
    }
}

fn load_member_roster(
    runtime: &LocalServiceRuntime,
    team: &TeamName,
    caller_identity: Option<&AgentName>,
    live_cwd: Option<&Path>,
    team_context: bool,
    all_rosters: &mut LoadedRosters,
    findings: &mut Vec<DoctorFinding>,
) -> Option<MembersList> {
    if let Err(error) = crate::address::validate_path_segment(team.as_str(), "team") {
        push_doctor_error_for_team(
            findings,
            DoctorSeverity::Error,
            error,
            team_context.then_some(team),
        );
        return None;
    }
    let roster = all_rosters
        .iter()
        .find(|(loaded_team, _)| loaded_team == team)
        .map(|(_, roster)| roster.clone())
        .unwrap_or_else(|| {
            let roster = runtime.load_team_roster(team);
            all_rosters.push((team.clone(), roster.clone()));
            roster
        });
    push_mixed_local_backend_warning(team, &roster, findings);
    roster_names::push_duplicate_effective_name_warnings(
        team,
        &roster,
        all_rosters,
        team_context,
        findings,
    );
    let members = ordered_roster_member_summaries(&roster, caller_identity, live_cwd);

    Some(MembersList {
        team: team.clone(),
        members,
    })
}

fn push_mixed_local_backend_warning(
    team: &TeamName,
    roster: &[RosterEntry],
    findings: &mut Vec<DoctorFinding>,
) {
    let mut tmux = Vec::new();
    let mut herdr = Vec::new();
    for member in roster {
        match local_message_received_backend(member) {
            Some(crate::delivery_channel::LocalMessageReceivedBackend::Tmux { .. }) => {
                tmux.push(member.agent_name.to_string())
            }
            Some(crate::delivery_channel::LocalMessageReceivedBackend::Herdr { .. }) => {
                herdr.push(member.agent_name.to_string())
            }
            None => {}
        }
    }
    if tmux.is_empty() || herdr.is_empty() {
        return;
    }
    findings.push(DoctorFinding {
        severity: DoctorSeverity::Warning,
        code: AtmErrorCode::RosterMixedLocalBackend,
        message: format!(
            "team {team} has mixed local backends; tmux members: [{}]; Herdr members: [{}]",
            tmux.join(", "), herdr.join(", ")
        ),
        remediation: Some(format!(
            "Use `atm teams update-member {team} <member> --backend herdr` or `atm teams update-member {team} <member> --backend tmux --target %N` to select the intended backend."
        )),
    });
}

pub(super) fn graft_receivers_for_teams(
    runtime: &LocalServiceRuntime,
    teams: &[TeamName],
    team_context: bool,
    findings: &mut Vec<DoctorFinding>,
) -> super::GraftReceiversDoctorReport {
    let receivers = teams
        .iter()
        .flat_map(|team| {
            super::graft_receivers_doctor_report(runtime, team, team_context, findings).receivers
        })
        .collect();
    super::GraftReceiversDoctorReport { receivers }
}

pub(super) fn push_doctor_error_for_team(
    findings: &mut Vec<DoctorFinding>,
    severity: DoctorSeverity,
    error: crate::error::AtmError,
    team: Option<&TeamName>,
) {
    if let Some(team) = team {
        let remediation = Some(error.remediation().to_owned());
        findings.push(DoctorFinding {
            severity,
            code: error.code(),
            message: team_message(team, error.detail()),
            remediation,
        });
    } else {
        push_doctor_error(findings, severity, error);
    }
}
