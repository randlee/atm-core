use super::{DoctorFinding, DoctorSeverity, load_member_roster, push_doctor_error};
use crate::error_codes::AtmErrorCode;
use crate::service_runtime::LocalServiceRuntime;
use crate::team_admin::MembersList;
use crate::types::{AgentName, TeamName};
use std::path::Path;

/// The effective team scope for one doctor run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DoctorTeamScope {
    Single(TeamName),
    AllTeams { resolved_none: bool },
}

impl DoctorTeamScope {
    pub(super) fn report_name(&self) -> &'static str {
        match self {
            Self::Single(_) => "single",
            Self::AllTeams { .. } => "all_teams",
        }
    }

    pub(super) fn is_all_teams(&self) -> bool {
        matches!(self, Self::AllTeams { .. })
    }
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
    let rosters = teams
        .iter()
        .filter_map(|team| {
            load_member_roster(
                runtime,
                team,
                caller_identity,
                live_cwd,
                team_context,
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
            message: format!("team {team}: {}", error.detail()),
            remediation,
        });
    } else {
        push_doctor_error(findings, severity, error);
    }
}
