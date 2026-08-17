use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::address::MessageParticipantFilter;
use crate::boundary;
use crate::error::AtmError;
use crate::schema::InboxMessage;
use crate::service_runtime_store::RetainedMailboxRuntime;
use crate::threading::{ThreadIndex, is_ephemeral, is_expired_ephemeral};
use crate::types::{DisplayBucket, IsoTimestamp, MessageClass, ReadSelection, TeamName};

use super::{BucketCounts, ClassifiedMessage, ReadQuery, filters, state};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataBackedReadSelection {
    pub summary_text: Option<String>,
    pub message_text: Option<String>,
}

pub(crate) fn selection_state_for_mailbox_metadata_rows(
    rows: &[boundary::MailStoreMailboxMetadataRow],
    query: &ReadQuery,
    seen_watermark: Option<IsoTimestamp>,
) -> (BucketCounts, Vec<ClassifiedMessage>) {
    let classified_all = classify_mailbox_metadata_rows(rows);
    if let Some(message_id) = query.message_id_filter() {
        let selected = classified_all
            .iter()
            .filter(|message| message.envelope.message_id == Some(*message_id))
            .filter(|message| {
                filters::matches_participant_filter(
                    message,
                    query.mailbox.participant_filter.as_ref(),
                )
            })
            .cloned()
            .collect();
        let logical_current = logical_current_messages(classified_all);
        let bucket_counts = bucket_counts_for(&logical_current);
        return (bucket_counts, selected);
    }
    let logical_current = logical_current_messages(classified_all);
    let bucket_counts = bucket_counts_for(&logical_current);
    let filtered = apply_metadata_only_filters(
        logical_current,
        query.mailbox.sender_filter.as_ref(),
        query.mailbox.participant_filter.as_ref(),
        query.mailbox.timestamp_filter,
        query.mailbox.task_filter.as_ref(),
    );
    let selected = select_messages(&filtered, query.selection_mode(), seen_watermark);
    (bucket_counts, selected)
}

pub(crate) fn classify_mailbox_metadata_rows(
    rows: &[boundary::MailStoreMailboxMetadataRow],
) -> Vec<ClassifiedMessage> {
    let projected = rows
        .iter()
        .enumerate()
        .map(|(index, row)| ClassifiedMessage {
            source_index: index.into(),
            source_path: PathBuf::from(row.message_key.as_ref()),
            bucket: DisplayBucket::Unread,
            class: MessageClass::Unread,
            envelope: InboxMessage {
                from: row.from_agent.clone(),
                source_chat_id: row.source_chat_id.clone(),
                // Metadata rows intentionally do not carry durable message body
                // text. AD.20 keeps this projection empty so later contains
                // evaluation cannot accidentally treat summary-only data as the
                // durable body contract.
                text: String::new(),
                timestamp: row.message_at,
                read: row.read,
                source_team: None,
                destination_chat_id: row.destination_chat_id.clone(),
                summary: row.summary.clone(),
                message_id: row.message_id,
                requires_ack: row.requires_ack,
                pending_ack_at: row.pending_ack.then_some(row.message_at),
                acknowledged_at: row.acknowledged_at,
                acknowledges_message_id: None,
                parent_message_id: row.parent_message_id,
                thread_mode: row.thread_mode,
                expires_at: row.expires_at,
                task_id: row.task_id.clone(),
                extra: serde_json::Map::new(),
            },
        })
        .collect::<Vec<_>>();
    let envelopes = projected
        .iter()
        .map(|message| message.envelope.clone())
        .collect::<Vec<_>>();
    let thread_index = ThreadIndex::new(&envelopes);

    projected
        .into_iter()
        .map(|message| {
            let effective = effective_display_envelope(&message.envelope, &thread_index);
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

pub(crate) fn apply_metadata_only_filters(
    messages: Vec<ClassifiedMessage>,
    sender_filter: Option<&crate::types::AgentName>,
    participant_filter: Option<&MessageParticipantFilter>,
    timestamp_filter: Option<IsoTimestamp>,
    task_filter: Option<&crate::types::TaskId>,
) -> Vec<ClassifiedMessage> {
    filters::apply_task_filter(
        filters::apply_timestamp_filter(
            filters::apply_participant_filter(
                filters::apply_sender_filter(messages, sender_filter),
                participant_filter,
            ),
            timestamp_filter,
        ),
        task_filter,
    )
}

pub(crate) fn filter_metadata_backed_contains_candidates<R: RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &Path,
    team: &TeamName,
    agent: &crate::types::AgentName,
    metadata_rows: &[boundary::MailStoreMailboxMetadataRow],
    messages: Vec<ClassifiedMessage>,
    contains_needle: Option<&str>,
) -> Result<Vec<ClassifiedMessage>, AtmError> {
    let Some(needle) = contains_needle else {
        return Ok(messages);
    };
    let row_by_id = metadata_rows
        .iter()
        .filter_map(|row| row.message_id.map(|message_id| (message_id, row)))
        .collect::<HashMap<_, _>>();

    messages
        .into_iter()
        .map(|message| -> Result<Option<ClassifiedMessage>, AtmError> {
            let row = metadata_rows.get(message.source_index.get()).ok_or_else(|| {
                AtmError::validation(format!(
                    "sqlite mailbox metadata source index {} is out of range during contains filtering",
                    message.source_index.get()
                ))

            })?;
            let mut selection = MetadataBackedReadSelection {
                summary_text: row.summary.clone(),
                message_text: None,
            };
            if filters::text_contains_needle(selection.summary_text.as_deref(), needle) {
                return Ok(Some(message));
            }
            selection.message_text = Some(load_durable_message_text(
                runtime,
                home_dir,
                team,
                agent,
                &row_by_id,
                &message,
                row,
            )?);
            Ok(filters::text_contains_needle(
                selection.message_text.as_deref(),
                needle,
            )
            .then_some(message))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|selected| selected.into_iter().flatten().collect())
}

pub(crate) fn load_durable_metadata_message<R: RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &Path,
    team: &TeamName,
    agent: &crate::types::AgentName,
    metadata_rows: &[boundary::MailStoreMailboxMetadataRow],
    selected_message: &ClassifiedMessage,
    exact_message_id: Option<crate::schema::AtmMessageId>,
) -> Result<ClassifiedMessage, AtmError> {
    let row_by_id = metadata_rows
        .iter()
        .filter_map(|row| row.message_id.map(|message_id| (message_id, row)))
        .collect::<HashMap<_, _>>();
    let metadata_row = metadata_rows
        .get(selected_message.source_index.get())
        .ok_or_else(|| {
            AtmError::validation(format!(
                "sqlite mailbox metadata source index {} is out of range during durable reload",
                selected_message.source_index.get()
            ))
        })?;
    let Some(record) =
        runtime.load_message_record(home_dir, team, agent, &metadata_row.message_key)?
    else {
        // A concurrent clear can delete the row after selection but before the
        // durable body reload. Preserve command completion with the selected
        // snapshot; actual reload errors still propagate above.
        return Ok(selected_message.clone());
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
            selected_message,
            record.envelope,
        )?
    } else {
        record.envelope
    };
    let envelope =
        runtime.render_message_body(&metadata_row.message_key, envelope.message_id, &envelope)?;
    Ok(ClassifiedMessage {
        source_index: selected_message.source_index,
        source_path: selected_message.source_path.clone(),
        bucket: state::display_bucket_for_class(state::classify_message(&envelope)),
        class: state::classify_message(&envelope),
        envelope,
    })
}

fn load_durable_message_text<R: RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &Path,
    team: &TeamName,
    agent: &crate::types::AgentName,
    row_by_id: &HashMap<crate::schema::AtmMessageId, &boundary::MailStoreMailboxMetadataRow>,
    selected_message: &ClassifiedMessage,
    metadata_row: &boundary::MailStoreMailboxMetadataRow,
) -> Result<String, AtmError> {
    let Some(record) =
        runtime.load_message_record(home_dir, team, agent, &metadata_row.message_key)?
    else {
        return Err(AtmError::validation(format!(
            "sqlite mailbox metadata row {} could not be reloaded for contains filtering",
            metadata_row.message_key
        )));
    };
    let envelope = if record.envelope.thread_mode == Some(crate::schema::ThreadMode::AddDetails) {
        load_logical_current_record(
            runtime,
            home_dir,
            team,
            agent,
            row_by_id,
            selected_message,
            record.envelope,
        )?
    } else {
        record.envelope
    };
    Ok(runtime
        .render_message_body(&metadata_row.message_key, envelope.message_id, &envelope)?
        .text)
}

fn load_logical_current_record<R: RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &Path,
    team: &TeamName,
    agent: &crate::types::AgentName,
    row_by_id: &HashMap<crate::schema::AtmMessageId, &boundary::MailStoreMailboxMetadataRow>,
    selected_message: &ClassifiedMessage,
    terminal_envelope: InboxMessage,
) -> Result<InboxMessage, AtmError> {
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
            )));
        };
        let Some(record) = runtime.load_message_record(home_dir, team, agent, &row.message_key)?
        else {
            return Err(AtmError::validation(format!(
                "sqlite mailbox thread row {} could not be reloaded for logical-current reconstruction",
                row.message_key
            )));
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
        })
}

pub(crate) fn logical_current_messages(messages: Vec<ClassifiedMessage>) -> Vec<ClassifiedMessage> {
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
        .map(|mut message| {
            if let Some(message_id) = message.envelope.message_id
                && let Some(logical) = thread_index.logical_current_envelope(message_id)
            {
                message.envelope = logical;
            }
            message
        })
        .collect()
}

pub(crate) fn bucket_counts_for(messages: &[ClassifiedMessage]) -> BucketCounts {
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
                DisplayBucket::Unread => counts.unread += 1,
                DisplayBucket::PendingAck => counts.pending_ack += 1,
                DisplayBucket::History => counts.history += 1,
            }
            counts
        },
    )
}

pub(crate) fn select_messages(
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

pub(crate) fn sort_and_limit_selected(selected: &mut Vec<ClassifiedMessage>, limit: Option<usize>) {
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

pub(crate) fn effective_display_envelope(
    envelope: &InboxMessage,
    thread_index: &ThreadIndex<'_>,
) -> InboxMessage {
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

fn hidden_from_normal_views(envelope: &InboxMessage) -> bool {
    let now = IsoTimestamp::now();
    is_expired_ephemeral(envelope, now) || (is_ephemeral(envelope) && envelope.read)
}

fn hidden_for_selection(envelope: &InboxMessage, selection_mode: ReadSelection) -> bool {
    let now = IsoTimestamp::now();
    if is_expired_ephemeral(envelope, now) {
        return true;
    }
    selection_mode != ReadSelection::All && is_ephemeral(envelope) && envelope.read
}
