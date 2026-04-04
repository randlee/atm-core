pub(crate) mod filters;
pub(crate) mod seen_state;
pub(crate) mod state;
pub(crate) mod wait;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::config;
use crate::error::AtmError;
use crate::home;
use crate::identity;
use crate::mailbox;
use crate::mailbox::source::{discover_origin_inboxes, resolve_target, SourceFile, SourcedMessage};
use crate::mailbox::surface::dedupe_legacy_message_id_surface;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::schema::MessageEnvelope;
use crate::types::{AckActivationMode, DisplayBucket, IsoTimestamp, MessageClass, ReadSelection};

#[derive(Debug, Clone)]
pub struct ReadQuery {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub actor_override: Option<String>,
    pub target_address: Option<String>,
    pub team_override: Option<String>,
    pub selection_mode: ReadSelection,
    pub seen_state_filter: bool,
    pub seen_state_update: bool,
    pub ack_activation_mode: AckActivationMode,
    pub limit: Option<usize>,
    pub sender_filter: Option<String>,
    pub timestamp_filter: Option<IsoTimestamp>,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BucketCounts {
    pub unread: usize,
    pub pending_ack: usize,
    pub history: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClassifiedMessage {
    #[serde(skip)]
    source_index: usize,
    #[serde(skip)]
    source_path: PathBuf,
    pub bucket: DisplayBucket,
    pub class: MessageClass,
    #[serde(flatten)]
    pub envelope: MessageEnvelope,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReadOutcome {
    pub action: &'static str,
    pub team: String,
    pub agent: String,
    pub selection_mode: ReadSelection,
    pub history_collapsed: bool,
    pub mutation_applied: bool,
    pub count: usize,
    pub messages: Vec<ClassifiedMessage>,
    pub bucket_counts: BucketCounts,
}

pub fn read_mail(
    query: ReadQuery,
    observability: &dyn ObservabilityPort,
) -> Result<ReadOutcome, AtmError> {
    let config = config::load_config(&query.current_dir)?;
    let actor = resolve_actor_identity(query.actor_override.as_deref(), config.as_ref())?;
    let actor_team = config::resolve_team(query.team_override.as_deref(), config.as_ref());
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

    let own_inbox = actor == target.agent && actor_team.as_deref() == Some(target.team.as_str());
    let seen_watermark = if query.seen_state_filter && query.selection_mode != ReadSelection::All {
        seen_state::load_seen_watermark(&query.home_dir, &target.team, &target.agent)?
    } else {
        None
    };

    let mut source_files = load_source_files(&query.home_dir, &target.team, &target.agent)?;
    let mut classified_all = classify_all(apply_idle_notification_dedup(
        dedupe_legacy_message_id_surface(
            merged_surface(&source_files),
            |message: &SourcedMessage| message.envelope.message_id,
            |message: &SourcedMessage| message.envelope.timestamp,
        ),
    ));
    let mut bucket_counts = bucket_counts_for(&classified_all);
    let mut filtered = apply_filters(
        classified_all.clone(),
        query.sender_filter.as_deref(),
        query.timestamp_filter,
    );
    let mut selected = select_messages(&filtered, query.selection_mode, seen_watermark);
    let mut timed_out = false;

    if selected.is_empty() {
        if let Some(timeout_secs) = query.timeout_secs {
            let wait_satisfied = wait::wait_for_eligible_message(
                timeout_secs,
                || {
                    Ok(apply_idle_notification_dedup(
                        dedupe_legacy_message_id_surface(
                            merged_surface(&load_source_files(
                                &query.home_dir,
                                &target.team,
                                &target.agent,
                            )?),
                            |message: &SourcedMessage| message.envelope.message_id,
                            |message: &SourcedMessage| message.envelope.timestamp,
                        ),
                    ))
                },
                |messages| !selected_after_filters(messages, &query, seen_watermark).is_empty(),
            )?;

            if wait_satisfied {
                source_files = load_source_files(&query.home_dir, &target.team, &target.agent)?;
                classified_all = classify_all(apply_idle_notification_dedup(
                    dedupe_legacy_message_id_surface(
                        merged_surface(&source_files),
                        |message: &SourcedMessage| message.envelope.message_id,
                        |message: &SourcedMessage| message.envelope.timestamp,
                    ),
                ));
                bucket_counts = bucket_counts_for(&classified_all);
                filtered = apply_filters(
                    classified_all.clone(),
                    query.sender_filter.as_deref(),
                    query.timestamp_filter,
                );
                selected = select_messages(&filtered, query.selection_mode, seen_watermark);
            } else {
                timed_out = true;
            }
        }
    }

    selected.sort_by(|left, right| {
        right
            .envelope
            .timestamp
            .cmp(&left.envelope.timestamp)
            .then_with(|| right.envelope.message_id.cmp(&left.envelope.message_id))
            .then_with(|| right.source_index.cmp(&left.source_index))
    });

    if let Some(limit) = query.limit {
        selected.truncate(limit);
    }

    let mutation_applied = if timed_out || selected.is_empty() {
        false
    } else {
        apply_display_mutations(
            &mut source_files,
            &selected,
            query.ack_activation_mode,
            own_inbox,
        )
    };

    if mutation_applied {
        persist_source_files(&source_files)?;
    }

    if query.seen_state_update && !selected.is_empty() {
        if let Some(latest_timestamp) = selected
            .iter()
            .map(|message| message.envelope.timestamp)
            .max()
        {
            seen_state::save_seen_watermark(
                &query.home_dir,
                &target.team,
                &target.agent,
                latest_timestamp,
            )?;
        }
    }

    let output_messages = selected
        .into_iter()
        .map(|selected_message| ClassifiedMessage {
            source_index: selected_message.source_index,
            source_path: selected_message.source_path.clone(),
            bucket: selected_message.bucket,
            class: selected_message.class,
            envelope: source_files
                .iter()
                .find(|source| source.path == selected_message.source_path)
                .and_then(|source| source.messages.get(selected_message.source_index))
                .cloned()
                .unwrap_or(selected_message.envelope),
        })
        .collect::<Vec<_>>();

    let history_collapsed = query.selection_mode != ReadSelection::All
        && query.selection_mode != ReadSelection::ActionableWithHistory
        && bucket_counts.history > 0;

    let outcome = ReadOutcome {
        action: "read",
        team: target.team,
        agent: target.agent,
        selection_mode: query.selection_mode,
        history_collapsed,
        mutation_applied,
        count: output_messages.len(),
        messages: output_messages,
        bucket_counts,
    };

    let _ = observability.emit_command_event(CommandEvent {
        command: "read",
        action: "read",
        outcome: if timed_out { "timeout" } else { "ok" },
        team: outcome.team.clone(),
        agent: outcome.agent.clone(),
        sender: actor,
        message_id: None,
        requires_ack: false,
        dry_run: false,
        task_id: None,
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
        let messages = mailbox::read_messages(&path)?;
        sources.push(SourceFile { path, messages });
    }

    Ok(sources)
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

fn apply_idle_notification_dedup(deduped: Vec<SourcedMessage>) -> Vec<SourcedMessage> {
    let latest_idle_for_sender = messages_from_idle_sender(&deduped);

    deduped
        .into_iter()
        .enumerate()
        .filter_map(|(index, message)| {
            dedupe_idle_notifications(index, &message, &latest_idle_for_sender).then_some(message)
        })
        .collect()
}

fn dedupe_idle_notifications(
    index: usize,
    message: &SourcedMessage,
    latest_idle_for_sender: &HashMap<String, usize>,
) -> bool {
    if !is_unread_idle_notification(&message.envelope) {
        return true;
    }

    idle_sender(&message.envelope)
        .and_then(|sender| latest_idle_for_sender.get(&sender))
        .map(|keep_index| *keep_index == index)
        .unwrap_or(true)
}

fn messages_from_idle_sender(messages: &[SourcedMessage]) -> HashMap<String, usize> {
    let mut latest_idle_for_sender = HashMap::new();

    for (index, message) in messages.iter().enumerate() {
        if !is_unread_idle_notification(&message.envelope) {
            continue;
        }

        if let Some(sender) = idle_sender(&message.envelope) {
            latest_idle_for_sender
                .entry(sender)
                .and_modify(|keep_index| *keep_index = index)
                .or_insert(index);
        }
    }

    latest_idle_for_sender
}

fn is_unread_idle_notification(message: &MessageEnvelope) -> bool {
    !message.read && idle_notification_sender(message).is_some()
}

fn idle_sender(message: &MessageEnvelope) -> Option<String> {
    idle_notification_sender(message)
}

fn idle_notification_sender(message: &MessageEnvelope) -> Option<String> {
    serde_json::from_str::<Value>(&message.text)
        .ok()
        .and_then(|value| {
            (value.get("type").and_then(Value::as_str) == Some("idle_notification"))
                .then(|| {
                    value
                        .get("from")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .flatten()
        })
}

fn classify_all(messages: Vec<SourcedMessage>) -> Vec<ClassifiedMessage> {
    messages
        .into_iter()
        .map(|message| {
            let class = state::classify_message(&message.envelope);
            let bucket = state::display_bucket_for_class(class);

            ClassifiedMessage {
                source_index: message.source_index,
                source_path: message.source_path,
                bucket,
                class,
                envelope: message.envelope,
            }
        })
        .collect()
}

fn apply_filters(
    messages: Vec<ClassifiedMessage>,
    sender_filter: Option<&str>,
    timestamp_filter: Option<IsoTimestamp>,
) -> Vec<ClassifiedMessage> {
    filters::apply_timestamp_filter(
        filters::apply_sender_filter(messages, sender_filter),
        timestamp_filter,
    )
}

fn bucket_counts_for(messages: &[ClassifiedMessage]) -> BucketCounts {
    messages.iter().fold(
        BucketCounts {
            unread: 0,
            pending_ack: 0,
            history: 0,
        },
        |mut counts, message| {
            match message.bucket {
                DisplayBucket::Unread => counts.unread += 1,
                DisplayBucket::PendingAck => counts.pending_ack += 1,
                DisplayBucket::History => counts.history += 1,
            }
            counts
        },
    )
}

fn select_messages(
    messages: &[ClassifiedMessage],
    selection_mode: ReadSelection,
    seen_watermark: Option<IsoTimestamp>,
) -> Vec<ClassifiedMessage> {
    let watermark = if selection_mode == ReadSelection::All {
        None
    } else {
        seen_watermark
    };

    filters::apply_selection_mode(messages.to_vec(), selection_mode, watermark)
}

fn selected_after_filters(
    messages: &[SourcedMessage],
    query: &ReadQuery,
    seen_watermark: Option<IsoTimestamp>,
) -> Vec<ClassifiedMessage> {
    let classified = classify_all(messages.to_vec());
    let filtered = apply_filters(
        classified,
        query.sender_filter.as_deref(),
        query.timestamp_filter,
    );
    select_messages(&filtered, query.selection_mode, seen_watermark)
}

fn apply_display_mutations(
    source_files: &mut [SourceFile],
    displayed_messages: &[ClassifiedMessage],
    ack_activation_mode: AckActivationMode,
    own_inbox: bool,
) -> bool {
    let mut mutation_applied = false;
    let promote_unread =
        own_inbox && ack_activation_mode == AckActivationMode::PromoteDisplayedUnread;
    let now = IsoTimestamp::now();

    for message in displayed_messages {
        let transitioned = transition_displayed_message(message, promote_unread, now);
        let updated = transitioned.into_envelope();
        if updated != message.envelope {
            if let Some(source_file) = source_files
                .iter_mut()
                .find(|source| source.path == message.source_path)
            {
                if let Some(stored) = source_file.messages.get_mut(message.source_index) {
                    *stored = updated;
                    mutation_applied = true;
                }
            }
        }
    }

    mutation_applied
}

fn transition_displayed_message(
    message: &ClassifiedMessage,
    promote_unread: bool,
    now: IsoTimestamp,
) -> state::TransitionedMessage {
    let read_state = state::derive_read_state(&message.envelope);
    let ack_state = state::derive_ack_state(&message.envelope);

    match (read_state, ack_state) {
        (crate::types::ReadState::Unread, crate::types::AckState::NoAckRequired) if promote_unread => {
            state::TransitionedMessage::ReadPendingAck(
                state::StoredMessage::<crate::types::UnreadReadState, crate::types::NoAckState>::unread_no_ack(
                    message.envelope.clone(),
                )
                .display_and_require_ack(now),
            )
        }
        (crate::types::ReadState::Unread, crate::types::AckState::NoAckRequired) => {
            state::TransitionedMessage::ReadNoAck(
                state::StoredMessage::<crate::types::UnreadReadState, crate::types::NoAckState>::unread_no_ack(
                    message.envelope.clone(),
                )
                .display_without_ack(),
            )
        }
        (crate::types::ReadState::Unread, crate::types::AckState::PendingAck) => {
            state::TransitionedMessage::ReadPendingAck(
                state::StoredMessage::<
                    crate::types::UnreadReadState,
                    crate::types::PendingAckState,
                >::unread_pending_ack(message.envelope.clone())
                .mark_read_pending_ack(),
            )
        }
        (crate::types::ReadState::Unread, crate::types::AckState::Acknowledged)
        | (crate::types::ReadState::Read, crate::types::AckState::NoAckRequired)
        | (crate::types::ReadState::Read, crate::types::AckState::PendingAck)
        | (crate::types::ReadState::Read, crate::types::AckState::Acknowledged) => {
            let mut unchanged = message.envelope.clone();
            if !unchanged.read {
                unchanged.read = true;
            }
            state::TransitionedMessage::Unchanged(unchanged)
        }
    }
}

fn persist_source_files(source_files: &[SourceFile]) -> Result<(), AtmError> {
    for source in source_files {
        mailbox::atomic::write_messages(&source.path, &source.messages)?;
    }

    Ok(())
}
