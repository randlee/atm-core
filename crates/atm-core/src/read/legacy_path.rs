use std::collections::HashMap;

use serde_json::Value;
use tracing::debug;

use crate::mailbox::source::{SourceFile, SourcedMessage};
use crate::mailbox::surface::dedupe_message_id_surface;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::schema::MessageEnvelope;
use crate::service_runtime::RetainedServiceRuntime;
use crate::service_runtime_store::RetainedMailboxRuntime;
use crate::threading::ThreadIndex;
use crate::types::{AgentName, CommandAction, IsoTimestamp, TeamName};
use crate::workflow;

use super::metadata_selection::{
    apply_filters, bucket_counts_for, effective_display_envelope, logical_current_messages,
    select_messages, sort_and_limit_selected,
};
use super::{
    BucketCounts, ClassifiedMessage, ReadOutcome, ReadQuery, apply_display_mutations,
    displayed_messages_require_mutation, output_messages_from_selection, state, wait,
};

pub(crate) fn read_mail_legacy_path<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    query: ReadQuery,
    observability: &dyn ObservabilityPort,
    runtime: &R,
    actor: AgentName,
    actor_team: Option<TeamName>,
    target: crate::mailbox::source::ResolvedTarget,
    seen_watermark: Option<IsoTimestamp>,
) -> Result<ReadOutcome, crate::error::AtmError> {
    let own_inbox = actor == target.agent && actor_team.as_deref() == Some(target.team.as_str());
    let workflow_path =
        runtime.workflow_state_path(&query.home_dir, &target.team, &target.agent)?;
    let mut workflow_state =
        runtime.load_workflow_state(&query.home_dir, &target.team, &target.agent)?;
    let mut source_files =
        runtime.observe_source_files(&query.home_dir, &target.team, &target.agent)?;
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
                    dedupe_message_id_surface(
                        merged_surface(&runtime.observe_source_files(
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
                runtime.load_workflow_state(&query.home_dir, &target.team, &target.agent)?;
            source_files =
                runtime.observe_source_files(&query.home_dir, &target.team, &target.agent)?;
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

    let match_count = selected.len();
    sort_and_limit_selected(&mut selected, Some(1));
    let mutation_needed = displayed_messages_require_mutation(&selected);

    let (mutation_applied, output_message, bucket_counts, selected_message_id, match_count) =
        if timed_out || selected.is_empty() || !mutation_needed {
            (
                false,
                output_messages_from_selection(&selected, &source_files, &workflow_state)
                    .into_iter()
                    .next(),
                bucket_counts,
                selected
                    .first()
                    .and_then(|message| message.envelope.message_id),
                match_count,
            )
        } else {
            runtime.with_locked_source_files(
                &query.home_dir,
                &target.team,
                &target.agent,
                [workflow_path],
                runtime.mailbox_timeout_policy().workflow_lock_timeout,
                |_source_paths, source_files| {
                    let mut workflow_state = runtime.load_workflow_state(
                        &query.home_dir,
                        &target.team,
                        &target.agent,
                    )?;
                    let (bucket_counts, mut selected) = selection_state_for_source_files(
                        source_files,
                        &workflow_state,
                        &query,
                        seen_watermark,
                    );
                    let match_count = selected.len();
                    sort_and_limit_selected(&mut selected, Some(1));
                    let mutation = apply_display_mutations(
                        source_files,
                        &mut workflow_state,
                        &selected,
                        query.ack_activation_mode,
                        own_inbox,
                    );
                    if mutation.mailbox_changed {
                        runtime.commit_source_files(source_files)?;
                    }
                    if mutation.workflow_changed {
                        runtime.save_workflow_state(
                            &query.home_dir,
                            &target.team,
                            &target.agent,
                            &workflow_state,
                        )?;
                    }
                    let output_message =
                        output_messages_from_selection(&selected, source_files, &workflow_state)
                            .into_iter()
                            .next();
                    Ok((
                        mutation.any_changed,
                        output_message,
                        bucket_counts,
                        selected
                            .first()
                            .and_then(|message| message.envelope.message_id),
                        match_count,
                    ))
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

pub(crate) fn selection_state_for_source_files(
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
        let logical_current = logical_current_messages(classified_all);
        let bucket_counts = bucket_counts_for(&logical_current);
        return (bucket_counts, selected);
    }
    let logical_current = logical_current_messages(classified_all);
    let bucket_counts = bucket_counts_for(&logical_current);
    let filtered = apply_filters(
        logical_current,
        query.sender_filter.as_ref(),
        query.timestamp_filter,
        query.task_filter.as_ref(),
        query.contains_filter.as_deref(),
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

pub(crate) fn idle_notification_sender(message: &MessageEnvelope) -> Option<AgentName> {
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
        Some(sender) => match sender.parse() {
            Ok(sender) => Some(sender),
            Err(error) => {
                debug!(
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
    let projected = messages
        .iter()
        .map(|message| workflow::project_envelope(&message.envelope, workflow_state))
        .collect::<Vec<_>>();
    let thread_index = ThreadIndex::new(&projected);

    messages
        .into_iter()
        .zip(projected.iter().cloned())
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

pub(crate) fn selected_after_filters(
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
    let filtered = apply_filters(
        logical_current_messages(classified),
        query.sender_filter.as_ref(),
        query.timestamp_filter,
        query.task_filter.as_ref(),
        query.contains_filter.as_deref(),
    );
    select_messages(&filtered, query.selection_mode, seen_watermark)
}
