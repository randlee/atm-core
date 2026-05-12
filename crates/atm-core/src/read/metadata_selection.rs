use std::path::PathBuf;

use crate::boundary;
use crate::schema::MessageEnvelope;
use crate::threading::{ThreadIndex, is_ephemeral, is_expired_ephemeral};
use crate::types::{DisplayBucket, IsoTimestamp, MessageClass, ReadSelection};

use super::{BucketCounts, ClassifiedMessage, ReadQuery, filters, state};

pub(crate) fn selection_state_for_mailbox_metadata_rows(
    rows: &[boundary::MailStoreMailboxMetadataRow],
    query: &ReadQuery,
    seen_watermark: Option<IsoTimestamp>,
) -> (BucketCounts, Vec<ClassifiedMessage>) {
    let classified_all = classify_mailbox_metadata_rows(rows);
    let logical_current = logical_current_messages(classified_all.clone());
    let bucket_counts = bucket_counts_for(&logical_current);
    if let Some(message_id) = query.message_id_filter {
        let selected = classified_all
            .into_iter()
            .filter(|message| message.envelope.message_id == Some(message_id))
            .collect();
        return (bucket_counts, selected);
    }
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
            envelope: MessageEnvelope {
                from: row.from_agent.clone(),
                text: row.summary.clone().unwrap_or_default(),
                timestamp: row.message_at,
                read: row.read,
                source_team: None,
                summary: row.summary.clone(),
                message_id: row.message_id,
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

pub(crate) fn apply_filters(
    messages: Vec<ClassifiedMessage>,
    sender_filter: Option<&crate::types::AgentName>,
    timestamp_filter: Option<IsoTimestamp>,
    task_filter: Option<&crate::types::TaskId>,
    contains_filter: Option<&str>,
) -> Vec<ClassifiedMessage> {
    filters::apply_contains_filter(
        filters::apply_task_filter(
            filters::apply_timestamp_filter(
                filters::apply_sender_filter(messages, sender_filter),
                timestamp_filter,
            ),
            task_filter,
        ),
        contains_filter,
    )
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
