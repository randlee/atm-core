pub(crate) mod filters;
pub(crate) mod seen_state;
pub(crate) mod state;
pub(crate) mod wait;

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value;
use tracing::debug;

use crate::address::AgentAddress;
use crate::config;
use crate::error::AtmError;
use crate::home;
use crate::identity;
use crate::mailbox;
use crate::mailbox::source::{SourceFile, SourcedMessage, resolve_target};
use crate::mailbox::surface::dedupe_legacy_message_id_surface;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::schema::MessageEnvelope;
use crate::types::{
    AckActivationMode, AgentName, DisplayBucket, IsoTimestamp, MessageClass, ReadSelection,
    SourceIndex, TeamName,
};
use crate::workflow;

/// Parameters for querying and optionally mutating one mailbox display surface.
#[derive(Debug, Clone)]
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
    pub limit: Option<usize>,
    pub sender_filter: Option<String>,
    pub timestamp_filter: Option<IsoTimestamp>,
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
        limit: Option<usize>,
        sender_filter: Option<String>,
        timestamp_filter: Option<IsoTimestamp>,
        timeout_secs: Option<u64>,
    ) -> Result<Self, AtmError> {
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
            limit,
            sender_filter,
            timestamp_filter,
            timeout_secs,
        })
    }
}

/// Bucket counts for one classified mailbox surface.
#[derive(Debug, Clone, Serialize)]
pub struct BucketCounts {
    pub unread: usize,
    pub pending_ack: usize,
    pub history: usize,
}

/// One mailbox message classified for ATM display output.
#[derive(Debug, Clone, Serialize)]
pub struct ClassifiedMessage {
    #[serde(skip)]
    source_index: SourceIndex,
    #[serde(skip)]
    source_path: PathBuf,
    pub bucket: DisplayBucket,
    pub class: MessageClass,
    #[serde(flatten)]
    pub envelope: MessageEnvelope,
}

/// Result of one mailbox read/query command.
#[derive(Debug, Clone, Serialize)]
pub struct ReadOutcome {
    pub action: &'static str,
    pub team: TeamName,
    pub agent: AgentName,
    pub selection_mode: ReadSelection,
    pub history_collapsed: bool,
    pub mutation_applied: bool,
    pub count: usize,
    pub messages: Vec<ClassifiedMessage>,
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
    let config = config::load_config(&query.current_dir)?;
    let actor = AgentName::from_validated(identity::resolve_actor_identity(
        query.actor_override.as_deref(),
        config.as_ref(),
    )?);
    let actor_team = config::resolve_team(query.team_override.as_deref(), config.as_ref());
    let target = resolve_target(
        query.target_address.as_ref(),
        &actor,
        query.team_override.as_ref(),
        config.as_ref(),
    )?;

    let team_dir = home::team_dir_from_home(&query.home_dir, &target.team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&target.team).with_recovery(
            "Create the team config for the requested team or target a different team before retrying `atm read`.",
        ));
    }

    let team_config = config::load_team_config(&team_dir)?;
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

    let own_inbox = actor == target.agent && actor_team.as_deref() == Some(target.team.as_str());
    let seen_watermark = if query.seen_state_filter && query.selection_mode != ReadSelection::All {
        seen_state::load_seen_watermark(&query.home_dir, &target.team, &target.agent)?
    } else {
        None
    };

    let workflow_path =
        home::workflow_state_path_from_home(&query.home_dir, &target.team, &target.agent)?;
    let mut workflow_state =
        workflow::load_workflow_state(&query.home_dir, &target.team, &target.agent)?;
    let mut source_files =
        mailbox::store::observe_source_files(&query.home_dir, &target.team, &target.agent)?;
    let (mut bucket_counts, mut selected) =
        selection_state_for_source_files(&source_files, &workflow_state, &query, seen_watermark);
    let mut timed_out = false;

    if selected.is_empty()
        && let Some(timeout_secs) = query.timeout_secs
    {
        let wait_satisfied = wait::wait_for_eligible_message(
            timeout_secs,
            || {
                Ok(apply_idle_notification_dedup(
                    dedupe_legacy_message_id_surface(
                        merged_surface(&mailbox::store::observe_source_files(
                            &query.home_dir,
                            &target.team,
                            &target.agent,
                        )?),
                        |message: &SourcedMessage| message.envelope.message_id,
                        |message: &SourcedMessage| message.envelope.timestamp,
                    ),
                    &workflow_state,
                ))
            },
            |messages| {
                !selected_after_filters(messages, &workflow_state, &query, seen_watermark)
                    .is_empty()
            },
        )?;

        if wait_satisfied {
            workflow_state =
                workflow::load_workflow_state(&query.home_dir, &target.team, &target.agent)?;
            source_files =
                mailbox::store::observe_source_files(&query.home_dir, &target.team, &target.agent)?;
            (bucket_counts, selected) = selection_state_for_source_files(
                &source_files,
                &workflow_state,
                &query,
                seen_watermark,
            );
        } else {
            timed_out = true;
        }
    }

    sort_and_limit_selected(&mut selected, query.limit);
    let mutation_needed = displayed_messages_require_mutation(&selected);

    let (mutation_applied, output_messages, bucket_counts) = if timed_out
        || selected.is_empty()
        || !mutation_needed
    {
        (
            false,
            output_messages_from_selection(&selected, &source_files, &workflow_state),
            bucket_counts,
        )
    } else {
        mailbox::store::with_locked_source_files(
            &query.home_dir,
            &target.team,
            &target.agent,
            [workflow_path],
            mailbox::lock::default_lock_timeout(),
            |_source_paths, source_files| {
                let mut workflow_state =
                    workflow::load_workflow_state(&query.home_dir, &target.team, &target.agent)?;
                let (bucket_counts, mut selected) = selection_state_for_source_files(
                    source_files,
                    &workflow_state,
                    &query,
                    seen_watermark,
                );
                sort_and_limit_selected(&mut selected, query.limit);
                let mutation = apply_display_mutations(
                    source_files,
                    &mut workflow_state,
                    &selected,
                    query.ack_activation_mode,
                    own_inbox,
                );
                if mutation.mailbox_changed {
                    mailbox::store::commit_source_files(source_files)?;
                }
                if mutation.workflow_changed {
                    workflow::save_workflow_state(
                        &query.home_dir,
                        &target.team,
                        &target.agent,
                        &workflow_state,
                    )?;
                }
                let output_messages =
                    output_messages_from_selection(&selected, source_files, &workflow_state);
                Ok((mutation.any_changed, output_messages, bucket_counts))
            },
        )?
    };

    if query.seen_state_update
        && !selected.is_empty()
        && let Some(latest_timestamp) = selected
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

    let history_collapsed = query.selection_mode != ReadSelection::All
        && query.selection_mode != ReadSelection::ActionableWithHistory
        && bucket_counts.history > 0;

    let outcome = ReadOutcome {
        action: "read",
        team: target.team.clone(),
        agent: target.agent.clone(),
        selection_mode: query.selection_mode,
        history_collapsed,
        mutation_applied,
        count: output_messages.len(),
        messages: output_messages,
        bucket_counts,
    };

    let _ = observability.emit(CommandEvent {
        command: "read",
        action: "read",
        outcome: if timed_out { "timeout" } else { "ok" },
        team: outcome.team.to_string(),
        agent: outcome.agent.to_string(),
        sender: actor.to_string(),
        message_id: None,
        requires_ack: false,
        dry_run: false,
        task_id: None,
        error_code: None,
        error_message: None,
    });

    Ok(outcome)
}

fn selection_state_for_source_files(
    source_files: &[SourceFile],
    workflow_state: &workflow::WorkflowStateFile,
    query: &ReadQuery,
    seen_watermark: Option<IsoTimestamp>,
) -> (BucketCounts, Vec<ClassifiedMessage>) {
    let classified_all = classify_all(
        apply_idle_notification_dedup(
            dedupe_legacy_message_id_surface(
                merged_surface(source_files),
                |message: &SourcedMessage| message.envelope.message_id,
                |message: &SourcedMessage| message.envelope.timestamp,
            ),
            workflow_state,
        ),
        workflow_state,
    );
    let bucket_counts = bucket_counts_for(&classified_all);
    let filtered = apply_filters(
        classified_all.clone(),
        query.sender_filter.as_deref(),
        query.timestamp_filter,
    );
    let selected = select_messages(&filtered, query.selection_mode, seen_watermark);
    (bucket_counts, selected)
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
    let value = match serde_json::from_str::<Value>(&message.text) {
        Ok(value) => value,
        Err(error) => {
            if message.text.contains("idle_notification") {
                debug!(
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
        Some(sender) => Some(sender.to_string()),
        None => {
            debug!(
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
    messages
        .into_iter()
        .map(|message| {
            let projected = workflow::project_envelope(&message.envelope, workflow_state);
            let class = state::classify_message(&projected);
            let bucket = state::display_bucket_for_class(class);

            ClassifiedMessage {
                source_index: message.source_index,
                source_path: message.source_path,
                bucket,
                class,
                envelope: projected,
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
    workflow_state: &workflow::WorkflowStateFile,
    query: &ReadQuery,
    seen_watermark: Option<IsoTimestamp>,
) -> Vec<ClassifiedMessage> {
    let classified = classify_all(messages.to_vec(), workflow_state);
    let filtered = apply_filters(
        classified,
        query.sender_filter.as_deref(),
        query.timestamp_filter,
    );
    select_messages(&filtered, query.selection_mode, seen_watermark)
}

fn sort_and_limit_selected(selected: &mut Vec<ClassifiedMessage>, limit: Option<usize>) {
    selected.sort_by(|left, right| {
        right
            .envelope
            .timestamp
            .cmp(&left.envelope.timestamp)
            .then_with(|| right.envelope.message_id.cmp(&left.envelope.message_id))
            .then_with(|| right.source_index.cmp(&left.source_index))
    });

    if let Some(limit) = limit {
        selected.truncate(limit);
    }
}

fn output_messages_from_selection(
    selected: &[ClassifiedMessage],
    source_files: &[SourceFile],
    workflow_state: &workflow::WorkflowStateFile,
) -> Vec<ClassifiedMessage> {
    selected
        .iter()
        .cloned()
        .map(|selected_message| ClassifiedMessage {
            source_index: selected_message.source_index,
            source_path: selected_message.source_path.clone(),
            bucket: selected_message.bucket,
            class: selected_message.class,
            envelope: source_files
                .iter()
                .find(|source| source.path == selected_message.source_path)
                .and_then(|source| source.messages.get(selected_message.source_index.get()))
                .map(|message| workflow::project_envelope(message, workflow_state))
                .unwrap_or(selected_message.envelope),
        })
        .collect()
}

#[derive(Debug, Default, Clone, Copy)]
struct DisplayMutationResult {
    any_changed: bool,
    mailbox_changed: bool,
    workflow_changed: bool,
}

fn displayed_messages_require_mutation(displayed_messages: &[ClassifiedMessage]) -> bool {
    displayed_messages
        .iter()
        .any(|message| !message.envelope.read)
}

fn apply_display_mutations(
    source_files: &mut [SourceFile],
    workflow_state: &mut workflow::WorkflowStateFile,
    displayed_messages: &[ClassifiedMessage],
    ack_activation_mode: AckActivationMode,
    own_inbox: bool,
) -> DisplayMutationResult {
    let mut mutation = DisplayMutationResult::default();
    let promote_unread =
        own_inbox && ack_activation_mode == AckActivationMode::PromoteDisplayedUnread;
    let now = IsoTimestamp::now();

    for message in displayed_messages {
        let transitioned = transition_displayed_message(message, promote_unread, now);
        let updated = transitioned.into_envelope();
        if updated == message.envelope {
            continue;
        }
        if workflow::apply_projected_state(workflow_state, &message.envelope, &updated) {
            mutation.any_changed = true;
            mutation.workflow_changed = true;
            continue;
        }
        if let Some(source_file) = source_files
            .iter_mut()
            .find(|source| source.path == message.source_path)
            && let Some(stored) = source_file.messages.get_mut(message.source_index.get())
        {
            *stored = updated;
            mutation.any_changed = true;
            mutation.mailbox_changed = true;
        }
    }

    mutation
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
    use std::path::PathBuf;

    use serde_json::Map;
    use tempfile::tempdir;

    use super::{ReadQuery, idle_notification_sender, selected_after_filters};
    use crate::mailbox::source::SourcedMessage;
    use crate::schema::{LegacyMessageId, MessageEnvelope};
    use crate::types::{
        AckActivationMode, DisplayBucket, IsoTimestamp, MessageClass, ReadSelection,
    };
    use crate::workflow;

    fn sourced_message(index: usize, text: &str) -> SourcedMessage {
        SourcedMessage {
            envelope: MessageEnvelope {
                from: "team-lead".to_string(),
                text: text.to_string(),
                timestamp: IsoTimestamp::now(),
                read: false,
                source_team: Some("atm-dev".to_string()),
                summary: None,
                message_id: Some(LegacyMessageId::new()),
                pending_ack_at: None,
                acknowledged_at: None,
                acknowledges_message_id: None,
                task_id: None,
                extra: Map::new(),
            },
            source_path: PathBuf::from("arch-ctm.json"),
            source_index: index.into(),
        }
    }

    #[test]
    fn idle_notification_sender_returns_none_for_malformed_json() {
        let message = sourced_message(0, r#"{"type":"idle_notification","from":"team-lead""#);

        assert_eq!(idle_notification_sender(&message.envelope), None);
    }

    #[test]
    fn malformed_idle_notification_adjacent_to_valid_records_remains_readable_and_classifiable() {
        let workflow_state = workflow::WorkflowStateFile::default();
        let messages = vec![
            sourced_message(0, r#"{"type":"idle_notification","from":"team-lead""#),
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
            limit: None,
            sender_filter: None,
            timestamp_filter: None,
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
            .find(|message| {
                message.envelope.text == r#"{"type":"idle_notification","from":"team-lead""#
            })
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
            Some("arch-ctm"),
            Some("../evil"),
            Some("atm-dev"),
            ReadSelection::Actionable,
            false,
            false,
            AckActivationMode::ReadOnly,
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
            Some("atm-dev"),
            ReadSelection::Actionable,
            false,
            false,
            AckActivationMode::ReadOnly,
            None,
            None,
            None,
            None,
        )
        .expect_err("invalid actor");

        assert!(error.message.contains("agent name"));
    }
}
