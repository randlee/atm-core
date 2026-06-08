use std::collections::HashSet;
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InboxSourceDiagnostics {
    pub duplicate_message_ids: usize,
    pub messages_without_ids: usize,
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

pub fn report_inbox_diagnostics(source_files: &[SourceFileRecord]) -> InboxSourceDiagnostics {
    let mut seen = HashSet::new();
    let mut duplicate_message_ids = 0usize;
    let mut messages_without_ids = 0usize;

    for source in source_files {
        for message in &source.messages {
            if let Some(message_id) = message.message_id {
                if !seen.insert(message_id) {
                    duplicate_message_ids += 1;
                }
            } else {
                messages_without_ids += 1;
            }
        }
    }

    InboxSourceDiagnostics {
        duplicate_message_ids,
        messages_without_ids,
    }
}

pub fn export_source_files(source_files: &[SourceFileRecord]) -> Result<usize, AtmError> {
    let committed_paths = source_files.len();
    mailbox::export_source_projections(source_files)?;
    Ok(committed_paths)
}

pub fn reexport_messages(path: &Path, messages: &[MessageEnvelope]) -> Result<usize, AtmError> {
    let wrote_messages = messages.len();
    mailbox::reexport_messages(path, messages)?;
    Ok(wrote_messages)
}

pub fn append_message_set(
    path: &Path,
    mode: ProjectionAppendMode,
    messages: &[MessageEnvelope],
) -> Result<usize, AtmError> {
    let wrote_messages = messages.len();
    mailbox::append_message_set(path, mode, messages)?;
    Ok(wrote_messages)
}
