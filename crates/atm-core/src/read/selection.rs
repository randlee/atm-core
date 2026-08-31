//! Pure mailbox selection shared by the retained sync path and the Tokio port.
//!
//! This module deliberately accepts already-authorized mailbox records and has
//! no storage, filesystem, clock-wait, or mutation dependency.  Both service
//! paths therefore classify visibility and apply filters from one definition.

use std::path::PathBuf;

use crate::address::MessageParticipantFilter;
use crate::schema::{AtmMessageId, InboxMessage};
use crate::threading::{ThreadIndex, is_ephemeral, is_expired_ephemeral};
use crate::types::{AgentName, DisplayBucket, IsoTimestamp, MessageClass, ReadSelection, TaskId};

use super::{BucketCounts, ClassifiedMessage, ReadQuery, filters, state};

/// Pure filter and visibility inputs for one authorized mailbox selection.
#[derive(Debug, Clone)]
pub struct MailboxSelectionRequest {
    pub selection_mode: ReadSelection,
    pub seen_watermark: Option<IsoTimestamp>,
    pub message_id_filter: Option<AtmMessageId>,
    pub sender_filter: Option<AgentName>,
    pub participant_filter: Option<MessageParticipantFilter>,
    pub timestamp_filter: Option<IsoTimestamp>,
    pub task_filter: Option<TaskId>,
    pub contains_filter: Option<String>,
}

impl Default for MailboxSelectionRequest {
    fn default() -> Self {
        Self {
            selection_mode: ReadSelection::Unread,
            seen_watermark: None,
            message_id_filter: None,
            sender_filter: None,
            participant_filter: None,
            timestamp_filter: None,
            task_filter: None,
            contains_filter: None,
        }
    }
}

impl MailboxSelectionRequest {
    /// Converts the retained request shape without exposing a second set of
    /// business-selection rules to the async runtime.
    #[must_use]
    pub fn from_read_query(query: &ReadQuery, seen_watermark: Option<IsoTimestamp>) -> Self {
        Self {
            selection_mode: query.selection_mode(),
            seen_watermark,
            message_id_filter: query.message_id_filter().copied(),
            sender_filter: query.mailbox.sender_filter.clone(),
            participant_filter: query.mailbox.participant_filter.clone(),
            timestamp_filter: query.mailbox.timestamp_filter,
            task_filter: query.mailbox.task_filter.clone(),
            contains_filter: query.mailbox.contains_filter.clone(),
        }
    }

    #[must_use]
    pub fn without_contains(mut self) -> Self {
        self.contains_filter = None;
        self
    }
}

/// One fully materialized, storage-neutral mailbox record.
#[derive(Debug, Clone)]
pub struct MailboxSelectionCandidate {
    pub message_key: String,
    pub envelope: InboxMessage,
}

/// Public, source-neutral output from the shared selection engine.
#[derive(Debug, Clone)]
pub struct SelectedMailboxMessage {
    pub message_key: String,
    pub bucket: DisplayBucket,
    pub class: MessageClass,
    pub envelope: InboxMessage,
}

/// Selection result used by the Tokio mailbox port.
#[derive(Debug, Clone)]
pub struct MailboxSelectionResult {
    pub bucket_counts: BucketCounts,
    pub selected: Vec<SelectedMailboxMessage>,
}

/// Selects fully materialized records. This is the public, storage-neutral
/// seam consumed by `atm-runtime`.
#[must_use]
pub fn select_mailbox_candidates(
    candidates: Vec<MailboxSelectionCandidate>,
    request: &MailboxSelectionRequest,
) -> MailboxSelectionResult {
    let (bucket_counts, selected) = select_classified_mailbox_candidates(candidates, request);
    MailboxSelectionResult {
        bucket_counts,
        selected: selected
            .into_iter()
            .map(|message| SelectedMailboxMessage {
                message_key: message.source_path.to_string_lossy().into_owned(),
                bucket: message.bucket,
                class: message.class,
                envelope: message.envelope,
            })
            .collect(),
    }
}

/// Crate-private form retaining source positions for the retained sync path's
/// durable-body reload. No I/O is performed here.
pub(crate) fn select_classified_mailbox_candidates(
    candidates: Vec<MailboxSelectionCandidate>,
    request: &MailboxSelectionRequest,
) -> (BucketCounts, Vec<ClassifiedMessage>) {
    let classified_all = classify_candidates(candidates);
    if let Some(message_id) = request.message_id_filter {
        let selected = classified_all
            .iter()
            .filter(|message| message.envelope.message_id == Some(message_id))
            .filter(|message| {
                filters::matches_participant_filter(message, request.participant_filter.as_ref())
            })
            .filter(|message| message_matches_contains(message, request.contains_filter.as_deref()))
            .cloned()
            .collect();
        return (
            bucket_counts_for(&logical_current_messages(classified_all)),
            selected,
        );
    }

    let logical_current = logical_current_messages(classified_all);
    let bucket_counts = bucket_counts_for(&logical_current);
    let filtered = filters::apply_task_filter(
        filters::apply_timestamp_filter(
            filters::apply_participant_filter(
                filters::apply_sender_filter(logical_current, request.sender_filter.as_ref()),
                request.participant_filter.as_ref(),
            ),
            request.timestamp_filter,
        ),
        request.task_filter.as_ref(),
    );
    let selected = filters::apply_selection_mode(
        filtered
            .into_iter()
            .filter(|message| !hidden_for_selection(&message.envelope, request.selection_mode))
            .filter(|message| message_matches_contains(message, request.contains_filter.as_deref()))
            .collect(),
        request.selection_mode,
        if request.selection_mode == ReadSelection::All {
            None
        } else {
            request.seen_watermark
        },
    );
    (bucket_counts, selected)
}

pub(crate) fn classify_mailbox_candidates(
    candidates: Vec<MailboxSelectionCandidate>,
) -> Vec<ClassifiedMessage> {
    classify_candidates(candidates)
}

fn classify_candidates(candidates: Vec<MailboxSelectionCandidate>) -> Vec<ClassifiedMessage> {
    let projected = candidates
        .into_iter()
        .enumerate()
        .map(|(index, candidate)| ClassifiedMessage {
            source_index: index.into(),
            source_path: PathBuf::from(candidate.message_key),
            bucket: DisplayBucket::Unread,
            class: MessageClass::Unread,
            envelope: candidate.envelope,
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
            let envelope = effective_display_envelope(&message.envelope, &thread_index);
            let class = state::classify_message(&envelope);
            ClassifiedMessage {
                source_index: message.source_index,
                source_path: message.source_path,
                bucket: state::display_bucket_for_class(class),
                class,
                envelope,
            }
        })
        .collect()
}

fn message_matches_contains(message: &ClassifiedMessage, needle: Option<&str>) -> bool {
    needle.is_none_or(|value| filters::text_contains_needle(Some(&message.envelope.text), value))
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
                DisplayBucket::Unread => counts.unread += 1,
                DisplayBucket::PendingAck => counts.pending_ack += 1,
                DisplayBucket::History => counts.history += 1,
            }
            counts
        },
    )
}

fn effective_display_envelope(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::InboxMessage;
    use crate::types::{AgentName, IsoTimestamp};

    fn candidate(key: &str, read: bool) -> MailboxSelectionCandidate {
        MailboxSelectionCandidate {
            message_key: key.to_owned(),
            envelope: InboxMessage {
                from: "sender".parse::<AgentName>().expect("agent"),
                source_chat_id: None,
                text: "body".to_owned(),
                timestamp: IsoTimestamp::now(),
                read,
                source_team: None,
                destination_chat_id: None,
                summary: None,
                message_id: None,
                requires_ack: false,
                pending_ack_at: None,
                acknowledged_at: None,
                acknowledges_message_id: None,
                parent_message_id: None,
                thread_mode: None,
                expires_at: None,
                task_id: None,
                extra: serde_json::Map::new(),
            },
        }
    }

    #[test]
    fn selection_keeps_unread_visibility_and_missing_messages_are_empty() {
        let result = select_mailbox_candidates(
            vec![candidate("unread", false), candidate("history", true)],
            &MailboxSelectionRequest::default(),
        );
        assert_eq!(result.bucket_counts.unread, 1);
        assert_eq!(result.bucket_counts.history, 1);
        assert_eq!(result.selected.len(), 1);
        assert_eq!(result.selected[0].message_key, "unread");

        let missing = select_mailbox_candidates(Vec::new(), &MailboxSelectionRequest::default());
        assert!(missing.selected.is_empty());
        assert_eq!(missing.bucket_counts.unread, 0);
    }
}
