use crate::read::ClassifiedMessage;
use crate::types::{AgentName, DisplayBucket, IsoTimestamp, ReadSelection, TaskId};

pub(crate) fn normalized_contains_needle(contains: Option<&str>) -> Option<String> {
    contains
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
}

pub(crate) fn text_contains_needle(text: Option<&str>, needle: &str) -> bool {
    text.is_some_and(|value| value.to_ascii_lowercase().contains(needle))
}

pub fn apply_sender_filter(
    messages: Vec<ClassifiedMessage>,
    sender: Option<&AgentName>,
) -> Vec<ClassifiedMessage> {
    match sender {
        Some(sender) => messages
            .into_iter()
            .filter(|message| &message.envelope.from == sender)
            .collect(),
        None => messages,
    }
}

pub fn apply_timestamp_filter(
    messages: Vec<ClassifiedMessage>,
    since: Option<IsoTimestamp>,
) -> Vec<ClassifiedMessage> {
    match since {
        Some(since) => messages
            .into_iter()
            .filter(|message| message.envelope.timestamp >= since)
            .collect(),
        None => messages,
    }
}

pub fn apply_task_filter(
    messages: Vec<ClassifiedMessage>,
    task_id: Option<&TaskId>,
) -> Vec<ClassifiedMessage> {
    match task_id {
        Some(task_id) => messages
            .into_iter()
            .filter(|message| message.envelope.task_id.as_ref() == Some(task_id))
            .collect(),
        None => messages,
    }
}

#[cfg(test)]
pub fn apply_contains_filter(
    messages: Vec<ClassifiedMessage>,
    contains: Option<&str>,
) -> Vec<ClassifiedMessage> {
    match normalized_contains_needle(contains) {
        Some(needle) => messages
            .into_iter()
            .filter(|message| {
                text_contains_needle(message.envelope.summary.as_deref(), &needle)
                    || text_contains_needle(Some(message.envelope.text.as_str()), &needle)
            })
            .collect(),
        None => messages,
    }
}

pub fn apply_selection_mode(
    messages: Vec<ClassifiedMessage>,
    mode: ReadSelection,
    _seen_watermark: Option<IsoTimestamp>,
) -> Vec<ClassifiedMessage> {
    messages
        .into_iter()
        .filter(|message| match mode {
            ReadSelection::Actionable => matches!(
                message.bucket,
                DisplayBucket::Unread | DisplayBucket::PendingAck
            ),
            ReadSelection::Unread => message.bucket == DisplayBucket::Unread,
            ReadSelection::PendingAck => message.bucket == DisplayBucket::PendingAck,
            ReadSelection::All => true,
        })
        .collect()
}
