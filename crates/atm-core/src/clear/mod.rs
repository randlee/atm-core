use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::config;
use crate::error::AtmError;
use crate::home;
use crate::identity;
use crate::mailbox;
use crate::mailbox::source::{
    SourceFile, SourcedMessage, discover_source_paths, load_source_files, resolve_target,
};
use crate::mailbox::surface::dedupe_legacy_message_id_surface;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::read::state;
use crate::schema::MessageEnvelope;
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

/// Remove read/acknowledged mailbox messages that match the clear query.
///
/// # Errors
///
/// Returns [`AtmError`] when config, identity, mailbox discovery, mailbox
/// locks, or atomic mailbox persistence fail.
pub fn clear_mail(
    query: ClearQuery,
    observability: &dyn ObservabilityPort,
) -> Result<ClearOutcome, AtmError> {
    let config = config::load_config(&query.current_dir)?;
    let actor = identity::resolve_actor_identity(query.actor_override.as_deref(), config.as_ref())?;
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

    let team_config = config::load_team_config(&team_dir)?;
    if target.explicit
        && !team_config
            .members
            .iter()
            .any(|member| member.name == target.agent)
    {
        return Err(AtmError::agent_not_found(&target.agent, &target.team));
    }

    let mut source_files = load_source_files(&discover_source_paths(
        &query.home_dir,
        &target.team,
        &target.agent,
    )?)?;
    // Clear intentionally does not apply read-surface idle-notification dedup.
    // Cleanup decisions must inspect the raw merged surface after legacy
    // message_id canonicalization only.
    let merged = dedupe_legacy_message_id_surface(
        merged_surface(&source_files),
        |message: &SourcedMessage| message.envelope.message_id,
        |message: &SourcedMessage| message.envelope.timestamp,
    );
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
        let source_paths = discover_source_paths(&query.home_dir, &target.team, &target.agent)?;
        let _locks = mailbox::lock::acquire_many_sorted(
            source_paths.clone(),
            mailbox::lock::DEFAULT_LOCK_TIMEOUT,
        )?;
        source_files = load_source_files(&source_paths)?;
        let merged = dedupe_legacy_message_id_surface(
            merged_surface(&source_files),
            |message: &SourcedMessage| message.envelope.message_id,
            |message: &SourcedMessage| message.envelope.timestamp,
        );
        let removable = merged
            .iter()
            .filter(|message| is_clearable(message, cutoff, query.idle_only))
            .map(|message| (message.source_path.clone(), message.source_index))
            .collect::<HashSet<_>>();

        apply_removals(&mut source_files, &removable);
        persist_source_files(&source_files)?;
    }

    let remaining_total = if query.dry_run {
        merged.len().saturating_sub(removable.len())
    } else {
        dedupe_legacy_message_id_surface(
            merged_surface(&source_files),
            |message: &SourcedMessage| message.envelope.message_id,
            |message: &SourcedMessage| message.envelope.timestamp,
        )
        .len()
    };

    let outcome = ClearOutcome {
        action: "clear",
        team: target.team,
        agent: target.agent,
        removed_total: removable.len(),
        remaining_total,
        removed_by_class,
    };

    let _ = observability.emit(CommandEvent {
        command: "clear",
        action: "clear",
        outcome: if query.dry_run { "dry_run" } else { "ok" },
        team: outcome.team.clone(),
        agent: outcome.agent.clone(),
        sender: actor,
        message_id: None,
        requires_ack: false,
        dry_run: query.dry_run,
        task_id: None,
        error_code: None,
        error_message: None,
    });

    Ok(outcome)
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
    // Claude Code currently defines idle notifications as JSON encoded in the
    // native `text` field. Do not replace this with an ATM-local schema here;
    // any ownership change must be documented in docs/claude-code-message-schema.md.
    serde_json::from_str::<Value>(&message.text)
        .ok()
        .map(|value| value.get("type").and_then(Value::as_str) == Some("idle_notification"))
        .unwrap_or(false)
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
