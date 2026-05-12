use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::address::AgentAddress;
use crate::error::AtmError;
use crate::identity;
use crate::mailbox::source::{SummarySourceFile, resolve_target};
use crate::mailbox::surface::dedupe_message_id_surface;
use crate::observability::ObservabilityPort;
use crate::read::{
    BucketCounts, ClassifiedMessage, filters, normalize_contains_filter, sort_and_limit_selected,
    state,
};
use crate::schema::{AtmMessageId, MessageEnvelope};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::RetainedMailboxRuntime;
use crate::threading::{ThreadIndex, is_ephemeral, is_expired_ephemeral};
use crate::types::{AgentName, CommandAction, IsoTimestamp, ReadSelection, TaskId, TeamName};
use crate::workflow;

const DEFAULT_LIST_LIMIT: usize = 200;
const MAX_LIST_LIMIT: usize = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListQuery {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub actor_override: Option<AgentName>,
    pub target_address: Option<AgentAddress>,
    pub team_override: Option<TeamName>,
    pub selection_mode: ReadSelection,
    pub seen_state_filter: bool,
    pub limit: Option<usize>,
    pub sender_filter: Option<AgentName>,
    pub timestamp_filter: Option<IsoTimestamp>,
    pub task_filter: Option<TaskId>,
    pub contains_filter: Option<String>,
}

impl ListQuery {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        home_dir: PathBuf,
        current_dir: PathBuf,
        actor_override: Option<&str>,
        target_address: Option<&str>,
        team_override: Option<&str>,
        selection_mode: ReadSelection,
        seen_state_filter: bool,
        limit: Option<usize>,
        sender_filter: Option<&str>,
        timestamp_filter: Option<IsoTimestamp>,
        task_filter: Option<&str>,
        contains_filter: Option<&str>,
    ) -> Result<Self, AtmError> {
        let limit = normalize_limit(limit)?;
        Ok(Self {
            home_dir,
            current_dir,
            actor_override: actor_override.map(str::parse).transpose()?,
            target_address: target_address.map(str::parse).transpose()?,
            team_override: team_override.map(str::parse).transpose()?,
            selection_mode,
            seen_state_filter,
            limit,
            sender_filter: sender_filter.map(str::parse).transpose()?,
            timestamp_filter,
            task_filter: task_filter.map(str::parse).transpose()?,
            contains_filter: normalize_contains_filter(contains_filter)?,
        })
    }
}

fn normalize_limit(limit: Option<usize>) -> Result<Option<usize>, AtmError> {
    match limit {
        Some(0) => Err(
            AtmError::validation("list limit must be at least 1".to_string()).with_recovery(
                "Use `--limit` with a positive integer before retrying `atm list`.",
            ),
        ),
        Some(value) if value > MAX_LIST_LIMIT => Err(
            AtmError::validation(format!(
                "list limit exceeds the {} row maximum",
                MAX_LIST_LIMIT
            ))
            .with_recovery(
                "Use a smaller `--limit` value before retrying `atm list` so daemon responses remain bounded.",
            ),
        ),
        Some(value) => Ok(Some(value)),
        None => Ok(Some(DEFAULT_LIST_LIMIT)),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListRow {
    #[serde(default)]
    pub message_id: Option<AtmMessageId>,
    pub summary: String,
    pub from: AgentName,
    pub timestamp: IsoTimestamp,
    pub read: bool,
    pub pending_ack: bool,
    pub task_id: Option<TaskId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListOutcome {
    pub action: CommandAction,
    pub team: TeamName,
    pub agent: AgentName,
    pub selection_mode: ReadSelection,
    pub history_collapsed: bool,
    pub count: usize,
    pub rows: Vec<ListRow>,
    pub bucket_counts: BucketCounts,
}

pub fn list_mail(
    query: ListQuery,
    observability: &dyn ObservabilityPort,
) -> Result<ListOutcome, AtmError> {
    let runtime = LocalServiceRuntime::default();
    list_mail_with_runtime(query, observability, &runtime)
}

#[derive(Debug, Clone)]
struct SummaryProjection {
    summary_preview: String,
    body_contains_match: bool,
    idle_notification_sender: Option<AgentName>,
}

#[derive(Debug, Clone)]
struct SummarySourcedMessage {
    projection: SummaryProjection,
    envelope: MessageEnvelope,
    source_path: PathBuf,
    source_index: crate::types::SourceIndex,
}

fn list_mail_with_runtime<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    query: ListQuery,
    _observability: &dyn ObservabilityPort,
    runtime: &R,
) -> Result<ListOutcome, AtmError> {
    let config = runtime.load_config(&query.current_dir)?;
    let actor = identity::resolve_actor_identity(query.actor_override.as_deref(), config.as_ref())?;
    let target = resolve_target(
        query.target_address.as_ref(),
        &actor,
        query.team_override.as_ref(),
        config.as_ref(),
    )?;
    let team_dir = runtime.team_dir(&query.home_dir, &target.team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&target.team).with_recovery(
            "Create the team config for the requested team or target a different team before retrying `atm list`.",
        ));
    }

    let team_config = runtime.load_team_config(&team_dir)?;
    if target.explicit
        && !team_config
            .members
            .iter()
            .any(|member| member.name == target.agent.as_str())
    {
        return Err(
            AtmError::agent_not_found(&target.agent, &target.team).with_recovery(
                "Update the team membership in config.json or list a different mailbox target.",
            ),
        );
    }

    let seen_watermark = if query.seen_state_filter && query.selection_mode != ReadSelection::All {
        runtime.load_seen_watermark(&query.home_dir, &target.team, &target.agent)?
    } else {
        None
    };

    let workflow_state =
        runtime.load_workflow_state(&query.home_dir, &target.team, &target.agent)?;
    let source_files = runtime.observe_summary_source_files(
        &query.home_dir,
        &target.team,
        &target.agent,
        query.contains_filter.as_deref(),
    )?;
    let projection_index = projection_index(&source_files);
    let classified_all = classify_summary_source_files(&source_files, &workflow_state);
    let logical_current = logical_current_messages(classified_all);
    let bucket_counts = bucket_counts_for(&logical_current);
    let filtered = apply_list_filters(
        logical_current,
        &projection_index,
        query.sender_filter.as_ref(),
        query.timestamp_filter,
        query.task_filter.as_ref(),
        query.contains_filter.as_deref(),
    );
    let mut selected = select_messages(&filtered, query.selection_mode, seen_watermark);
    sort_and_limit_selected(&mut selected, query.limit);

    let rows = selected
        .iter()
        .map(|message| list_row_from_message(message, &projection_index))
        .collect::<Vec<_>>();
    let history_collapsed = query.selection_mode != ReadSelection::All && bucket_counts.history > 0;

    Ok(ListOutcome {
        action: CommandAction::List,
        team: target.team,
        agent: target.agent,
        selection_mode: query.selection_mode,
        history_collapsed,
        count: rows.len(),
        rows,
        bucket_counts,
    })
}

fn projection_index(
    source_files: &[SummarySourceFile],
) -> HashMap<(PathBuf, usize), SummaryProjection> {
    source_files
        .iter()
        .flat_map(|source| {
            source
                .messages
                .iter()
                .cloned()
                .enumerate()
                .map(|(index, message)| {
                    (
                        (source.path.clone(), index),
                        SummaryProjection {
                            summary_preview: message.summary_preview,
                            body_contains_match: message.body_contains_match,
                            idle_notification_sender: message.idle_notification_sender,
                        },
                    )
                })
        })
        .collect()
}

fn classify_summary_source_files(
    source_files: &[SummarySourceFile],
    workflow_state: &workflow::WorkflowStateFile,
) -> Vec<ClassifiedMessage> {
    let deduped = dedupe_message_id_surface(
        merged_summary_surface(source_files),
        |message: &SummarySourcedMessage| message.envelope.message_id,
        |message: &SummarySourcedMessage| message.envelope.timestamp,
    );
    let projected = apply_idle_notification_dedup(deduped);
    let envelopes = projected
        .iter()
        .map(|message| workflow::project_envelope(&message.envelope, workflow_state))
        .collect::<Vec<_>>();
    let thread_index = ThreadIndex::new(&envelopes);

    projected
        .into_iter()
        .zip(envelopes.iter().cloned())
        .map(|(message, projected)| {
            let effective = effective_display_envelope(&projected, &thread_index);
            let class = state::classify_message(&effective);
            let bucket = state::display_bucket_for_class(class);
            ClassifiedMessage {
                source_index: message.source_index,
                source_path: message.source_path,
                bucket,
                class,
                envelope: effective,
            }
        })
        .collect()
}

fn merged_summary_surface(source_files: &[SummarySourceFile]) -> Vec<SummarySourcedMessage> {
    source_files
        .iter()
        .flat_map(|source| {
            source
                .messages
                .iter()
                .cloned()
                .enumerate()
                .map(|(source_index, message)| SummarySourcedMessage {
                    projection: SummaryProjection {
                        summary_preview: message.summary_preview,
                        body_contains_match: message.body_contains_match,
                        idle_notification_sender: message.idle_notification_sender,
                    },
                    envelope: message.envelope,
                    source_path: source.path.clone(),
                    source_index: source_index.into(),
                })
        })
        .collect()
}

fn apply_idle_notification_dedup(
    messages: Vec<SummarySourcedMessage>,
) -> Vec<SummarySourcedMessage> {
    let mut latest_idle_for_sender = HashMap::new();
    for (index, message) in messages.iter().enumerate() {
        if !message.envelope.read
            && let Some(sender) = message.projection.idle_notification_sender.as_ref()
        {
            latest_idle_for_sender.insert(sender.clone(), index);
        }
    }

    messages
        .into_iter()
        .enumerate()
        .filter_map(|(index, message)| {
            if message.envelope.read {
                return Some(message);
            }
            match message.projection.idle_notification_sender.as_ref() {
                Some(sender) if latest_idle_for_sender.get(sender) != Some(&index) => None,
                _ => Some(message),
            }
        })
        .collect()
}

fn logical_current_messages(messages: Vec<ClassifiedMessage>) -> Vec<ClassifiedMessage> {
    let projected = messages
        .iter()
        .map(|message| message.envelope.clone())
        .collect::<Vec<_>>();
    let thread_index = ThreadIndex::new(&projected);

    messages
        .into_iter()
        .filter(|message| {
            message
                .envelope
                .message_id
                .is_none_or(|message_id| thread_index.is_terminal(message_id))
        })
        .collect()
}

fn apply_list_filters(
    messages: Vec<ClassifiedMessage>,
    projection_index: &HashMap<(PathBuf, usize), SummaryProjection>,
    sender_filter: Option<&AgentName>,
    timestamp_filter: Option<IsoTimestamp>,
    task_filter: Option<&TaskId>,
    contains_filter: Option<&str>,
) -> Vec<ClassifiedMessage> {
    let filtered = filters::apply_task_filter(
        filters::apply_timestamp_filter(
            filters::apply_sender_filter(messages, sender_filter),
            timestamp_filter,
        ),
        task_filter,
    );

    match contains_filter {
        Some(needle) => filtered
            .into_iter()
            .filter(|message| contains_projection_match(message, projection_index, needle))
            .collect(),
        None => filtered,
    }
}

fn contains_projection_match(
    message: &ClassifiedMessage,
    projection_index: &HashMap<(PathBuf, usize), SummaryProjection>,
    needle: &str,
) -> bool {
    let Some(projection) =
        projection_index.get(&(message.source_path.clone(), message.source_index.get()))
    else {
        return false;
    };

    projection.body_contains_match
        || ascii_case_insensitive_contains(&projection.summary_preview, needle)
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

    let visible = messages
        .iter()
        .filter(|message| !hidden_for_selection(&message.envelope, selection_mode))
        .cloned()
        .collect::<Vec<_>>();

    filters::apply_selection_mode(visible, selection_mode, watermark)
}

fn bucket_counts_for(messages: &[ClassifiedMessage]) -> BucketCounts {
    messages.iter().fold(
        BucketCounts {
            unread: 0,
            pending_ack: 0,
            history: 0,
        },
        |mut counts, message| {
            if hidden_from_normal_views(&message.envelope) {
                return counts;
            }
            match message.bucket {
                crate::types::DisplayBucket::Unread => counts.unread += 1,
                crate::types::DisplayBucket::PendingAck => counts.pending_ack += 1,
                crate::types::DisplayBucket::History => counts.history += 1,
            }
            counts
        },
    )
}

fn hidden_from_normal_views(envelope: &MessageEnvelope) -> bool {
    let now = IsoTimestamp::now();
    is_expired_ephemeral(envelope, now) || (is_ephemeral(envelope) && envelope.read)
}

fn hidden_for_selection(envelope: &MessageEnvelope, selection_mode: ReadSelection) -> bool {
    let now = IsoTimestamp::now();
    if is_expired_ephemeral(envelope, now) {
        return true;
    }
    selection_mode != ReadSelection::All && is_ephemeral(envelope) && envelope.read
}

fn effective_display_envelope(
    envelope: &MessageEnvelope,
    thread_index: &ThreadIndex<'_>,
) -> MessageEnvelope {
    let Some(message_id) = envelope.message_id else {
        return envelope.clone();
    };
    if thread_index.is_terminal(message_id) {
        return envelope.clone();
    }

    let mut historical = envelope.clone();
    historical.read = true;
    historical.pending_ack_at = None;
    historical
}

fn list_row_from_message(
    message: &ClassifiedMessage,
    projection_index: &HashMap<(PathBuf, usize), SummaryProjection>,
) -> ListRow {
    let summary = projection_index
        .get(&(message.source_path.clone(), message.source_index.get()))
        .map(|projection| projection.summary_preview.clone())
        .unwrap_or_default();

    ListRow {
        message_id: message.envelope.message_id,
        summary,
        from: message.envelope.from.clone(),
        timestamp: message.envelope.timestamp,
        read: message.envelope.read,
        pending_ack: message.envelope.pending_ack_at.is_some()
            && message.envelope.acknowledged_at.is_none(),
        task_id: message.envelope.task_id.clone(),
    }
}

fn ascii_case_insensitive_contains(haystack: &str, needle: &str) -> bool {
    let needle = needle.as_bytes();
    if needle.is_empty() {
        return true;
    }
    haystack.as_bytes().windows(needle.len()).any(|window| {
        window
            .iter()
            .zip(needle.iter())
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
    })
}
