use std::collections::HashMap;

use serde_json::Value;
use tracing::debug;

use super::*;
use crate::mailbox::surface::dedupe_legacy_message_id_surface;
use crate::types::AgentName;

pub(crate) fn apply_store_display_mutations(
    store: &dyn ReadStore,
    displayed_messages: &[ClassifiedMessage],
    ack_activation_mode: AckActivationMode,
    own_inbox: bool,
) -> Result<bool, AtmError> {
    if displayed_messages.is_empty() {
        return Ok(false);
    }
    let promote_unread =
        own_inbox && ack_activation_mode == AckActivationMode::PromoteDisplayedUnread;
    let now = IsoTimestamp::now();
    let mut visibility_updates = Vec::new();
    let mut ack_updates = Vec::new();

    for message in displayed_messages {
        let transitioned = transition_displayed_message(message, promote_unread, now);
        let updated = transitioned.into_envelope();
        if updated == message.envelope {
            continue;
        }
        let message_key = projected_message_key(message)?;
        visibility_updates.push(VisibilityStateRecord {
            message_key: message_key.clone(),
            read_at: updated.read.then_some(now),
            cleared_at: None,
        });
        if updated.pending_ack_at != message.envelope.pending_ack_at
            || updated.acknowledged_at != message.envelope.acknowledged_at
        {
            ack_updates.push(AckStateRecord {
                message_key,
                pending_ack_at: updated.pending_ack_at,
                acknowledged_at: updated.acknowledged_at,
                ack_reply_message_key: None,
                ack_reply_team: None,
                ack_reply_agent: None,
            });
        }
    }

    if visibility_updates.is_empty() && ack_updates.is_empty() {
        return Ok(false);
    }
    if !visibility_updates.is_empty() {
        store
            .upsert_visibility_batch(&visibility_updates)
            .map_err(|error| {
                AtmError::mailbox_write("failed to persist read projection state")
                    .with_source(error)
            })?;
    }
    if !ack_updates.is_empty() {
        store
            .upsert_ack_state_batch(&ack_updates)
            .map_err(|error| {
                AtmError::mailbox_write("failed to persist pending-ack projection state")
                    .with_source(error)
            })?;
    }
    Ok(true)
}

pub(crate) fn projected_message_key(message: &ClassifiedMessage) -> Result<MessageKey, AtmError> {
    if let Some(message_key) = &message.message_key {
        return Ok(message_key.clone());
    }
    projected_message_key_from_source_path(&message.source_path)
}

pub(crate) fn projected_message_key_from_source_path(
    source_path: &std::path::Path,
) -> Result<MessageKey, AtmError> {
    let path = source_path.to_string_lossy();
    let Some(raw_message_key) = path.strip_prefix("sqlite:") else {
        return Err(AtmError::mailbox_read(format!(
            "projected source path must start with sqlite:, got {path}"
        )));
    };
    raw_message_key.parse().map_err(|error| {
        AtmError::mailbox_read(format!(
            "projected sqlite source path contained invalid message_key `{raw_message_key}`: {error}"
        ))
    })
}

pub(crate) fn selection_state_for_source_files(
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
        query.sender_filter.as_ref(),
        query.timestamp_filter,
    );
    let selected = select_messages(&filtered, query.selection_mode, seen_watermark);
    (bucket_counts, selected)
}

pub(crate) fn merged_surface(source_files: &[SourceFile]) -> Vec<SourcedMessage> {
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

pub(crate) fn apply_idle_notification_dedup(
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

pub(crate) fn is_unread_idle_notification(message: &MessageEnvelope) -> bool {
    !message.read && idle_notification_sender(message).is_some()
}

pub(crate) fn idle_sender(message: &MessageEnvelope) -> Option<AgentName> {
    idle_notification_sender(message).and_then(|sender| sender.parse().ok())
}

pub(crate) fn idle_notification_sender(message: &MessageEnvelope) -> Option<String> {
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

pub(crate) fn classify_all(
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
                message_key: None,
                source_index: message.source_index,
                source_path: message.source_path,
                bucket,
                class,
                envelope: projected,
            }
        })
        .collect()
}

pub(crate) fn apply_filters(
    messages: Vec<ClassifiedMessage>,
    sender_filter: Option<&AgentName>,
    timestamp_filter: Option<IsoTimestamp>,
) -> Vec<ClassifiedMessage> {
    filters::apply_timestamp_filter(
        filters::apply_sender_filter(messages, sender_filter),
        timestamp_filter,
    )
}

pub(crate) fn bucket_counts_for(messages: &[ClassifiedMessage]) -> BucketCounts {
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
