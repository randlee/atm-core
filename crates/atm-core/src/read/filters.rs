use crate::read::ClassifiedMessage;
use crate::types::{AgentName, DisplayBucket, IsoTimestamp, ReadSelection, TaskId};

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

pub fn apply_contains_filter(
    messages: Vec<ClassifiedMessage>,
    contains: Option<&str>,
) -> Vec<ClassifiedMessage> {
    match contains.map(str::trim).filter(|value| !value.is_empty()) {
        Some(needle) => {
            let needle = needle.to_ascii_lowercase();
            messages
                .into_iter()
                .filter(|message| {
                    message
                        .envelope
                        .summary
                        .as_deref()
                        .unwrap_or_default()
                        .to_ascii_lowercase()
                        .contains(&needle)
                        || message.envelope.text.to_ascii_lowercase().contains(&needle)
                })
                .collect()
        }
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
