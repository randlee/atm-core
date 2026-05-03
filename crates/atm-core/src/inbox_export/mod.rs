use std::path::Path;

use crate::error::AtmError;
use crate::home;
use crate::mailbox;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::schema::MessageEnvelope;
use crate::types::TaskId;
use crate::types::{AgentName, TeamName};
use crate::workflow;

#[derive(Debug, Clone)]
pub struct ExportEventContext {
    pub command: &'static str,
    pub sender: String,
    pub message_id: Option<crate::schema::LegacyMessageId>,
    pub requires_ack: bool,
    pub task_id: Option<TaskId>,
}

pub fn export_message(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
    envelope: &MessageEnvelope,
    observability: &dyn ObservabilityPort,
    event: ExportEventContext,
) -> Result<(), AtmError> {
    let inbox_path = home::inbox_path_from_home(home_dir, team, agent)?;
    let result = workflow::commit_workflow_state(
        home_dir,
        team,
        agent,
        [inbox_path.clone()],
        mailbox::lock::default_lock_timeout(),
        |workflow_state| {
            let mut inbox_messages = mailbox::read_messages(&inbox_path)?;
            inbox_messages.push(envelope.clone());
            mailbox::store::commit_mailbox_state(&inbox_path, &inbox_messages)?;
            Ok((
                (),
                workflow::remember_initial_state(workflow_state, envelope),
            ))
        },
    );
    if let Err(error) = &result {
        let _ = observability.emit(CommandEvent {
            command: event.command,
            action: "export",
            outcome: "error",
            team: team.clone(),
            agent: agent.clone(),
            sender: event.sender,
            message_id: event.message_id,
            requires_ack: event.requires_ack,
            dry_run: false,
            task_id: event.task_id,
            error_code: Some(error.code),
            error_message: Some(error.message.clone()),
        });
    }
    result
}
