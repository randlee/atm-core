pub(crate) mod filters;
pub(crate) mod metadata_selection;
pub(crate) mod seen_state;
pub(crate) mod state;
pub(crate) mod wait;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::address::AgentAddress;
use crate::boundary;
use crate::config;
use crate::error::AtmError;
use crate::identity;
use crate::mailbox::source::resolve_target;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::schema::{AtmMessageId, MessageEnvelope};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::{RetainedMailboxRuntime, default_runtime};
use crate::threading::ThreadIndex;
use crate::types::{
    AckActivationMode, AgentName, CommandAction, DisplayBucket, IsoTimestamp, MessageClass,
    ReadSelection, SourceIndex, TaskId, TeamName,
};
use metadata_selection::{selection_state_for_mailbox_metadata_rows, sort_and_limit_selected};

pub const MAX_CONTAINS_FILTER_LEN: usize = 1024;
pub const MAX_TIMEOUT_SECS: u64 = 3600;

/// Parameters for querying and optionally mutating one mailbox display surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadQuery {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub actor_override: Option<AgentName>,
    pub target_address: Option<AgentAddress>,
    pub team_override: Option<TeamName>,
    pub selection_mode: ReadSelection,
    pub seen_state_filter: bool,
    pub seen_state_update: bool,
    pub ack_activation_mode: AckActivationMode,
    pub message_id_filter: Option<AtmMessageId>,
    pub sender_filter: Option<AgentName>,
    pub timestamp_filter: Option<IsoTimestamp>,
    pub task_filter: Option<TaskId>,
    pub contains_filter: Option<String>,
    pub timeout_secs: Option<u64>,
}

impl ReadQuery {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        home_dir: PathBuf,
        current_dir: PathBuf,
        actor_override: Option<&str>,
        target_address: Option<&str>,
        team_override: Option<&str>,
        selection_mode: ReadSelection,
        seen_state_filter: bool,
        seen_state_update: bool,
        ack_activation_mode: AckActivationMode,
        message_id_filter: Option<&str>,
        sender_filter: Option<&str>,
        timestamp_filter: Option<IsoTimestamp>,
        task_filter: Option<&str>,
        contains_filter: Option<&str>,
        timeout_secs: Option<u64>,
    ) -> Result<Self, AtmError> {
        let contains_filter = normalize_contains_filter(contains_filter)?;
        let timeout_secs = validate_timeout_secs(timeout_secs)?;
        Ok(Self {
            home_dir,
            current_dir,
            actor_override: actor_override.map(str::parse).transpose()?,
            target_address: target_address.map(str::parse).transpose()?,
            team_override: team_override.map(str::parse).transpose()?,
            selection_mode,
            seen_state_filter,
            seen_state_update,
            ack_activation_mode,
            message_id_filter: message_id_filter
                .map(|value| {
                    value.parse::<AtmMessageId>().map_err(|source| {
                        AtmError::validation(format!("invalid message id: {value}"))
                            .with_recovery(
                                "Provide a valid UUID-formatted --message-id before retrying `atm read`.",
                            )
                            .with_source(source)
                    })
                })
                .transpose()?,
            sender_filter: sender_filter.map(str::parse).transpose()?,
            timestamp_filter,
            task_filter: task_filter.map(str::parse).transpose()?,
            contains_filter,
            timeout_secs,
        })
    }
}

pub(crate) fn normalize_contains_filter(
    contains_filter: Option<&str>,
) -> Result<Option<String>, AtmError> {
    match contains_filter.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.len() > MAX_CONTAINS_FILTER_LEN => Err(
            AtmError::validation(format!(
                "contains filter exceeds the {}-byte maximum",
                MAX_CONTAINS_FILTER_LEN
            ))
            .with_recovery(
                "Shorten the `--contains` filter before retrying so ATM can keep daemon-side substring scans bounded.",
            ),
        ),
        Some(value) => Ok(Some(value.to_string())),
        None => Ok(None),
    }
}

fn validate_timeout_secs(timeout_secs: Option<u64>) -> Result<Option<u64>, AtmError> {
    match timeout_secs {
        Some(value) if value > MAX_TIMEOUT_SECS => Err(AtmError::validation(format!(
            "timeout exceeds the {} second maximum",
            MAX_TIMEOUT_SECS
        ))
        .with_recovery("Use a timeout no greater than one hour before retrying `atm read`.")),
        _ => Ok(timeout_secs),
    }
}

/// Bucket counts for one classified mailbox surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketCounts {
    pub unread: usize,
    pub pending_ack: usize,
    pub history: usize,
}

/// One mailbox message classified for ATM display output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifiedMessage {
    #[serde(skip)]
    pub(crate) source_index: SourceIndex,
    #[serde(skip)]
    pub(crate) source_path: PathBuf,
    pub bucket: DisplayBucket,
    pub class: MessageClass,
    #[serde(flatten)]
    pub envelope: MessageEnvelope,
}

/// Result of one mailbox read/query command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadOutcome {
    pub action: CommandAction,
    pub team: TeamName,
    pub agent: AgentName,
    pub selection_mode: ReadSelection,
    pub mutation_applied: bool,
    pub count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<ClassifiedMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_message_id: Option<AtmMessageId>,
    pub match_count: usize,
    pub additional_match_count: usize,
    pub bucket_counts: BucketCounts,
}

/// Read one mailbox surface, optionally marking displayed messages as read.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::IdentityUnavailable`],
/// [`crate::error_codes::AtmErrorCode::TeamUnavailable`],
/// [`crate::error_codes::AtmErrorCode::TeamNotFound`],
/// [`crate::error_codes::AtmErrorCode::AgentNotFound`],
/// [`crate::error_codes::AtmErrorCode::AddressParseFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxReadFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxWriteFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxLockFailed`], or
/// [`crate::error_codes::AtmErrorCode::MailboxLockTimeout`] when actor or
/// target resolution fails, the team or agent cannot be validated, shared
/// mailbox locks cannot be acquired, or the selected mailbox state cannot be
/// reloaded or persisted safely.
pub fn read_mail(
    query: ReadQuery,
    observability: &dyn ObservabilityPort,
) -> Result<ReadOutcome, AtmError> {
    let runtime = default_runtime()?;
    read_mail_with_runtime_impl(query, observability, &runtime)
}

pub fn read_mail_with_runtime(
    query: ReadQuery,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
) -> Result<ReadOutcome, AtmError> {
    read_mail_with_runtime_impl(query, observability, runtime)
}

fn read_mail_with_runtime_impl<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    query: ReadQuery,
    observability: &dyn ObservabilityPort,
    runtime: &R,
) -> Result<ReadOutcome, AtmError> {
    let ReadRuntimeContext {
        actor,
        actor_team,
        target,
        seen_watermark,
    } = resolve_read_context(&query, runtime)?;
    let own_inbox = actor == target.agent && actor_team.as_deref() == Some(target.team.as_str());
    let mut metadata_rows =
        runtime.query_mailbox_metadata_rows(&query.home_dir, &target.team, &target.agent, None)?;
    let (mut bucket_counts, mut selected) =
        selection_state_for_mailbox_metadata_rows(&metadata_rows, &query, seen_watermark);
    let mut timed_out = false;

    if selected.is_empty()
        && let Some(timeout_secs) = query.timeout_secs
    {
        let wait_satisfied = wait::wait_for_eligible_message(
            timeout_secs,
            || {
                runtime.query_mailbox_metadata_rows(
                    &query.home_dir,
                    &target.team,
                    &target.agent,
                    None,
                )
            },
            |rows| {
                !selection_state_for_mailbox_metadata_rows(rows, &query, seen_watermark)
                    .1
                    .is_empty()
            },
        )?;

        if wait_satisfied {
            metadata_rows = runtime.query_mailbox_metadata_rows(
                &query.home_dir,
                &target.team,
                &target.agent,
                None,
            )?;
            (bucket_counts, selected) =
                selection_state_for_mailbox_metadata_rows(&metadata_rows, &query, seen_watermark);
        } else {
            timed_out = true;
        }
    }

    let match_count = selected.len();
    sort_and_limit_selected(&mut selected, Some(1));
    let mutation_needed = displayed_messages_require_mutation(&selected);

    let (mutation_applied, output_message, bucket_counts, selected_message_id, match_count) =
        if timed_out || selected.is_empty() || !mutation_needed {
            (
                false,
                output_messages_from_metadata_selection(
                    runtime,
                    &query.home_dir,
                    &target.team,
                    &target.agent,
                    &metadata_rows,
                    &selected,
                    query.message_id_filter,
                )?
                .into_iter()
                .next(),
                bucket_counts,
                selected
                    .first()
                    .and_then(|message| message.envelope.message_id),
                match_count,
            )
        } else {
            let mut metadata_rows = runtime.query_mailbox_metadata_rows(
                &query.home_dir,
                &target.team,
                &target.agent,
                None,
            )?;
            let (bucket_counts, mut selected) =
                selection_state_for_mailbox_metadata_rows(&metadata_rows, &query, seen_watermark);
            let match_count = selected.len();
            sort_and_limit_selected(&mut selected, Some(1));
            let mutation_applied = apply_display_mutations_to_store(
                runtime,
                &target.team,
                &target.agent,
                &selected,
                query.ack_activation_mode,
                own_inbox,
            )?;
            metadata_rows = runtime.query_mailbox_metadata_rows(
                &query.home_dir,
                &target.team,
                &target.agent,
                None,
            )?;
            let (_updated_counts, updated_selected) =
                selection_state_for_mailbox_metadata_rows(&metadata_rows, &query, seen_watermark);
            let output_message = output_messages_from_metadata_selection(
                runtime,
                &query.home_dir,
                &target.team,
                &target.agent,
                &metadata_rows,
                &updated_selected.into_iter().take(1).collect::<Vec<_>>(),
                query.message_id_filter,
            )?
            .into_iter()
            .next();
            (
                mutation_applied,
                output_message,
                bucket_counts,
                selected
                    .first()
                    .and_then(|message| message.envelope.message_id),
                match_count,
            )
        };

    if query.seen_state_update
        && !selected.is_empty()
        && let Some(latest_timestamp) = selected
            .iter()
            .map(|message| message.envelope.timestamp)
            .max()
    {
        runtime.save_seen_watermark(
            &query.home_dir,
            &target.team,
            &target.agent,
            latest_timestamp,
        )?;
    }

    let outcome = ReadOutcome {
        action: CommandAction::Read,
        team: target.team.clone(),
        agent: target.agent.clone(),
        selection_mode: query.selection_mode,
        mutation_applied,
        count: usize::from(output_message.is_some()),
        message: output_message,
        selected_message_id,
        match_count,
        additional_match_count: match_count.saturating_sub(1),
        bucket_counts,
    };

    if let Err(error) = observability.emit(CommandEvent {
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
        error_code: None,
        error_message: None,
    }) {
        tracing::warn!(%error, command = "read", action = "read", "failed to emit read command event");
    }

    Ok(outcome)
}

struct ReadRuntimeContext {
    actor: AgentName,
    actor_team: Option<TeamName>,
    target: crate::mailbox::source::ResolvedTarget,
    seen_watermark: Option<IsoTimestamp>,
}

fn resolve_read_context<R: RetainedServiceRuntime>(
    query: &ReadQuery,
    runtime: &R,
) -> Result<ReadRuntimeContext, AtmError> {
    let config = runtime.load_config(&query.current_dir)?;
    let actor = identity::resolve_actor_identity(query.actor_override.as_deref(), config.as_ref())?;
    let actor_team = config::resolve_team(query.team_override.as_deref(), config.as_ref());
    let target = resolve_target(
        query.target_address.as_ref(),
        &actor,
        query.team_override.as_ref(),
        config.as_ref(),
    )?;

    let team_dir = runtime.team_dir(&query.home_dir, &target.team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&target.team).with_recovery(
            "Create the team config for the requested team or target a different team before retrying `atm read`.",
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
                "Update the team membership in config.json or read a different mailbox target.",
            ),
        );
    }

    let seen_watermark = if query.seen_state_filter && query.selection_mode != ReadSelection::All {
        runtime.load_seen_watermark(&query.home_dir, &target.team, &target.agent)?
    } else {
        None
    };

    Ok(ReadRuntimeContext {
        actor,
        actor_team,
        target,
        seen_watermark,
    })
}

fn message_key_for_classified(
    message: &ClassifiedMessage,
) -> Result<boundary::MessageKey, AtmError> {
    boundary::MessageKey::new(message.source_path.to_string_lossy().into_owned())
}

fn output_messages_from_metadata_selection<R: RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
    metadata_rows: &[boundary::MailStoreMailboxMetadataRow],
    selected: &[ClassifiedMessage],
    exact_message_id: Option<AtmMessageId>,
) -> Result<Vec<ClassifiedMessage>, AtmError> {
    let row_by_id = metadata_rows
        .iter()
        .filter_map(|row| row.message_id.map(|message_id| (message_id, row)))
        .collect::<HashMap<_, _>>();

    selected
        .iter()
        .cloned()
        .map(|selected_message| {
            let message_key = message_key_for_classified(&selected_message)?;
            let Some(record) = runtime.load_message_record(home_dir, team, agent, &message_key)?
            else {
                return Err(AtmError::validation(format!(
                    "sqlite mailbox metadata row {} could not be reloaded for read output",
                    message_key
                ))
                .with_recovery(
                    "Repair or remove the malformed sqlite mailbox row before retrying `atm read`.",
                ));
            };
            let envelope = if exact_message_id == record.envelope.message_id {
                record.envelope
            } else if record.envelope.thread_mode == Some(crate::schema::ThreadMode::AddDetails) {
                load_logical_current_record(
                    runtime,
                    home_dir,
                    team,
                    agent,
                    &row_by_id,
                    &selected_message,
                    record.envelope,
                )?
            } else {
                record.envelope
            };
            Ok(ClassifiedMessage {
                source_index: selected_message.source_index,
                source_path: selected_message.source_path,
                bucket: state::display_bucket_for_class(state::classify_message(&envelope)),
                class: state::classify_message(&envelope),
                envelope,
            })
        })
        .collect()
}

fn load_logical_current_record<R: RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
    row_by_id: &HashMap<AtmMessageId, &boundary::MailStoreMailboxMetadataRow>,
    selected_message: &ClassifiedMessage,
    terminal_envelope: MessageEnvelope,
) -> Result<MessageEnvelope, AtmError> {
    let Some(mut current_id) = terminal_envelope.message_id else {
        return Ok(terminal_envelope);
    };

    let mut chain_ids = Vec::new();
    while let Some(row) = row_by_id.get(&current_id) {
        chain_ids.push(current_id);
        let Some(parent_id) = row.parent_message_id else {
            break;
        };
        current_id = parent_id;
    }
    chain_ids.reverse();

    let mut chain = Vec::new();
    for message_id in chain_ids {
        let Some(row) = row_by_id.get(&message_id) else {
            return Err(AtmError::validation(format!(
                "sqlite mailbox thread row for {} disappeared during logical-current reconstruction",
                message_id
            ))
            .with_recovery(
                "Repair the malformed sqlite thread chain before retrying `atm read`.",
            ));
        };
        let Some(record) = runtime.load_message_record(home_dir, team, agent, &row.message_key)?
        else {
            return Err(AtmError::validation(format!(
                "sqlite mailbox thread row {} could not be reloaded for logical-current reconstruction",
                row.message_key
            ))
            .with_recovery(
                "Repair the malformed sqlite thread chain before retrying `atm read`.",
            ));
        };
        chain.push(record.envelope);
    }
    let thread_index = ThreadIndex::new(&chain);
    thread_index
        .logical_current_envelope(
            terminal_envelope
                .message_id
                .or(selected_message.envelope.message_id)
                .unwrap_or(current_id),
        )
        .ok_or_else(|| {
            AtmError::validation("failed to reconstruct logical current thread envelope")
                .with_recovery(
                    "Repair or remove the malformed sqlite mailbox thread rows before retrying `atm read`.",
                )
        })
}

fn apply_display_mutations_to_store<R: RetainedMailboxRuntime>(
    runtime: &R,
    team: &TeamName,
    agent: &AgentName,
    displayed_messages: &[ClassifiedMessage],
    ack_activation_mode: AckActivationMode,
    own_inbox: bool,
) -> Result<bool, AtmError> {
    let mut changed = false;
    let promote_unread =
        own_inbox && ack_activation_mode == AckActivationMode::PromoteDisplayedUnread;
    let now = IsoTimestamp::now();

    for message in displayed_messages {
        let updated = transition_displayed_message(message, promote_unread, now).into_envelope();
        if updated == message.envelope {
            continue;
        }
        runtime.persist_message_state(boundary::MailMessageState {
            team: team.clone(),
            agent: agent.clone(),
            actor: agent.clone(),
            message_key: message_key_for_classified(message)?,
            read: updated.read,
            pending_ack_at: updated.pending_ack_at,
            acknowledged_at: updated.acknowledged_at,
            expires_at: updated.expires_at,
            deleted_at: None,
            updated_at: Some(now),
        })?;
        changed = true;
    }

    Ok(changed)
}

fn displayed_messages_require_mutation(displayed_messages: &[ClassifiedMessage]) -> bool {
    displayed_messages
        .iter()
        .any(|message| !message.envelope.read)
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use serde_json::Map;
    use serde_json::Value;
    use tempfile::tempdir;

    use super::{BucketCounts, ClassifiedMessage, ReadQuery, metadata_selection, state};
    use crate::mailbox::source::SourceFile;
    use crate::mailbox::source::SourcedMessage;
    use crate::mailbox::surface::dedupe_message_id_surface;
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::schema::{AtmMessageId, MessageEnvelope, ThreadMode};
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::threading::ThreadIndex;
    use crate::types::{
        AckActivationMode, AgentName, DisplayBucket, IsoTimestamp, MessageClass, ReadSelection,
        TaskId, TeamName,
    };
    use crate::workflow;

    fn selection_state_for_source_files(
        source_files: &[SourceFile],
        workflow_state: &workflow::WorkflowStateFile,
        query: &ReadQuery,
        seen_watermark: Option<IsoTimestamp>,
    ) -> (BucketCounts, Vec<ClassifiedMessage>) {
        let classified_all = classify_all(
            apply_idle_notification_dedup(
                dedupe_message_id_surface(
                    merged_surface(source_files),
                    |message: &SourcedMessage| message.envelope.message_id,
                    |message: &SourcedMessage| message.envelope.timestamp,
                ),
                workflow_state,
            ),
            workflow_state,
        );
        if let Some(message_id) = query.message_id_filter {
            let selected = classified_all
                .iter()
                .filter(|message| message.envelope.message_id == Some(message_id))
                .cloned()
                .collect();
            let logical_current = metadata_selection::logical_current_messages(classified_all);
            let bucket_counts = metadata_selection::bucket_counts_for(&logical_current);
            return (bucket_counts, selected);
        }
        let logical_current = metadata_selection::logical_current_messages(classified_all);
        let bucket_counts = metadata_selection::bucket_counts_for(&logical_current);
        let filtered = metadata_selection::apply_filters(
            logical_current,
            query.sender_filter.as_ref(),
            query.timestamp_filter,
            query.task_filter.as_ref(),
            query.contains_filter.as_deref(),
        );
        let selected =
            metadata_selection::select_messages(&filtered, query.selection_mode, seen_watermark);
        (bucket_counts, selected)
    }

    fn selected_after_filters(
        messages: &[SourcedMessage],
        workflow_state: &workflow::WorkflowStateFile,
        query: &ReadQuery,
        seen_watermark: Option<IsoTimestamp>,
    ) -> Vec<ClassifiedMessage> {
        let classified = classify_all(messages.to_vec(), workflow_state);
        if let Some(message_id) = query.message_id_filter {
            return classified
                .into_iter()
                .filter(|message| message.envelope.message_id == Some(message_id))
                .collect();
        }
        let filtered = metadata_selection::apply_filters(
            metadata_selection::logical_current_messages(classified),
            query.sender_filter.as_ref(),
            query.timestamp_filter,
            query.task_filter.as_ref(),
            query.contains_filter.as_deref(),
        );
        metadata_selection::select_messages(&filtered, query.selection_mode, seen_watermark)
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
                        source_index: source_index.into(),
                    })
            })
            .collect()
    }

    fn apply_idle_notification_dedup(
        deduped: Vec<SourcedMessage>,
        workflow_state: &workflow::WorkflowStateFile,
    ) -> Vec<SourcedMessage> {
        let projected = deduped
            .into_iter()
            .map(|message| SourcedMessage {
                envelope: workflow::project_envelope(&message.envelope, workflow_state),
                source_path: message.source_path,
                source_index: message.source_index,
            })
            .collect::<Vec<_>>();
        let latest_idle_for_sender = messages_from_idle_sender(&projected);

        projected
            .into_iter()
            .enumerate()
            .filter_map(|(index, message)| {
                dedupe_idle_notifications(index, &message, &latest_idle_for_sender)
                    .then_some(message)
            })
            .collect()
    }

    fn dedupe_idle_notifications(
        index: usize,
        message: &SourcedMessage,
        latest_idle_for_sender: &HashMap<AgentName, usize>,
    ) -> bool {
        if !is_unread_idle_notification(&message.envelope) {
            return true;
        }

        idle_sender(&message.envelope)
            .and_then(|sender| latest_idle_for_sender.get(&sender))
            .map(|keep_index| *keep_index == index)
            .unwrap_or(true)
    }

    fn messages_from_idle_sender(messages: &[SourcedMessage]) -> HashMap<AgentName, usize> {
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

    fn idle_sender(message: &MessageEnvelope) -> Option<AgentName> {
        idle_notification_sender(message)
    }

    fn idle_notification_sender(message: &MessageEnvelope) -> Option<AgentName> {
        let value = match serde_json::from_str::<Value>(&message.text) {
            Ok(value) => value,
            Err(error) => {
                if message.text.contains("idle_notification") {
                    tracing::debug!(
                        %error,
                        recovery = "Repair or remove the malformed Claude idle-notification JSON. ATM will continue treating the record as a normal mailbox message.",
                        message_text = %message.text,
                        "ignoring malformed idle-notification JSON while classifying read surface"
                    );
                }
                return None;
            }
        };

        if value.get("type").and_then(Value::as_str) != Some("idle_notification") {
            return None;
        }

        match value.get("from").and_then(Value::as_str) {
            Some(sender) => match sender.parse() {
                Ok(sender) => Some(sender),
                Err(error) => {
                    tracing::debug!(
                        %error,
                        recovery = "Ensure Claude idle-notification payloads include a valid ATM agent name in `from`. ATM will continue treating the record as a normal mailbox message.",
                        sender,
                        message_text = %message.text,
                        "ignoring malformed idle-notification payload with invalid `from`"
                    );
                    None
                }
            },
            None => {
                tracing::debug!(
                    recovery = "Ensure Claude idle-notification payloads include a string `from` field. ATM will continue treating the record as a normal mailbox message.",
                    message_text = %message.text,
                    "ignoring malformed idle-notification payload missing string `from`"
                );
                None
            }
        }
    }

    fn classify_all(
        messages: Vec<SourcedMessage>,
        workflow_state: &workflow::WorkflowStateFile,
    ) -> Vec<ClassifiedMessage> {
        let projected = messages
            .iter()
            .map(|message| workflow::project_envelope(&message.envelope, workflow_state))
            .collect::<Vec<_>>();
        let thread_index = ThreadIndex::new(&projected);

        messages
            .into_iter()
            .zip(projected.iter().cloned())
            .map(|(message, projected)| {
                let effective =
                    metadata_selection::effective_display_envelope(&projected, &thread_index);
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

    fn sourced_message(index: usize, text: &str) -> SourcedMessage {
        SourcedMessage {
            envelope: message(text, AtmMessageId::new(), None, None, false),
            source_path: PathBuf::from(format!("{TEST_SENDER}.json")),
            source_index: index.into(),
        }
    }

    fn message_at(
        text: &str,
        message_id: AtmMessageId,
        parent_message_id: Option<AtmMessageId>,
        thread_mode: Option<ThreadMode>,
        read: bool,
        timestamp: IsoTimestamp,
    ) -> MessageEnvelope {
        MessageEnvelope {
            from: ROLE_TEAM_LEAD.parse::<AgentName>().expect("agent"),
            text: text.to_string(),
            timestamp,
            read,
            source_team: Some(TEST_TEAM.parse::<TeamName>().expect("team")),
            summary: None,
            message_id: Some(message_id),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id,
            thread_mode,
            expires_at: None,
            task_id: None,
            extra: Map::new(),
        }
    }

    fn message(
        text: &str,
        message_id: AtmMessageId,
        parent_message_id: Option<AtmMessageId>,
        thread_mode: Option<ThreadMode>,
        read: bool,
    ) -> MessageEnvelope {
        message_at(
            text,
            message_id,
            parent_message_id,
            thread_mode,
            read,
            IsoTimestamp::now(),
        )
    }

    #[test]
    fn idle_notification_sender_returns_none_for_malformed_json() {
        let malformed = format!(
            r#"{{"type":"idle_notification","from":"{}""#,
            ROLE_TEAM_LEAD
        );
        let message = sourced_message(0, &malformed);

        assert_eq!(idle_notification_sender(&message.envelope), None);
    }

    #[test]
    fn malformed_idle_notification_adjacent_to_valid_records_remains_readable_and_classifiable() {
        let workflow_state = workflow::WorkflowStateFile::default();
        let malformed = format!(
            r#"{{"type":"idle_notification","from":"{}""#,
            ROLE_TEAM_LEAD
        );
        let messages = vec![
            sourced_message(0, &malformed),
            sourced_message(1, "normal unread"),
        ];
        let query = ReadQuery {
            home_dir: PathBuf::new(),
            current_dir: PathBuf::new(),
            actor_override: None,
            target_address: None,
            team_override: None,
            selection_mode: ReadSelection::All,
            seen_state_filter: false,
            seen_state_update: false,
            ack_activation_mode: AckActivationMode::ReadOnly,
            message_id_filter: None,
            sender_filter: None,
            timestamp_filter: None,
            task_filter: None,
            contains_filter: None,
            timeout_secs: None,
        };

        let selected = std::panic::catch_unwind(|| {
            selected_after_filters(&messages, &workflow_state, &query, None)
        })
        .expect("malformed idle notification should not panic");

        assert_eq!(selected.len(), 2);
        let valid = selected
            .iter()
            .find(|message| message.envelope.text == "normal unread")
            .expect("valid record");
        assert_eq!(valid.class, MessageClass::Unread);
        assert_eq!(valid.bucket, DisplayBucket::Unread);

        let malformed = selected
            .iter()
            .find(|message| message.envelope.text == malformed)
            .expect("malformed record");
        assert_eq!(malformed.class, MessageClass::Unread);
        assert_eq!(malformed.bucket, DisplayBucket::Unread);
    }

    #[test]
    fn read_query_new_rejects_invalid_target_before_command_execution() {
        let tempdir = tempdir().expect("tempdir");
        let error = ReadQuery::new(
            tempdir.path().to_path_buf(),
            tempdir.path().to_path_buf(),
            Some(TEST_SENDER),
            Some("../evil"),
            Some(TEST_TEAM),
            ReadSelection::Actionable,
            false,
            false,
            AckActivationMode::ReadOnly,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect_err("invalid target");

        assert!(error.message.contains("agent name"));
    }

    #[test]
    fn read_query_new_rejects_invalid_actor_before_command_execution() {
        let tempdir = tempdir().expect("tempdir");
        let error = ReadQuery::new(
            tempdir.path().to_path_buf(),
            tempdir.path().to_path_buf(),
            Some("../evil"),
            None,
            Some(TEST_TEAM),
            ReadSelection::Actionable,
            false,
            false,
            AckActivationMode::ReadOnly,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect_err("invalid actor");

        assert!(error.message.contains("agent name"));
    }

    #[test]
    fn actionable_selection_prefers_terminal_thread_message() {
        let root_id = AtmMessageId::new();
        let terminal_id = AtmMessageId::new();
        let root_at =
            IsoTimestamp::from_datetime(chrono::Utc::now() - chrono::Duration::seconds(1));
        let terminal_at = IsoTimestamp::now();
        let source_files = vec![SourceFile {
            path: PathBuf::from("recipient.json"),
            messages: vec![
                message_at("root", root_id, None, None, false, root_at),
                message_at(
                    "detail",
                    terminal_id,
                    Some(root_id),
                    Some(ThreadMode::AddDetails),
                    false,
                    terminal_at,
                ),
            ],
        }];
        let query = ReadQuery {
            home_dir: PathBuf::new(),
            current_dir: PathBuf::new(),
            actor_override: None,
            target_address: None,
            team_override: None,
            selection_mode: ReadSelection::Actionable,
            seen_state_filter: false,
            seen_state_update: false,
            ack_activation_mode: AckActivationMode::ReadOnly,
            message_id_filter: None,
            sender_filter: None,
            timestamp_filter: None,
            task_filter: None,
            contains_filter: None,
            timeout_secs: None,
        };

        let (_counts, selected) = selection_state_for_source_files(
            &source_files,
            &workflow::WorkflowStateFile::default(),
            &query,
            None,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].envelope.message_id, Some(terminal_id));
    }

    #[test]
    fn actionable_selection_preserves_parent_context_for_add_details() {
        let root_id = AtmMessageId::new();
        let detail_id = AtmMessageId::new();
        let source_files = vec![SourceFile {
            path: PathBuf::from("recipient.json"),
            messages: vec![
                message("root context", root_id, None, None, false),
                message(
                    "follow-up detail",
                    detail_id,
                    Some(root_id),
                    Some(ThreadMode::AddDetails),
                    false,
                ),
            ],
        }];
        let query = ReadQuery {
            home_dir: PathBuf::new(),
            current_dir: PathBuf::new(),
            actor_override: None,
            target_address: None,
            team_override: None,
            selection_mode: ReadSelection::Actionable,
            seen_state_filter: false,
            seen_state_update: false,
            ack_activation_mode: AckActivationMode::ReadOnly,
            message_id_filter: None,
            sender_filter: None,
            timestamp_filter: None,
            task_filter: None,
            contains_filter: Some("root context".to_string()),
            timeout_secs: None,
        };

        let (_counts, selected) = selection_state_for_source_files(
            &source_files,
            &workflow::WorkflowStateFile::default(),
            &query,
            None,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].envelope.message_id, Some(detail_id));
        assert_eq!(
            selected[0].envelope.text,
            "root context\n\nfollow-up detail"
        );
    }

    #[test]
    fn actionable_selection_does_not_preserve_parent_context_for_supersede() {
        let root_id = AtmMessageId::new();
        let supersede_id = AtmMessageId::new();
        let source_files = vec![SourceFile {
            path: PathBuf::from("recipient.json"),
            messages: vec![
                message("root context", root_id, None, None, false),
                message(
                    "replacement instruction",
                    supersede_id,
                    Some(root_id),
                    Some(ThreadMode::Supersede),
                    false,
                ),
            ],
        }];
        let query = ReadQuery {
            home_dir: PathBuf::new(),
            current_dir: PathBuf::new(),
            actor_override: None,
            target_address: None,
            team_override: None,
            selection_mode: ReadSelection::Actionable,
            seen_state_filter: false,
            seen_state_update: false,
            ack_activation_mode: AckActivationMode::ReadOnly,
            message_id_filter: None,
            sender_filter: None,
            timestamp_filter: None,
            task_filter: None,
            contains_filter: Some("root context".to_string()),
            timeout_secs: None,
        };

        let (_counts, selected) = selection_state_for_source_files(
            &source_files,
            &workflow::WorkflowStateFile::default(),
            &query,
            None,
        );

        assert!(selected.is_empty());
    }

    #[test]
    fn successor_after_read_stays_actionable_for_add_details() {
        let root_id = AtmMessageId::new();
        let detail_id = AtmMessageId::new();
        let source_files = vec![SourceFile {
            path: PathBuf::from("recipient.json"),
            messages: vec![
                message("root context", root_id, None, None, true),
                message(
                    "follow-up detail",
                    detail_id,
                    Some(root_id),
                    Some(ThreadMode::AddDetails),
                    false,
                ),
            ],
        }];
        let query = ReadQuery {
            home_dir: PathBuf::new(),
            current_dir: PathBuf::new(),
            actor_override: None,
            target_address: None,
            team_override: None,
            selection_mode: ReadSelection::Actionable,
            seen_state_filter: false,
            seen_state_update: false,
            ack_activation_mode: AckActivationMode::ReadOnly,
            message_id_filter: None,
            sender_filter: None,
            timestamp_filter: None,
            task_filter: None,
            contains_filter: None,
            timeout_secs: None,
        };

        let (_counts, selected) = selection_state_for_source_files(
            &source_files,
            &workflow::WorkflowStateFile::default(),
            &query,
            None,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].envelope.message_id, Some(detail_id));
        assert_eq!(selected[0].bucket, DisplayBucket::Unread);
    }

    #[test]
    fn successor_after_read_stays_actionable_for_supersede() {
        let root_id = AtmMessageId::new();
        let supersede_id = AtmMessageId::new();
        let source_files = vec![SourceFile {
            path: PathBuf::from("recipient.json"),
            messages: vec![
                message("root context", root_id, None, None, true),
                message(
                    "replacement instruction",
                    supersede_id,
                    Some(root_id),
                    Some(ThreadMode::Supersede),
                    false,
                ),
            ],
        }];
        let query = ReadQuery {
            home_dir: PathBuf::new(),
            current_dir: PathBuf::new(),
            actor_override: None,
            target_address: None,
            team_override: None,
            selection_mode: ReadSelection::Actionable,
            seen_state_filter: false,
            seen_state_update: false,
            ack_activation_mode: AckActivationMode::ReadOnly,
            message_id_filter: None,
            sender_filter: None,
            timestamp_filter: None,
            task_filter: None,
            contains_filter: None,
            timeout_secs: None,
        };

        let (_counts, selected) = selection_state_for_source_files(
            &source_files,
            &workflow::WorkflowStateFile::default(),
            &query,
            None,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].envelope.message_id, Some(supersede_id));
        assert_eq!(selected[0].bucket, DisplayBucket::Unread);
    }

    #[test]
    fn read_ephemeral_message_is_hidden_outside_view_all() {
        let message_id = AtmMessageId::new();
        let expires_at =
            IsoTimestamp::from_datetime(chrono::Utc::now() + chrono::Duration::minutes(30));
        let messages = vec![SourcedMessage {
            envelope: MessageEnvelope {
                expires_at: Some(expires_at),
                ..message("ephemeral", message_id, None, None, true)
            },
            source_path: PathBuf::from("recipient.json"),
            source_index: 0.into(),
        }];
        let workflow_state = workflow::WorkflowStateFile::default();
        let actionable = ReadQuery {
            home_dir: PathBuf::new(),
            current_dir: PathBuf::new(),
            actor_override: None,
            target_address: None,
            team_override: None,
            selection_mode: ReadSelection::Actionable,
            seen_state_filter: false,
            seen_state_update: false,
            ack_activation_mode: AckActivationMode::ReadOnly,
            message_id_filter: None,
            sender_filter: None,
            timestamp_filter: None,
            task_filter: None,
            contains_filter: None,
            timeout_secs: None,
        };
        let all = ReadQuery {
            selection_mode: ReadSelection::All,
            ..actionable.clone()
        };

        assert!(selected_after_filters(&messages, &workflow_state, &actionable, None).is_empty());
        assert_eq!(
            selected_after_filters(&messages, &workflow_state, &all, None).len(),
            1
        );
    }

    #[test]
    fn expired_ephemeral_message_is_cleaned_from_all_views() {
        let message_id = AtmMessageId::new();
        let expires_at =
            IsoTimestamp::from_datetime(chrono::Utc::now() - chrono::Duration::minutes(1));
        let messages = vec![SourcedMessage {
            envelope: MessageEnvelope {
                expires_at: Some(expires_at),
                ..message("expired ephemeral", message_id, None, None, false)
            },
            source_path: PathBuf::from("recipient.json"),
            source_index: 0.into(),
        }];
        let workflow_state = workflow::WorkflowStateFile::default();
        let actionable = ReadQuery {
            home_dir: PathBuf::new(),
            current_dir: PathBuf::new(),
            actor_override: None,
            target_address: None,
            team_override: None,
            selection_mode: ReadSelection::Actionable,
            seen_state_filter: false,
            seen_state_update: false,
            ack_activation_mode: AckActivationMode::ReadOnly,
            message_id_filter: None,
            sender_filter: None,
            timestamp_filter: None,
            task_filter: None,
            contains_filter: None,
            timeout_secs: None,
        };
        let all = ReadQuery {
            selection_mode: ReadSelection::All,
            ..actionable.clone()
        };

        assert!(selected_after_filters(&messages, &workflow_state, &actionable, None).is_empty());
        assert!(selected_after_filters(&messages, &workflow_state, &all, None).is_empty());
    }

    #[test]
    fn task_filter_matches_logical_current_terminal_message_only() {
        let root_id = AtmMessageId::new();
        let terminal_id = AtmMessageId::new();
        let task_id: TaskId = "TASK-77".parse().expect("task id");
        let root_at =
            IsoTimestamp::from_datetime(chrono::Utc::now() - chrono::Duration::seconds(1));
        let terminal_at = IsoTimestamp::now();
        let source_files = vec![SourceFile {
            path: PathBuf::from("recipient.json"),
            messages: vec![
                MessageEnvelope {
                    task_id: Some(task_id.clone()),
                    ..message_at("root", root_id, None, None, false, root_at)
                },
                MessageEnvelope {
                    task_id: Some(task_id.clone()),
                    ..message_at(
                        "terminal",
                        terminal_id,
                        Some(root_id),
                        Some(ThreadMode::AddDetails),
                        false,
                        terminal_at,
                    )
                },
            ],
        }];
        let query = ReadQuery {
            home_dir: PathBuf::new(),
            current_dir: PathBuf::new(),
            actor_override: None,
            target_address: None,
            team_override: None,
            selection_mode: ReadSelection::All,
            seen_state_filter: false,
            seen_state_update: false,
            ack_activation_mode: AckActivationMode::ReadOnly,
            message_id_filter: None,
            sender_filter: None,
            timestamp_filter: None,
            task_filter: Some(task_id),
            contains_filter: None,
            timeout_secs: None,
        };

        let (_counts, selected) = selection_state_for_source_files(
            &source_files,
            &workflow::WorkflowStateFile::default(),
            &query,
            None,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].envelope.message_id, Some(terminal_id));
    }

    #[test]
    fn exact_message_id_bypasses_logical_current_collapse() {
        let root_id = AtmMessageId::new();
        let terminal_id = AtmMessageId::new();
        let source_files = vec![SourceFile {
            path: PathBuf::from("recipient.json"),
            messages: vec![
                message("root", root_id, None, None, false),
                message(
                    "terminal",
                    terminal_id,
                    Some(root_id),
                    Some(ThreadMode::AddDetails),
                    false,
                ),
            ],
        }];
        let query = ReadQuery {
            home_dir: PathBuf::new(),
            current_dir: PathBuf::new(),
            actor_override: None,
            target_address: None,
            team_override: None,
            selection_mode: ReadSelection::All,
            seen_state_filter: false,
            seen_state_update: false,
            ack_activation_mode: AckActivationMode::ReadOnly,
            message_id_filter: Some(root_id),
            sender_filter: None,
            timestamp_filter: None,
            task_filter: None,
            contains_filter: None,
            timeout_secs: None,
        };

        let (_counts, selected) = selection_state_for_source_files(
            &source_files,
            &workflow::WorkflowStateFile::default(),
            &query,
            None,
        );

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].envelope.message_id, Some(root_id));
    }
}
