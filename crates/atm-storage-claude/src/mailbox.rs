use std::fs;
use std::path::{Path, PathBuf};

use atm_storage::{AgentName, AtmError, Message, MessageEnvelope, MessageKey, MessageQuery, TeamName};

#[derive(Debug, Clone)]
struct SourceProjectionFile {
    path: PathBuf,
    agent: AgentName,
    team: TeamName,
    messages: Vec<MessageEnvelope>,
}

fn read_message_file(path: &Path) -> Result<Vec<MessageEnvelope>, AtmError> {
    let raw = fs::read_to_string(path).map_err(|error| {
        AtmError::mailbox_read(format!("failed to read mailbox {}: {error}", path.display()))
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
        AtmError::mailbox_read(format!("failed to parse mailbox {}: {error}", path.display()))
            .with_source(error)
    })
}

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
            AtmError::mailbox_write(format!("failed to write mailbox {}: {error}", path.display()))
                .with_source(error)
        });
    }
    let mut encoded = serde_json::to_vec(messages).map_err(|error| {
        AtmError::mailbox_write("failed to encode mailbox projection").with_source(error)
    })?;
    encoded.push(b'\n');
    fs::write(path, encoded).map_err(|error| {
        AtmError::mailbox_write(format!("failed to write mailbox {}: {error}", path.display()))
            .with_source(error)
    })
}

fn key_for_message(message: &MessageEnvelope) -> MessageKey {
    message
        .message_id
        .map(MessageKey::from)
        .unwrap_or_else(|| {
            MessageKey::new(format!(
                "{}:{}",
                message.from,
                message.timestamp.into_inner().to_rfc3339()
            ))
            .expect("derived message key is not blank")
        })
}

fn discover_source_paths(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<PathBuf>, AtmError> {
    let primary = crate::paths::inbox_path(home_dir, team, agent)?;
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

fn source_files_for_query(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<SourceProjectionFile>, AtmError> {
    let paths = discover_source_paths(home_dir, team, agent)?;
    paths.into_iter()
        .map(|path| {
            let messages = read_message_file(&path)?;
            Ok(SourceProjectionFile {
                path,
                agent: agent.clone(),
                team: team.clone(),
                messages,
            })
        })
        .collect()
}

fn matches_query(message: &MessageEnvelope, query: &MessageQuery) -> bool {
    if let Some(sender) = &query.sender
        && &message.from != sender
    {
        return false;
    }
    if let Some(task_id) = &query.task_id
        && message.task_id.as_ref() != Some(task_id)
    {
        return false;
    }
    true
}

fn all_source_files(home_dir: &Path) -> Result<Vec<SourceProjectionFile>, AtmError> {
    let teams_dir = home_dir.join(".claude").join("teams");
    if !teams_dir.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for team_entry in fs::read_dir(&teams_dir).map_err(|error| {
        AtmError::mailbox_read(format!(
            "failed to read teams directory {}: {error}",
            teams_dir.display()
        ))
        .with_source(error)
    })? {
        let team_entry = team_entry.map_err(|error| {
            AtmError::mailbox_read(format!(
                "failed to enumerate teams directory {}: {error}",
                teams_dir.display()
            ))
            .with_source(error)
        })?;
        let Some(team_name) = team_entry
            .file_name()
            .to_str()
            .and_then(|value| value.parse::<TeamName>().ok())
        else {
            continue;
        };
        let inboxes_dir = team_entry.path().join("inboxes");
        if !inboxes_dir.exists() {
            continue;
        }
        for inbox_entry in fs::read_dir(&inboxes_dir).map_err(|error| {
            AtmError::mailbox_read(format!(
                "failed to read inbox directory {}: {error}",
                inboxes_dir.display()
            ))
            .with_source(error)
        })? {
            let inbox_entry = inbox_entry.map_err(|error| {
                AtmError::mailbox_read(format!(
                    "failed to enumerate inbox directory {}: {error}",
                    inboxes_dir.display()
                ))
                .with_source(error)
            })?;
            let path = inbox_entry.path();
            let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            let Some(prefix) = file_name.strip_suffix(".json").or_else(|| file_name.strip_suffix(".jsonl")) else {
                continue;
            };
            let Some(agent_prefix) = prefix.split('.').next() else {
                continue;
            };
            let Ok(agent_name) = agent_prefix.parse::<AgentName>() else {
                continue;
            };
            let messages = read_message_file(&path)?;
            files.push(SourceProjectionFile {
                path,
                agent: agent_name,
                team: team_name.clone(),
                messages,
            });
        }
    }
    Ok(files)
}

pub fn save_message(home_dir: &Path, message: &Message) -> Result<(), AtmError> {
    let path = crate::paths::inbox_path(home_dir, &message.team, &message.agent)?;
    let mut messages = if path.exists() {
        read_message_file(&path)?
    } else {
        Vec::new()
    };
    messages.push(message.envelope.clone());
    write_message_file(&path, &messages)
}

pub fn load_message(home_dir: &Path, key: &MessageKey) -> Result<Option<Message>, AtmError> {
    for file in all_source_files(home_dir)? {
        for message in file.messages {
            if &key_for_message(&message) == key {
                return Ok(Some(Message {
                    team: file.team,
                    agent: file.agent,
                    message_key: key.clone(),
                    envelope: message,
                }));
            }
        }
    }
    Ok(None)
}

pub fn list_messages(home_dir: &Path, query: &MessageQuery) -> Result<Vec<Message>, AtmError> {
    let mut messages = Vec::new();
    for file in source_files_for_query(home_dir, &query.team, &query.agent)? {
        for message in file.messages {
            if matches_query(&message, query) {
                messages.push(Message {
                    team: file.team.clone(),
                    agent: file.agent.clone(),
                    message_key: key_for_message(&message),
                    envelope: message,
                });
            }
        }
    }
    if let Some(limit) = query.limit
        && messages.len() > limit
    {
        messages.truncate(limit);
    }
    Ok(messages)
}

pub fn delete_message(home_dir: &Path, key: &MessageKey) -> Result<(), AtmError> {
    for file in all_source_files(home_dir)? {
        let retained = file
            .messages
            .iter()
            .filter(|message| &key_for_message(message) != key)
            .cloned()
            .collect::<Vec<_>>();
        if retained.len() != file.messages.len() {
            write_message_file(&file.path, &retained)?;
        }
    }
    Ok(())
}
