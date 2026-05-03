use std::path::Path;

use crate::error::AtmError;
use crate::home;
use crate::mailbox;
use crate::schema::MessageEnvelope;
use crate::types::{AgentName, TeamName};
use crate::workflow;

pub fn export_message(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
    envelope: &MessageEnvelope,
) -> Result<(), AtmError> {
    let inbox_path = home::inbox_path_from_home(home_dir, team, agent)?;
    workflow::commit_workflow_state(
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
    )
}
