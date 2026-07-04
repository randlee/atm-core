#![allow(
    dead_code,
    reason = "Phase AD obsolete: retained only as daemon-local historical Claude compatibility support until the later reconcile/watch deletion sprint removes these helpers entirely."
)]

use std::fs;
use std::path::{Path, PathBuf};

#[cfg(test)]
use atm_storage::AtmMessageId;
use atm_storage::{AgentName, AtmError, MessageEnvelope, TeamName};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SourceFileRecord {
    pub(crate) path: PathBuf,
    pub(crate) messages: Vec<MessageEnvelope>,
}

#[derive(Debug, Clone)]
struct SourceProjectionFile {
    path: PathBuf,
    messages: Vec<MessageEnvelope>,
}

pub(crate) fn import_inbox_source(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<SourceFileRecord>, AtmError> {
    source_files_for_query(home_dir, team, agent).map(|files| {
        files
            .into_iter()
            .map(|file| SourceFileRecord {
                path: file.path,
                messages: file.messages,
            })
            .collect()
    })
}

pub(crate) fn compute_identity_fingerprint(message: &MessageEnvelope) -> Option<String> {
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

#[cfg(test)]
pub(crate) fn reexport_messages(
    path: &Path,
    messages: &[MessageEnvelope],
) -> Result<usize, AtmError> {
    let wrote_messages = messages.len();
    let projected = projected_export_messages(path, messages)?;
    write_message_file(path, &projected)?;
    Ok(wrote_messages)
}

fn source_files_for_query(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<SourceProjectionFile>, AtmError> {
    let paths = discover_source_paths(home_dir, team, agent)?;
    paths
        .into_iter()
        .map(|path| {
            let messages = read_message_file(&path)?;
            Ok(SourceProjectionFile { path, messages })
        })
        .collect()
}

fn discover_source_paths(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<PathBuf>, AtmError> {
    let primary = inbox_path(home_dir, team, agent);
    let mut paths = Vec::new();
    if primary.exists() {
        paths.push(primary.clone());
    }
    let inboxes_dir = primary.parent().ok_or_else(|| {
        AtmError::mailbox_read(format!("mailbox path {} has no parent", primary.display()))
    })?;
    if inboxes_dir.exists() {
        let prefix = format!("{agent}.");
        let primary_name = format!("{agent}.json");
        for entry in fs::read_dir(inboxes_dir).map_err(|error| {
            AtmError::mailbox_read(format!(
                "failed to read inbox directory {}: {error}",
                inboxes_dir.display()
            ))
            .with_source(error)
        })? {
            let entry = entry.map_err(|error| {
                AtmError::mailbox_read(format!(
                    "failed to enumerate inbox directory {}: {error}",
                    inboxes_dir.display()
                ))
                .with_source(error)
            })?;
            let path = entry.path();
            if path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| {
                    name.starts_with(&prefix) && name.ends_with(".json") && name != primary_name
                })
            {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn read_message_file(path: &Path) -> Result<Vec<MessageEnvelope>, AtmError> {
    let raw = fs::read_to_string(path).map_err(|error| {
        AtmError::mailbox_read(format!(
            "failed to read mailbox {}: {error}",
            path.display()
        ))
        .with_source(error)
    })?;
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    if path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
    {
        return raw
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                serde_json::from_str::<MessageEnvelope>(line).map_err(|error| {
                    AtmError::mailbox_read(format!(
                        "failed to parse mailbox record in {}: {error}",
                        path.display()
                    ))
                    .with_source(error)
                })
            })
            .collect();
    }
    serde_json::from_str::<Vec<MessageEnvelope>>(&raw).map_err(|error| {
        AtmError::mailbox_read(format!(
            "failed to parse mailbox {}: {error}",
            path.display()
        ))
        .with_source(error)
    })
}

#[cfg(test)]
fn write_message_file(path: &Path, messages: &[MessageEnvelope]) -> Result<(), AtmError> {
    let parent = path.parent().ok_or_else(|| {
        AtmError::mailbox_write(format!("mailbox path {} has no parent", path.display()))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        AtmError::mailbox_write(format!(
            "failed to create mailbox directory {}: {error}",
            parent.display()
        ))
        .with_source(error)
    })?;
    if path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
    {
        let mut encoded = String::new();
        for message in messages {
            let line = serde_json::to_string(message).map_err(|error| {
                AtmError::mailbox_write("failed to encode mailbox record").with_source(error)
            })?;
            encoded.push_str(&line);
            encoded.push('\n');
        }
        return fs::write(path, encoded).map_err(|error| {
            AtmError::mailbox_write(format!(
                "failed to write mailbox {}: {error}",
                path.display()
            ))
            .with_source(error)
        });
    }
    let mut encoded = serde_json::to_vec(messages).map_err(|error| {
        AtmError::mailbox_write("failed to encode mailbox projection").with_source(error)
    })?;
    encoded.push(b'\n');
    fs::write(path, encoded).map_err(|error| {
        AtmError::mailbox_write(format!(
            "failed to write mailbox {}: {error}",
            path.display()
        ))
        .with_source(error)
    })
}

fn team_dir(home_dir: &Path, team: &TeamName) -> PathBuf {
    home_dir.join(".claude").join("teams").join(team.as_str())
}

fn inbox_path(home_dir: &Path, team: &TeamName, agent: &AgentName) -> PathBuf {
    team_dir(home_dir, team)
        .join("inboxes")
        .join(format!("{agent}.json"))
}

#[cfg(test)]
fn retrieval_stub_text(message_id: AtmMessageId) -> String {
    format!("atm read --message-id {message_id}")
}

#[cfg(test)]
const DEFAULT_CLAUDE_JSONL_BODY_EXPORT_MAX_BYTES: usize = 128 * 1024;

#[cfg(test)]
fn projected_export_messages(
    path: &Path,
    messages: &[MessageEnvelope],
) -> Result<Vec<MessageEnvelope>, AtmError> {
    messages
        .iter()
        .map(|message| projected_export_message(path, message))
        .collect()
}

#[cfg(test)]
fn projected_export_message(
    path: &Path,
    message: &MessageEnvelope,
) -> Result<MessageEnvelope, AtmError> {
    let export_cap = export_cap_for_path(path)?;
    if let Some(message_id) = message.message_id
        && (export_cap == 0 || message.text.len() > export_cap)
    {
        let mut projected = message.clone();
        projected.text = retrieval_stub_text(message_id);
        return Ok(projected);
    }
    Ok(message.clone())
}

#[cfg(test)]
fn export_cap_for_path(path: &Path) -> Result<usize, AtmError> {
    Ok(find_config_override(path)?.unwrap_or(DEFAULT_CLAUDE_JSONL_BODY_EXPORT_MAX_BYTES))
}

#[cfg(test)]
fn find_config_override(path: &Path) -> Result<Option<usize>, AtmError> {
    let Some(start_dir) = path.parent() else {
        return Ok(None);
    };
    let Some(config) = atm_core::load_atm_config(start_dir)? else {
        return Ok(None);
    };
    if config.claude_jsonl_body_export_max_bytes.is_zero() {
        return Ok(Some(0));
    }
    Ok(config.claude_jsonl_body_export_max_bytes.as_usize())
}
