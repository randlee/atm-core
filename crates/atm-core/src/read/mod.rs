pub(crate) mod filters;
pub(crate) mod projection;
pub(crate) mod seen_state;
pub(crate) mod state;
pub(crate) mod wait;

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::address::AgentAddress;
use crate::config;
use crate::error::AtmError;
use crate::home;
use crate::identity;
use crate::inbox_ingress::{InboxIngestStore, InboxIngress};
use crate::mail_store::{AckStateRecord, MailStore, StoredMessageRecord, VisibilityStateRecord};
use crate::mailbox;
use crate::mailbox::source::{SourceFile, SourcedMessage, resolve_target};
use crate::mailbox::surface::dedupe_legacy_message_id_surface;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::schema::{AtmMetadataFields, MessageEnvelope};
use crate::store::MessageKey;
use crate::task_store::TaskStore;
use crate::types::{
    AckActivationMode, AgentName, DisplayBucket, IsoTimestamp, MessageClass, ReadSelection,
    SourceIndex, TeamName,
};
use crate::workflow;
use projection::{
    apply_filters, apply_idle_notification_dedup, apply_store_display_mutations, bucket_counts_for,
    classify_all, idle_sender, is_unread_idle_notification, merged_surface,
    selection_state_for_source_files,
};

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
    pub sender_filter: Option<AgentName>,
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
        sender_filter: Option<AgentName>,
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

#[derive(Debug, Clone, Serialize)]
pub struct BucketCounts {
    pub unread: usize,
    pub pending_ack: usize,
    pub history: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClassifiedMessage {
    #[serde(skip)]
    message_key: Option<MessageKey>,
    #[serde(skip)]
    source_index: SourceIndex,
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
    pub team: TeamName,
    pub agent: AgentName,
    pub selection_mode: ReadSelection,
    pub history_collapsed: bool,
    pub mutation_applied: bool,
    pub count: usize,
    pub messages: Vec<ClassifiedMessage>,
    pub bucket_counts: BucketCounts,
}

pub trait ReadStore: InboxIngestStore + MailStore + TaskStore {}

impl<T> ReadStore for T where T: InboxIngestStore + MailStore + TaskStore + ?Sized {}

#[deprecated(note = "transitional path; use read_mail_via_store")]
pub fn read_mail(
    query: ReadQuery,
    observability: &dyn ObservabilityPort,
) -> Result<ReadOutcome, AtmError> {
    let config = config::load_config(&query.current_dir)?;
    let actor = identity::resolve_actor_identity(query.actor_override.as_deref(), config.as_ref())?;
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
        team: outcome.team.clone(),
        agent: outcome.agent.clone(),
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

pub fn read_mail_via_store(
    query: ReadQuery,
    store: &dyn ReadStore,
    ingress: &dyn InboxIngress,
    observability: &dyn ObservabilityPort,
) -> Result<ReadOutcome, AtmError> {
    let config = config::load_config(&query.current_dir)?;
    let actor = identity::resolve_actor_identity(query.actor_override.as_deref(), config.as_ref())?;
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

    ingress.ingest_mailbox_state(
        &query.home_dir,
        &target.team,
        &target.agent,
        store,
        observability,
    )?;

    let (mut bucket_counts, mut selected) = selection_state_for_store_messages(
        store,
        &target.team,
        &target.agent,
        &query,
        seen_watermark,
    )?;
    let mut timed_out = false;

    if selected.is_empty()
        && let Some(timeout_secs) = query.timeout_secs
    {
        let wait_satisfied = wait::wait_for_eligible_message(
            timeout_secs,
            || {
                ingress.ingest_mailbox_state(
                    &query.home_dir,
                    &target.team,
                    &target.agent,
                    store,
                    observability,
                )?;
                project_messages_for_recipient(store, &target.team, &target.agent)
            },
            |messages| {
                !selected_after_filters_projected(messages, &query, seen_watermark).is_empty()
            },
        )?;

        if wait_satisfied {
            (bucket_counts, selected) = selection_state_for_store_messages(
                store,
                &target.team,
                &target.agent,
                &query,
                seen_watermark,
            )?;
        } else {
            timed_out = true;
        }
    }

    sort_and_limit_selected(&mut selected, query.limit);
    let mutation_applied =
        apply_store_display_mutations(store, &selected, query.ack_activation_mode, own_inbox)?;
    let output_messages = selected.clone();

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
        team: outcome.team.clone(),
        agent: outcome.agent.clone(),
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

fn selection_state_for_store_messages(
    store: &dyn ReadStore,
    team: &TeamName,
    agent: &AgentName,
    query: &ReadQuery,
    seen_watermark: Option<IsoTimestamp>,
) -> Result<(BucketCounts, Vec<ClassifiedMessage>), AtmError> {
    let classified_all =
        classify_projected_messages(project_messages_for_recipient(store, team, agent)?);
    let bucket_counts = bucket_counts_for(&classified_all);
    let filtered = apply_filters(
        classified_all.clone(),
        query.sender_filter.as_ref(),
        query.timestamp_filter,
    );
    let selected = select_messages(&filtered, query.selection_mode, seen_watermark);
    Ok((bucket_counts, selected))
}

fn selected_after_filters_projected(
    messages: &[ClassifiedMessage],
    query: &ReadQuery,
    seen_watermark: Option<IsoTimestamp>,
) -> Vec<ClassifiedMessage> {
    let filtered = apply_filters(
        messages.to_vec(),
        query.sender_filter.as_ref(),
        query.timestamp_filter,
    );
    select_messages(&filtered, query.selection_mode, seen_watermark)
}

pub(crate) fn project_messages_for_recipient(
    store: &dyn ReadStore,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<ClassifiedMessage>, AtmError> {
    let messages = store
        .list_messages_for_recipient(team, agent)
        .map_err(|error| {
            AtmError::mailbox_read("failed to project SQLite mailbox state").with_source(error)
        })?;
    let mut projected = Vec::new();
    for (index, message) in messages.into_iter().enumerate() {
        let ack_state = store
            .load_ack_state(&message.message_key)
            .map_err(|error| {
                AtmError::mailbox_read("failed to load projected ack state").with_source(error)
            })?;
        let visibility = store
            .load_visibility(&message.message_key)
            .map_err(|error| {
                AtmError::mailbox_read("failed to load projected visibility state")
                    .with_source(error)
            })?;
        if visibility
            .as_ref()
            .and_then(|state| state.cleared_at)
            .is_some()
        {
            continue;
        }
        let task_id = store
            .load_tasks_for_message(&message.message_key)
            .map_err(|error| {
                AtmError::mailbox_read("failed to load projected task linkage").with_source(error)
            })?
            .into_iter()
            .next()
            .map(|task| task.task_id);
        let envelope =
            envelope_from_store_record(&message, ack_state.as_ref(), visibility.as_ref(), task_id)?;
        projected.push(ClassifiedMessage {
            message_key: Some(message.message_key.clone()),
            source_index: index.into(),
            source_path: PathBuf::from(format!("sqlite:{}", message.message_key)),
            bucket: DisplayBucket::Unread,
            class: MessageClass::Unread,
            envelope,
        });
    }
    Ok(apply_idle_notification_dedup_projected(projected))
}

fn classify_projected_messages(messages: Vec<ClassifiedMessage>) -> Vec<ClassifiedMessage> {
    messages
        .into_iter()
        .map(|mut message| {
            message.class = state::classify_message(&message.envelope);
            message.bucket = state::display_bucket_for_class(message.class);
            message
        })
        .collect()
}

fn apply_idle_notification_dedup_projected(
    messages: Vec<ClassifiedMessage>,
) -> Vec<ClassifiedMessage> {
    let latest_idle_for_sender = messages_from_idle_sender_projected(&messages);
    messages
        .into_iter()
        .enumerate()
        .filter_map(|(index, message)| {
            dedupe_idle_notifications_projected(index, &message, &latest_idle_for_sender)
                .then_some(message)
        })
        .collect()
}

fn messages_from_idle_sender_projected(
    messages: &[ClassifiedMessage],
) -> HashMap<AgentName, usize> {
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

fn dedupe_idle_notifications_projected(
    index: usize,
    message: &ClassifiedMessage,
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

fn envelope_from_store_record(
    message: &StoredMessageRecord,
    ack_state: Option<&AckStateRecord>,
    visibility: Option<&VisibilityStateRecord>,
    task_id: Option<crate::types::TaskId>,
) -> Result<MessageEnvelope, AtmError> {
    let mut extra = Map::new();
    let mut metadata = message
        .raw_metadata_json
        .as_deref()
        .map(serde_json::from_str::<Value>)
        .transpose()
        .map_err(|error| {
            AtmError::mailbox_read("failed to decode stored metadata projection").with_source(error)
        })?
        .unwrap_or_else(|| Value::Object(Map::new()));

    if !metadata.is_object() {
        metadata = Value::Object(Map::new());
    }
    let Some(metadata_object) = metadata.as_object_mut() else {
        unreachable!("metadata normalized to object")
    };
    let atm = metadata_object
        .entry("atm".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !atm.is_object() {
        *atm = Value::Object(Map::new());
    }
    let Some(atm_object) = atm.as_object_mut() else {
        unreachable!("atm metadata normalized to object")
    };
    let atm_fields = AtmMetadataFields {
        message_id: message.atm_message_id,
        source_team: message.sender_team.clone(),
        from_identity: message.sender_canonical.clone(),
        pending_ack_at: ack_state.and_then(|state| state.pending_ack_at),
        acknowledged_at: ack_state.and_then(|state| state.acknowledged_at),
        acknowledges_message_id: None,
        task_id: task_id.clone(),
        alert_kind: None,
        missing_config_path: None,
        extra: atm_object.clone(),
    };
    let atm_value = serde_json::to_value(atm_fields).map_err(|error| {
        AtmError::mailbox_read("failed to encode stored ATM metadata projection").with_source(error)
    })?;
    *atm_object = atm_value.as_object().cloned().unwrap_or_default();
    extra.insert("metadata".to_string(), metadata);

    Ok(MessageEnvelope {
        from: message
            .sender_canonical
            .clone()
            .unwrap_or_else(|| AgentName::from_validated(message.sender_display.clone())),
        text: message.body.clone(),
        timestamp: message.created_at,
        read: visibility.and_then(|state| state.read_at).is_some(),
        source_team: message.sender_team.clone(),
        summary: message.summary.clone(),
        message_id: message.legacy_message_id,
        pending_ack_at: ack_state.and_then(|state| state.pending_ack_at),
        acknowledged_at: ack_state.and_then(|state| state.acknowledged_at),
        acknowledges_message_id: None,
        task_id,
        extra,
    })
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
        query.sender_filter.as_ref(),
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
            message_key: None,
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

    use super::{ReadQuery, selected_after_filters};
    use crate::mailbox::source::SourcedMessage;
    use crate::read::projection::idle_notification_sender;
    use crate::schema::{LegacyMessageId, MessageEnvelope};
    use crate::types::{
        AckActivationMode, AgentName, DisplayBucket, IsoTimestamp, MessageClass, ReadSelection,
        TeamName,
    };
    use crate::workflow;

    fn sourced_message(index: usize, text: &str) -> SourcedMessage {
        SourcedMessage {
            envelope: MessageEnvelope {
                from: "team-lead".parse::<AgentName>().expect("agent"),
                text: text.to_string(),
                timestamp: IsoTimestamp::now(),
                read: false,
                source_team: Some("atm-dev".parse::<TeamName>().expect("team")),
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
