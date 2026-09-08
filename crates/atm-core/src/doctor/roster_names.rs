use super::{DoctorFinding, DoctorSeverity, team_scope};
use crate::boundary::RosterEntry;
use crate::error_codes::AtmErrorCode;
use crate::types::TeamName;

pub(super) fn push_duplicate_effective_name_warnings(
    team: &TeamName,
    roster: &[RosterEntry],
    all_rosters: &[(TeamName, Vec<RosterEntry>)],
    team_context: bool,
    findings: &mut Vec<DoctorFinding>,
) {
    let all_names = all_rosters
        .iter()
        .flat_map(|(_, roster)| roster.iter())
        .map(atm_storage::RosterUniqueName::from_member)
        .collect::<Vec<_>>();
    let collisions = atm_storage::roster_unique_name_collisions(&all_names);
    for member in roster {
        let Some(member_name) = collisions.iter().find(|candidate| {
            candidate.team_name == member.team_name && candidate.agent_name == member.agent_name
        }) else {
            continue;
        };
        for conflict in collisions.iter().filter(|candidate| {
            candidate.unique_name == member_name.unique_name
                && (candidate.team_name != member_name.team_name
                    || candidate.agent_name != member_name.agent_name)
        }) {
            let detail = format!(
                "effective roster name '{}' for member '{}' conflicts with member '{}@{}'; assign a unique --alias before the next roster write",
                member_name.unique_name,
                member_name.agent_name,
                conflict.agent_name,
                conflict.team_name
            );
            findings.push(DoctorFinding {
                severity: DoctorSeverity::Warning,
                code: AtmErrorCode::WarningRosterDrift,
                message: if team_context {
                    team_scope::team_message(team, detail)
                } else {
                    detail
                },
                remediation: Some(
                    "Run `atm teams update-member --alias <unique-herdr-name> <team> <member>` to make the effective roster name unique."
                        .to_owned(),
                ),
            });
        }
    }
}
