//! Durable terminal-state escalation message preparation and delivery.

use super::pipeline::{TerminalHerdrEscalationOutcome, write_mail_with_runtime};
use crate::boundary::{self, MemberKey};
use crate::error::AtmError;
use crate::observability::ObservabilityPort;
use crate::send::{NudgeMode, SendMessageSource, WriteRequest};
use crate::service_runtime::LocalServiceRuntime;
use crate::types::{AgentName, IsoTimestamp, TeamName};

pub(super) fn terminal_episode(
    runtime: &atm_storage::RosterRuntimeObservation,
) -> Result<(&'static str, IsoTimestamp), AtmError> {
    let started_at = runtime.state_changed_at.ok_or_else(|| {
        AtmError::validation("Herdr escalation admission requires a runtime episode epoch")
    })?;
    match runtime.state {
        crate::protocol::RuntimeMemberState::Blocked => Ok(("blocked", started_at)),
        crate::protocol::RuntimeMemberState::Offline => Ok(("offline", started_at)),
        _ => Err(AtmError::validation(
            "Herdr escalation admission requires blocked or offline runtime state",
        )),
    }
}

pub(super) fn terminal_escalation_details(
    runtime: &LocalServiceRuntime,
    task_store: &dyn atm_storage::TaskStore,
    source_member: &MemberKey,
    team: &TeamName,
    state_name: &str,
    started_at: IsoTimestamp,
) -> Result<(Vec<crate::address::AgentAddress>, String, String), AtmError> {
    let tasks = task_store.open_tasks(source_member)?;
    if tasks.is_empty() {
        return Err(AtmError::validation(
            "Herdr escalation admission requires an authoritative open task",
        ));
    }
    let roster = runtime.shared_roster_store_arc().load_roster(team)?;
    let recipients = terminal_escalation_recipients(task_store, team, &roster)?;
    let task_ids = tasks
        .iter()
        .map(|task| task.task_id.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let summary = format!("escalation:{state_name}_escalated:{source_member}");
    let body = format!(
        "{state_name} member {} has been in this state since {}\nOpen tasks: {}\nRun: atm members --team {}",
        source_member.agent(),
        started_at,
        task_ids,
        team,
    );
    Ok((recipients, summary, body))
}

pub(super) fn terminal_escalation_recipients(
    task_store: &dyn atm_storage::TaskStore,
    team: &TeamName,
    roster: &atm_storage::RosterSnapshot,
) -> Result<Vec<crate::address::AgentAddress>, AtmError> {
    let leads: Vec<_> = roster
        .members
        .iter()
        .filter(|member| member.agent_type == crate::schema::AgentType::Lead)
        .map(|member| member.agent_name.clone())
        .collect();
    let mut recipients = if leads.len() == 1 {
        leads
            .into_iter()
            .map(|lead| format!("{lead}@{team}").parse())
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };
    for recipient in task_store.effective_escalation_recipients(team)? {
        let is_current_member = roster
            .members
            .iter()
            .any(|member| member.agent_name == *recipient.agent());
        if recipient.host().is_none()
            && recipient.team().is_none_or(|t| t == team)
            && is_current_member
            && !recipients.iter().any(|known| known == &recipient)
        {
            recipients.push(recipient);
        }
    }
    Ok(recipients)
}

pub(super) struct TerminalEscalationMail {
    pub(super) body: String,
    pub(super) summary: String,
    pub(super) started_at: IsoTimestamp,
}

pub(super) fn write_terminal_escalations(
    daemon_home: std::path::PathBuf,
    recipients: Vec<crate::address::AgentAddress>,
    team: TeamName,
    mail: TerminalEscalationMail,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
) -> Result<TerminalHerdrEscalationOutcome, AtmError> {
    let mut outcome = TerminalHerdrEscalationOutcome::default();
    for recipient in recipients {
        let recipient_name = recipient.agent().clone();
        if terminal_recipient_already_reported(
            runtime,
            &team,
            &recipient_name,
            &mail.summary,
            mail.started_at,
        )? {
            continue;
        }
        let request = WriteRequest::new(
            daemon_home.clone(),
            daemon_home.clone(),
            AgentName::from_validated(boundary::DAEMON_ACTOR_NAME),
            &recipient.to_string(),
            team.clone(),
            SendMessageSource::Inline(mail.body.clone()),
            Some(mail.summary.clone()),
            false,
            None,
            false,
        )?
        .with_internal_daemon_escalation_provenance()
        .with_nudge_mode(NudgeMode::Deferred);
        match write_mail_with_runtime(request, observability, runtime) {
            Ok(written) => outcome
                .deliveries
                .push((recipient_name, written.persisted_message_id())),
            Err(error) => {
                outcome.recipients_failed = outcome.recipients_failed.saturating_add(1);
                tracing::warn!(subsystem = "herdr_queue_wake", action = "terminal_escalation_mail_write", outcome = "failed", recipient = %recipient, error = %error, "Terminal escalation recipient write failed");
            }
        }
    }
    Ok(outcome)
}

fn terminal_recipient_already_reported(
    runtime: &LocalServiceRuntime,
    team: &TeamName,
    recipient: &AgentName,
    summary: &str,
    started_at: IsoTimestamp,
) -> Result<bool, AtmError> {
    let messages = runtime
        .message_store
        .list_messages(&atm_storage::MessageQuery {
            team: team.clone(),
            agent: recipient.clone(),
            sender: Some(AgentName::from_validated(boundary::DAEMON_ACTOR_NAME)),
            task_id: None,
            limit: None,
        })?;
    Ok(messages.iter().any(|message| {
        message.envelope.summary.as_deref() == Some(summary)
            && message.envelope.timestamp >= started_at
    }))
}
