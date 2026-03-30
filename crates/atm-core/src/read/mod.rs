pub mod filters;
pub mod seen_state;
pub mod state;
pub mod wait;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::address::AgentAddress;
use crate::config;
use crate::error::{AtmError, AtmErrorKind};
use crate::home;
use crate::identity;
use crate::mailbox;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::schema::{MessageEnvelope, TeamConfig};
use crate::types::{AckActivationMode, DisplayBucket, MessageClass, ReadSelection};

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
    pub timestamp_filter: Option<DateTime<Utc>>,
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
    pub bucket: DisplayBucket,
    pub class: MessageClass,
    #[serde(flatten)]
    pub message: MessageEnvelope,
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

    let team_config = load_team_config(&team_dir)?;
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

    let mut mailbox_messages = load_inbox_messages(&query.home_dir, &target.team, &target.agent)?;
    let mut derived = build_derived_messages(
        mailbox_messages.clone(),
        query.sender_filter.as_deref(),
        query.timestamp_filter,
    );

    let mut bucket_counts = bucket_counts_for(&derived, query.selection_mode, seen_watermark);
    let mut selected = select_messages(&derived, query.selection_mode, seen_watermark);
    let mut timed_out = false;

    if selected.is_empty() {
        if let Some(timeout_secs) = query.timeout_secs {
            let wait_satisfied = wait::wait_for_eligible_message(
                &query.home_dir,
                &target.team,
                &target.agent,
                timeout_secs,
                |message| {
                    message_matches_filters(
                        message,
                        query.sender_filter.as_deref(),
                        query.timestamp_filter,
                        query.selection_mode,
                        seen_watermark,
                    )
                },
            )?;

            if wait_satisfied {
                mailbox_messages =
                    load_inbox_messages(&query.home_dir, &target.team, &target.agent)?;
                derived = build_derived_messages(
                    mailbox_messages.clone(),
                    query.sender_filter.as_deref(),
                    query.timestamp_filter,
                );
                bucket_counts = bucket_counts_for(&derived, query.selection_mode, seen_watermark);
                selected = select_messages(&derived, query.selection_mode, seen_watermark);
            } else {
                timed_out = true;
            }
        }
    }

    selected.sort_by(|left, right| {
        right
            .message
            .timestamp
            .cmp(&left.message.timestamp)
            .then_with(|| right.message.message_id.cmp(&left.message.message_id))
            .then_with(|| right.source_index.cmp(&left.source_index))
    });

    if let Some(limit) = query.limit {
        selected.truncate(limit);
    }

    let mutation_applied = if timed_out || selected.is_empty() {
        false
    } else {
        apply_display_mutations(
            &mut mailbox_messages,
            &selected,
            query.ack_activation_mode,
            own_inbox,
        )
    };

    if mutation_applied {
        let inbox_path = home::inbox_path_from_home(&query.home_dir, &target.team, &target.agent)?;
        mailbox::atomic::write_messages(&inbox_path, &mailbox_messages)?;
    }

    if query.seen_state_update && !selected.is_empty() {
        if let Some(latest_timestamp) = selected
            .iter()
            .map(|message| message.message.timestamp)
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
            bucket: selected_message.bucket,
            class: selected_message.class,
            message: mailbox_messages[selected_message.source_index].clone(),
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
        message_id: String::new(),
        requires_ack: false,
        dry_run: false,
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

fn load_inbox_messages(
    home_dir: &Path,
    team: &str,
    agent: &str,
) -> Result<Vec<MessageEnvelope>, AtmError> {
    let inbox_path = home::inbox_path_from_home(home_dir, team, agent)?;
    let messages = mailbox::read_messages(&inbox_path)?;
    Ok(dedupe_messages(messages))
}

fn dedupe_messages(messages: Vec<MessageEnvelope>) -> Vec<MessageEnvelope> {
    let mut latest_for_id: HashMap<Uuid, (DateTime<Utc>, usize)> = HashMap::new();
    for (index, message) in messages.iter().enumerate() {
        if let Some(message_id) = message.message_id {
            latest_for_id
                .entry(message_id)
                .and_modify(|entry| {
                    if message.timestamp > entry.0
                        || (message.timestamp == entry.0 && index > entry.1)
                    {
                        *entry = (message.timestamp, index);
                    }
                })
                .or_insert((message.timestamp, index));
        }
    }

    messages
        .into_iter()
        .enumerate()
        .filter_map(|(index, message)| match message.message_id {
            Some(message_id) => latest_for_id
                .get(&message_id)
                .and_then(|(_, keep_index)| (*keep_index == index).then_some(message)),
            None => Some(message),
        })
        .collect()
}

fn build_derived_messages(
    messages: Vec<MessageEnvelope>,
    sender_filter: Option<&str>,
    timestamp_filter: Option<DateTime<Utc>>,
) -> Vec<ClassifiedMessage> {
    messages
        .into_iter()
        .enumerate()
        .filter(|(_, message)| {
            sender_filter
                .map(|sender| message.from == sender)
                .unwrap_or(true)
                && timestamp_filter
                    .map(|since| message.timestamp >= since)
                    .unwrap_or(true)
        })
        .map(|(source_index, message)| {
            let class = state::classify_message(&message);
            let bucket = state::display_bucket_for_class(class);

            ClassifiedMessage {
                source_index,
                bucket,
                class,
                message,
            }
        })
        .collect()
}

fn bucket_counts_for(
    messages: &[ClassifiedMessage],
    selection_mode: ReadSelection,
    seen_watermark: Option<DateTime<Utc>>,
) -> BucketCounts {
    let count_source = if selection_mode == ReadSelection::All {
        filters::apply_selection_mode(messages.to_vec(), ReadSelection::All, None)
    } else {
        filters::apply_selection_mode(
            messages.to_vec(),
            ReadSelection::ActionableWithHistory,
            seen_watermark,
        )
    };

    count_source.into_iter().fold(
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
    seen_watermark: Option<DateTime<Utc>>,
) -> Vec<ClassifiedMessage> {
    let watermark = if selection_mode == ReadSelection::All {
        None
    } else {
        seen_watermark
    };

    filters::apply_selection_mode(messages.to_vec(), selection_mode, watermark)
}

fn message_matches_filters(
    message: &MessageEnvelope,
    sender_filter: Option<&str>,
    timestamp_filter: Option<DateTime<Utc>>,
    selection_mode: ReadSelection,
    seen_watermark: Option<DateTime<Utc>>,
) -> bool {
    if let Some(sender) = sender_filter {
        if message.from != sender {
            return false;
        }
    }

    if let Some(since) = timestamp_filter {
        if message.timestamp < since {
            return false;
        }
    }

    let classified = ClassifiedMessage {
        source_index: 0,
        class: state::classify_message(message),
        bucket: state::display_bucket_for_class(state::classify_message(message)),
        message: message.clone(),
    };

    !select_messages(&[classified], selection_mode, seen_watermark).is_empty()
}

fn apply_display_mutations(
    mailbox_messages: &mut [MessageEnvelope],
    displayed_messages: &[ClassifiedMessage],
    ack_activation_mode: AckActivationMode,
    own_inbox: bool,
) -> bool {
    let mut mutation_applied = false;
    let promote_unread =
        own_inbox && ack_activation_mode == AckActivationMode::PromoteDisplayedUnread;
    let now = Utc::now();

    for message in displayed_messages {
        let stored = &mut mailbox_messages[message.source_index];
        let was_read = stored.read;
        let had_pending_ack = stored.pending_ack_at.is_some();
        let had_acknowledged = stored.acknowledged_at.is_some();

        stored.read = true;
        if !was_read {
            mutation_applied = true;
        }

        if promote_unread && !was_read && !had_pending_ack && !had_acknowledged {
            stored.pending_ack_at = Some(now);
            mutation_applied = true;
        }
    }

    mutation_applied
}
