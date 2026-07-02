use std::path::Path;
use std::path::PathBuf;

use atm_storage::AtmError;
use atm_storage::{AgentName, MessageEnvelope, TeamName};
use serde::{Deserialize, Serialize};

use crate::mailbox;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceFileRecord {
    pub path: PathBuf,
    pub messages: Vec<MessageEnvelope>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectionAppendMode {
    RecoveredLogicalMessageSet,
}

pub fn import_inbox_source(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<SourceFileRecord>, AtmError> {
    mailbox::import_source_projections(home_dir, team, agent)
}

pub fn compute_identity_fingerprint(message: &MessageEnvelope) -> Option<String> {
    message
        .message_id
        .map(|message_id| message_id.to_string())
        .or_else(|| {
            Some(format!(
                "{}:{}",
                message.from,
                message.timestamp.into_inner().to_rfc3339()
            ))
        })
}

pub fn reexport_messages(path: &Path, messages: &[MessageEnvelope]) -> Result<usize, AtmError> {
    let wrote_messages = messages.len();
    mailbox::reexport_messages(path, messages)?;
    Ok(wrote_messages)
}
