use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Map;
use tracing::{trace, warn};

use crate::address::AgentAddress;
use crate::config;
use crate::error::{AtmError, AtmErrorKind};
use crate::home;
use crate::identity;
use crate::mailbox;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::read::state;
use crate::schema::{LegacyMessageId, MessageEnvelope};
use crate::send::{input, summary};
use crate::types::IsoTimestamp;

#[derive(Debug, Clone)]
pub struct AckRequest {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub actor_override: Option<String>,
    pub team_override: Option<String>,
    pub message_id: LegacyMessageId,
    pub reply_body: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AckOutcome {
    pub action: &'static str,
    pub team: String,
    pub agent: String,
    pub message_id: LegacyMessageId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub reply_target: String,
    pub reply_message_id: LegacyMessageId,
    pub reply_text: String,
}

#[derive(Debug, Clone)]
struct SourceFile {
    path: PathBuf,
    messages: Vec<MessageEnvelope>,
}

#[derive(Debug, Clone)]
struct SourcedMessage {
    envelope: MessageEnvelope,
    source_path: PathBuf,
    source_index: usize,
}

pub fn ack_mail(
    request: AckRequest,
    observability: &dyn ObservabilityPort,
) -> Result<AckOutcome, AtmError> {
    let config = config::load_config(&request.current_dir)?;
    let actor = resolve_actor_identity(request.actor_override.as_deref(), config.as_ref())?;
    let team = config::resolve_team(request.team_override.as_deref(), config.as_ref())
        .ok_or_else(AtmError::team_unavailable)?;
    let team_dir = home::team_dir_from_home(&request.home_dir, &team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&team));
    }

    let team_config = config::load_team_config(&team_dir)?;
    if !team_config
        .members
        .iter()
        .any(|member| member.name == actor)
    {
        return Err(AtmError::agent_not_found(&actor, &team));
    }

    let mut source_files = load_source_files(&request.home_dir, &team, &actor)?;
    let source_message = dedupe_sourced_messages(merged_surface(&source_files))
        .into_iter()
        .filter_map(|message| match message.envelope.message_id {
            Some(_) => Some(message),
            None => {
                trace!(
                    source_path = %message.source_path.display(),
                    source_index = message.source_index,
                    "skipping source message without message_id during ack lookup"
                );
                None
            }
        })
        .find(|message| message.envelope.message_id == Some(request.message_id))
        .ok_or_else(|| {
            AtmError::validation(format!(
                "message {} was not found in {}@{}",
                request.message_id, actor, team
            ))
        })?;

    match (
        state::derive_read_state(&source_message.envelope),
        state::derive_ack_state(&source_message.envelope),
    ) {
        (crate::types::ReadState::Read, crate::types::AckState::PendingAck) => {}
        (_, crate::types::AckState::Acknowledged) => {
            return Err(AtmError::validation(format!(
                "message {} is already acknowledged",
                request.message_id
            )));
        }
        _ => {
            return Err(AtmError::validation(format!(
                "message {} is not in the (read, pending_ack) state",
                request.message_id
            )));
        }
    }

    let (reply_agent, reply_team) = resolve_reply_target(&source_message.envelope, &team)?;
    let reply_team_dir = home::team_dir_from_home(&request.home_dir, &reply_team)?;
    if !reply_team_dir.exists() {
        return Err(AtmError::team_not_found(&reply_team));
    }

    let reply_team_config = config::load_team_config(&reply_team_dir)?;
    if !reply_team_config
        .members
        .iter()
        .any(|member| member.name == reply_agent)
    {
        return Err(AtmError::agent_not_found(&reply_agent, &reply_team));
    }

    let ack_timestamp = IsoTimestamp::now();
    let reply_text = input::validate_message_text(request.reply_body)?;
    let reply_message_id = LegacyMessageId::new();
    let source_task_id = source_message.envelope.task_id.clone();
    let reply_message = MessageEnvelope {
        from: actor.clone(),
        text: reply_text.clone(),
        timestamp: ack_timestamp,
        read: false,
        source_team: Some(team.clone()),
        summary: Some(summary::build_summary(&reply_text, None)),
        message_id: Some(reply_message_id),
        pending_ack_at: None,
        acknowledged_at: None,
        acknowledges_message_id: Some(request.message_id),
        task_id: None,
        extra: Map::new(),
    };

    update_source_message(&mut source_files, &source_message, ack_timestamp)?;

    let reply_inbox_path =
        home::inbox_path_from_home(&request.home_dir, &reply_team, &reply_agent)?;
    append_reply_message(&mut source_files, &reply_inbox_path, reply_message)?;
    persist_source_files(&source_files)?;

    let outcome = AckOutcome {
        action: "ack",
        team: team.clone(),
        agent: actor.clone(),
        message_id: request.message_id,
        task_id: source_task_id.clone(),
        reply_target: format!("{reply_agent}@{reply_team}"),
        reply_message_id,
        reply_text: reply_text.clone(),
    };

    let _ = observability.emit_command_event(CommandEvent {
        command: "ack",
        action: "ack",
        outcome: "ok",
        team,
        agent: actor.clone(),
        sender: actor,
        message_id: Some(request.message_id.into()),
        requires_ack: false,
        dry_run: false,
        task_id: source_task_id,
    });

    Ok(outcome)
}

fn resolve_actor_identity(
    actor_override: Option<&str>,
    config: Option<&config::AtmConfig>,
) -> Result<String, AtmError> {
    if let Some(actor) = actor_override.filter(|value| !value.trim().is_empty()) {
        return Ok(actor.to_string());
    }

    if let Some(identity) = identity::hook::read_hook_identity()? {
        return Ok(identity);
    }

    identity::resolve_sender_identity(config)
}

fn resolve_reply_target(
    message: &MessageEnvelope,
    current_team: &str,
) -> Result<(String, String), AtmError> {
    let parsed: AgentAddress = if message.from.contains('@') {
        message.from.parse()?
    } else {
        AgentAddress {
            agent: message.from.clone(),
            team: message
                .source_team
                .clone()
                .or_else(|| Some(current_team.to_string())),
        }
    };

    let team = parsed.team.ok_or_else(AtmError::team_unavailable)?;
    Ok((parsed.agent, team))
}

fn load_source_files(
    home_dir: &Path,
    team: &str,
    agent: &str,
) -> Result<Vec<SourceFile>, AtmError> {
    let inbox_path = home::inbox_path_from_home(home_dir, team, agent)?;
    let inboxes_dir = inbox_path
        .parent()
        .ok_or_else(|| AtmError::mailbox_read("inbox path has no parent directory"))?;
    let inboxes_dir = inboxes_dir.to_path_buf();

    let mut paths = vec![inbox_path];
    paths.extend(discover_origin_inboxes(&inboxes_dir, agent)?);

    let mut sources = Vec::with_capacity(paths.len());
    for path in paths {
        sources.push(SourceFile {
            messages: mailbox::read_messages(&path)?,
            path,
        });
    }

    Ok(sources)
}

fn discover_origin_inboxes(inboxes_dir: &Path, agent: &str) -> Result<Vec<PathBuf>, AtmError> {
    if !inboxes_dir.exists() {
        return Ok(Vec::new());
    }

    let prefix = format!("{agent}.");
    let primary = format!("{agent}.json");
    let mut paths = fs::read_dir(inboxes_dir)
        .map_err(|error| {
            AtmError::new(
                AtmErrorKind::MailboxRead,
                format!(
                    "failed to read inbox directory {}: {error}",
                    inboxes_dir.display()
                ),
            )
            .with_source(error)
        })?
        .filter_map(|entry| match entry {
            Ok(entry) => Some(entry.path()),
            Err(error) => {
                warn!(
                    inbox_dir = %inboxes_dir.display(),
                    agent,
                    %error,
                    "skipping unreadable origin inbox entry"
                );
                None
            }
        })
        .filter(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .map(|name| name.starts_with(&prefix) && name.ends_with(".json") && name != primary)
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    paths.sort();
    Ok(paths)
}

fn merged_surface(source_files: &[SourceFile]) -> Vec<SourcedMessage> {
    source_files
        .iter()
        .flat_map(|source| {
            source
                .messages
                .iter()
                .cloned()
                .enumerate()
                .map(|(source_index, envelope)| SourcedMessage {
                    envelope,
                    source_path: source.path.clone(),
                    source_index,
                })
        })
        .collect()
}

fn dedupe_sourced_messages(messages: Vec<SourcedMessage>) -> Vec<SourcedMessage> {
    let mut latest_for_id: HashMap<LegacyMessageId, (IsoTimestamp, usize)> = HashMap::new();
    for (index, message) in messages.iter().enumerate() {
        if let Some(message_id) = message.envelope.message_id {
            latest_for_id
                .entry(message_id)
                .and_modify(|entry| {
                    if message.envelope.timestamp > entry.0
                        || (message.envelope.timestamp == entry.0 && index > entry.1)
                    {
                        *entry = (message.envelope.timestamp, index);
                    }
                })
                .or_insert((message.envelope.timestamp, index));
        }
    }

    messages
        .into_iter()
        .enumerate()
        .filter_map(|(index, message)| match message.envelope.message_id {
            Some(message_id) => latest_for_id
                .get(&message_id)
                .and_then(|(_, keep_index)| (*keep_index == index).then_some(message)),
            None => Some(message),
        })
        .collect()
}

fn update_source_message(
    source_files: &mut [SourceFile],
    source_message: &SourcedMessage,
    acknowledged_at: IsoTimestamp,
) -> Result<(), AtmError> {
    let source_file = source_files
        .iter_mut()
        .find(|source| source.path == source_message.source_path)
        .ok_or_else(|| {
            AtmError::mailbox_write(format!(
                "source inbox disappeared during acknowledgement: {}",
                source_message.source_path.display()
            ))
        })?;

    let stored = source_file
        .messages
        .get_mut(source_message.source_index)
        .ok_or_else(|| {
            AtmError::mailbox_write(format!(
                "source message index {} disappeared during acknowledgement",
                source_message.source_index
            ))
        })?;

    let transitioned = state::StoredMessage::<
        crate::types::ReadReadState,
        crate::types::PendingAckState,
    >::read_pending_ack(stored.clone())
    .acknowledge(acknowledged_at)
    .envelope;
    *stored = transitioned;
    Ok(())
}

fn append_reply_message(
    source_files: &mut Vec<SourceFile>,
    reply_inbox_path: &Path,
    reply_message: MessageEnvelope,
) -> Result<(), AtmError> {
    if let Some(source_file) = source_files
        .iter_mut()
        .find(|source| source.path == reply_inbox_path)
    {
        source_file.messages.push(reply_message);
        return Ok(());
    }

    source_files.push(SourceFile {
        path: reply_inbox_path.to_path_buf(),
        messages: vec![reply_message],
    });
    source_files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(())
}

fn persist_source_files(source_files: &[SourceFile]) -> Result<(), AtmError> {
    for source in source_files {
        mailbox::atomic::write_messages(&source.path, &source.messages)?;
    }
    Ok(())
}
