use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use serde::Serialize;
use serde_json::Value;
use tracing::warn;
use uuid::Uuid;

use crate::address::AgentAddress;
use crate::config;
use crate::error::{AtmError, AtmErrorKind};
use crate::home;
use crate::identity;
use crate::mailbox;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::read::state;
use crate::schema::{MessageEnvelope, TeamConfig};
use crate::types::MessageClass;

#[derive(Debug, Clone)]
pub struct ClearQuery {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub actor_override: Option<String>,
    pub target_address: Option<String>,
    pub team_override: Option<String>,
    pub older_than: Option<Duration>,
    pub idle_only: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RemovedByClass {
    pub acknowledged: usize,
    pub read: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClearOutcome {
    pub action: &'static str,
    pub team: String,
    pub agent: String,
    pub removed_total: usize,
    pub remaining_total: usize,
    pub removed_by_class: RemovedByClass,
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

pub fn clear_mail(
    query: ClearQuery,
    observability: &dyn ObservabilityPort,
) -> Result<ClearOutcome, AtmError> {
    let config = config::load_config(&query.current_dir)?;
    let actor = resolve_actor_identity(query.actor_override.as_deref(), config.as_ref())?;
    let target = resolve_target(
        query.target_address.as_deref(),
        &actor,
        query.team_override.as_deref(),
        config.as_ref(),
    )?;

    let team_dir = home::team_dir_from_home(&query.home_dir, &target.team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&target.team));
    }

    let team_config = load_team_config(&team_dir)?;
    if target.explicit
        && !team_config
            .members
            .iter()
            .any(|member| member.name == target.agent)
    {
        return Err(AtmError::agent_not_found(&target.agent, &target.team));
    }

    let mut source_files = load_source_files(&query.home_dir, &target.team, &target.agent)?;
    let merged = dedupe_sourced_messages(merged_surface(&source_files));
    let cutoff = cutoff_timestamp(query.older_than)?;

    let mut removed_by_class = RemovedByClass::default();
    let removable = merged
        .iter()
        .filter(|message| is_clearable(message, cutoff, query.idle_only))
        .inspect(|message| {
            count_removed(
                &mut removed_by_class,
                state::classify_message(&message.envelope),
            )
        })
        .map(|message| (message.source_path.clone(), message.source_index))
        .collect::<HashSet<_>>();

    if !query.dry_run {
        apply_removals(&mut source_files, &removable);
        persist_source_files(&source_files)?;
    }

    let remaining_total = if query.dry_run {
        merged.len().saturating_sub(removable.len())
    } else {
        dedupe_sourced_messages(merged_surface(&source_files)).len()
    };

    let outcome = ClearOutcome {
        action: "clear",
        team: target.team,
        agent: target.agent,
        removed_total: removable.len(),
        remaining_total,
        removed_by_class,
    };

    let _ = observability.emit_command_event(CommandEvent {
        command: "clear",
        action: "clear",
        outcome: if query.dry_run { "dry_run" } else { "ok" },
        team: outcome.team.clone(),
        agent: outcome.agent.clone(),
        sender: actor,
        message_id: String::new(),
        requires_ack: false,
        dry_run: query.dry_run,
        task_id: None,
    });

    Ok(outcome)
}

#[derive(Debug)]
struct ResolvedTarget {
    agent: String,
    team: String,
    explicit: bool,
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

fn resolve_target(
    target_address: Option<&str>,
    actor: &str,
    team_override: Option<&str>,
    config: Option<&config::AtmConfig>,
) -> Result<ResolvedTarget, AtmError> {
    let Some(target_address) = target_address else {
        let team =
            config::resolve_team(team_override, config).ok_or_else(AtmError::team_unavailable)?;
        return Ok(ResolvedTarget {
            agent: actor.to_string(),
            team,
            explicit: false,
        });
    };

    let parsed: AgentAddress = target_address.parse()?;
    let team = parsed
        .team
        .or_else(|| config::resolve_team(team_override, config))
        .ok_or_else(AtmError::team_unavailable)?;

    Ok(ResolvedTarget {
        agent: parsed.agent,
        team,
        explicit: true,
    })
}

fn load_team_config(team_dir: &Path) -> Result<TeamConfig, AtmError> {
    let config_path = team_dir.join("config.json");
    let raw = fs::read_to_string(&config_path).map_err(|error| {
        AtmError::new(
            AtmErrorKind::Config,
            format!(
                "failed to read team config at {}: {error}",
                config_path.display()
            ),
        )
        .with_source(error)
    })?;

    serde_json::from_str(&raw).map_err(|error| {
        AtmError::new(
            AtmErrorKind::Config,
            format!(
                "failed to parse team config at {}: {error}",
                config_path.display()
            ),
        )
        .with_source(error)
    })
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
                .map(|name| {
                    name.starts_with(&prefix)
                        && name.ends_with(".json")
                        && name != format!("{agent}.json")
                })
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
                .collect::<Vec<_>>()
        })
        .collect()
}

fn dedupe_sourced_messages(messages: Vec<SourcedMessage>) -> Vec<SourcedMessage> {
    let mut latest_for_id: HashMap<Uuid, (crate::types::IsoTimestamp, usize)> = HashMap::new();
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

fn cutoff_timestamp(
    older_than: Option<Duration>,
) -> Result<Option<chrono::DateTime<Utc>>, AtmError> {
    older_than
        .map(|duration| {
            TimeDelta::from_std(duration)
                .map_err(|error| AtmError::validation(format!("invalid duration filter: {error}")))
        })
        .transpose()
        .map(|delta| delta.map(|delta| Utc::now() - delta))
}

fn is_clearable(message: &SourcedMessage, cutoff: Option<DateTime<Utc>>, idle_only: bool) -> bool {
    let class = state::classify_message(&message.envelope);
    matches!(class, MessageClass::Read | MessageClass::Acknowledged)
        && cutoff
            .map(|cutoff| message.envelope.timestamp.into_inner() <= cutoff)
            .unwrap_or(true)
        && (!idle_only || is_idle_notification(&message.envelope))
}

fn is_idle_notification(message: &MessageEnvelope) -> bool {
    serde_json::from_str::<Value>(&message.text)
        .ok()
        .and_then(|value| value.get("type").and_then(Value::as_str).map(str::to_owned))
        .as_deref()
        == Some("idle_notification")
}

fn count_removed(counts: &mut RemovedByClass, class: MessageClass) {
    match class {
        MessageClass::Unread => unreachable!("unread messages are never clearable"),
        MessageClass::PendingAck => unreachable!("pending-ack messages are never clearable"),
        MessageClass::Acknowledged => counts.acknowledged += 1,
        MessageClass::Read => counts.read += 1,
    }
}

fn apply_removals(source_files: &mut [SourceFile], removable: &HashSet<(PathBuf, usize)>) {
    for source in source_files {
        source.messages = source
            .messages
            .iter()
            .cloned()
            .enumerate()
            .filter_map(|(index, message)| {
                (!removable.contains(&(source.path.clone(), index))).then_some(message)
            })
            .collect();
    }
}

fn persist_source_files(source_files: &[SourceFile]) -> Result<(), AtmError> {
    for source in source_files {
        mailbox::atomic::write_messages(&source.path, &source.messages)?;
    }
    Ok(())
}
