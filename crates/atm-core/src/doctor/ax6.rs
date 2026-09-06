use std::sync::Arc;

use super::{
    DoctorFinding, DoctorSeverity, EscalationRecipientsDoctorReport,
    TeamEscalationRecipientsDoctorReport,
};
use crate::boundary::{DurableRosterStore, TaskState, TaskStore};
use crate::error_codes::AtmErrorCode;
use crate::service_runtime::LocalServiceRuntime;
use crate::types::TeamName;
use atm_storage::{DAEMON_ACTOR_NAME, EscalationScope, RosterMember, RosterSnapshot, TaskRow};

pub(super) fn task_and_roster_findings(
    runtime: &LocalServiceRuntime,
    resolved_team: Option<&TeamName>,
    findings: &mut Vec<DoctorFinding>,
) -> EscalationRecipientsDoctorReport {
    let roster_store = runtime.shared_roster_store_arc();
    let teams = doctor_teams(roster_store.as_ref(), resolved_team, findings);
    let task_store = match runtime.task_store() {
        Ok(store) => Some(store),
        Err(error) => {
            push_storage_failure(findings, "task store", error);
            None
        }
    };
    let daemon = daemon_recipients(task_store.as_ref(), findings);
    let teams = teams
        .into_iter()
        .filter_map(|team| team_report(roster_store.as_ref(), task_store.as_ref(), team, findings))
        .collect();
    EscalationRecipientsDoctorReport { daemon, teams }
}

fn doctor_teams(
    roster_store: &(dyn DurableRosterStore + Send + Sync),
    resolved_team: Option<&TeamName>,
    findings: &mut Vec<DoctorFinding>,
) -> Vec<TeamName> {
    let mut teams = resolved_team.map_or_else(
        || match roster_store.list_teams() {
            Ok(teams) => teams,
            Err(error) => {
                push_storage_failure(findings, "team list", error);
                Vec::new()
            }
        },
        |team| vec![team.clone()],
    );
    teams.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    teams.dedup();
    teams
}

fn daemon_recipients(
    task_store: Option<&Arc<dyn TaskStore + Send + Sync>>,
    findings: &mut Vec<DoctorFinding>,
) -> Vec<String> {
    match task_store {
        Some(store) => match store.list_escalation_recipients(&EscalationScope::Daemon) {
            Ok(recipients) => recipients,
            Err(error) => {
                push_storage_failure(findings, "daemon escalation recipients", error);
                Vec::new()
            }
        },
        None => Vec::new(),
    }
}

fn team_report(
    roster_store: &(dyn DurableRosterStore + Send + Sync),
    task_store: Option<&Arc<dyn TaskStore + Send + Sync>>,
    team: TeamName,
    findings: &mut Vec<DoctorFinding>,
) -> Option<TeamEscalationRecipientsDoctorReport> {
    let roster = match roster_store.load_roster(&team) {
        Ok(roster) => roster,
        Err(error) => {
            push_storage_failure(findings, "team roster", error);
            return None;
        }
    };
    let tasks = match task_store {
        Some(store) => match store.list_tasks(&team, None) {
            Ok(tasks) => tasks,
            Err(error) => {
                push_storage_failure(findings, "team task list", error);
                Vec::new()
            }
        },
        None => Vec::new(),
    };
    team_findings(&team, &roster, &tasks, findings);
    let (own, effective) = team_recipients(task_store, &team, findings);
    Some(TeamEscalationRecipientsDoctorReport {
        team,
        source: if own.is_empty() {
            "daemon default".to_owned()
        } else {
            "team".to_owned()
        },
        recipients: effective,
    })
}

fn team_recipients(
    task_store: Option<&Arc<dyn TaskStore + Send + Sync>>,
    team: &TeamName,
    findings: &mut Vec<DoctorFinding>,
) -> (Vec<String>, Vec<String>) {
    let Some(store) = task_store else {
        return (Vec::new(), Vec::new());
    };
    let scope = EscalationScope::Team(team.clone());
    let own = match store.list_escalation_recipients(&scope) {
        Ok(recipients) => recipients,
        Err(error) => {
            push_storage_failure(findings, "team escalation recipients", error);
            Vec::new()
        }
    };
    let effective = match store.effective_escalation_recipients(team) {
        Ok(recipients) => recipients,
        Err(error) => {
            push_storage_failure(findings, "effective escalation recipients", error);
            Vec::new()
        }
    };
    (own, effective)
}

fn push_storage_failure(
    findings: &mut Vec<DoctorFinding>,
    subject: &str,
    error: crate::error::AtmError,
) {
    findings.push(DoctorFinding {
        severity: DoctorSeverity::Error,
        code: error.code(),
        message: format!("{subject} failed: {}", error.detail()),
        remediation: Some(error.remediation().to_owned()),
    });
}

pub(super) fn team_findings(
    team: &TeamName,
    roster: &RosterSnapshot,
    tasks: &[TaskRow],
    findings: &mut Vec<DoctorFinding>,
) {
    let lead_count = roster
        .members
        .iter()
        .filter(|member| member.agent_type == crate::schema::AgentType::Lead)
        .count();
    if lead_count == 0 {
        findings.push(DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: AtmErrorCode::RosterNoLead,
            message: format!("team {team} has no lead member"),
            remediation: Some(
                "assign one lead: atm teams update-member <team> <member> --agent-type lead"
                    .to_owned(),
            ),
        });
    } else if lead_count > 1 {
        findings.push(DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: AtmErrorCode::RosterMultipleLeads,
            message: format!("team {team} has {lead_count} lead members"),
            remediation: Some(
                "keep one lead: atm teams update-member <team> <member> --agent-type <other type>"
                    .to_owned(),
            ),
        });
    }
    reserved_name_findings(team, roster, findings);
    for task in tasks.iter().filter(|task| {
        task.state != TaskState::Complete
            && task.reminder_count >= crate::boundary::TASK_STALLED_REMINDER_THRESHOLD
    }) {
        findings.push(DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: AtmErrorCode::TaskStalled,
            message: format!(
                "task {} assigned to {} has been reminded {} times",
                task.task_id, task.assignee, task.reminder_count
            ),
            remediation: Some(
                "check the assignee or close the task: atm send <assignee> --task-complete <task_id> --stdin"
                    .to_owned(),
            ),
        });
    }
    member_info_findings(team, roster, tasks, findings);
}

fn reserved_name_findings(
    team: &TeamName,
    roster: &RosterSnapshot,
    findings: &mut Vec<DoctorFinding>,
) {
    for member in &roster.members {
        if member.agent_name.as_str() == DAEMON_ACTOR_NAME {
            findings.push(DoctorFinding {
                severity: DoctorSeverity::Warning,
                code: AtmErrorCode::RosterReservedName,
                message: format!("team {team} contains reserved member name {DAEMON_ACTOR_NAME}"),
                remediation: Some(
                    "rename the member: atm-daemon is reserved for daemon-originated messages"
                        .to_owned(),
                ),
            });
        }
    }
}

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
