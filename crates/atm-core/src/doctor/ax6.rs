use super::{DoctorFinding, DoctorSeverity};
use crate::boundary::TaskState;
use crate::error_codes::AtmErrorCode;
use crate::types::TeamName;
use atm_storage::{RosterMember, RosterSnapshot, TaskRow};

pub(super) fn member_info_findings(
    team: &TeamName,
    roster: &RosterSnapshot,
    tasks: &[TaskRow],
    findings: &mut Vec<DoctorFinding>,
) {
    let counts = roster
        .members
        .iter()
        .map(|member| {
            let assigned = task_count(tasks, member, TaskState::Assigned);
            let active = task_count(tasks, member, TaskState::Active);
            format!(
                "{}={{assigned:{assigned}, active:{active}}}",
                member.agent_name
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    findings.push(DoctorFinding {
        severity: DoctorSeverity::Info,
        code: AtmErrorCode::ObservabilityHealthOk,
        message: format!("team {team} task counts: {counts}"),
        remediation: None,
    });
}

fn task_count(tasks: &[TaskRow], member: &RosterMember, state: TaskState) -> usize {
    tasks
        .iter()
        .filter(|task| task.assignee == member.agent_name && task.state == state)
        .count()
}
