use crate::read::ClassifiedMessage;
use crate::types::{DisplayBucket, IsoTimestamp, ReadSelection};

pub fn apply_sender_filter(
    messages: Vec<ClassifiedMessage>,
    sender: Option<&str>,
) -> Vec<ClassifiedMessage> {
    match sender {
        Some(sender) => messages
            .into_iter()
            .filter(|message| message.envelope.from == sender)
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

pub fn apply_selection_mode(
    messages: Vec<ClassifiedMessage>,
    mode: ReadSelection,
    seen_watermark: Option<IsoTimestamp>,
) -> Vec<ClassifiedMessage> {
    messages
        .into_iter()
        .filter(|message| match mode {
            ReadSelection::Actionable => matches!(
                message.bucket,
                DisplayBucket::Unread | DisplayBucket::PendingAck
            ),
            ReadSelection::UnreadOnly => message.bucket == DisplayBucket::Unread,
            ReadSelection::PendingAckOnly => message.bucket == DisplayBucket::PendingAck,
            ReadSelection::ActionableWithHistory => match message.bucket {
                DisplayBucket::Unread | DisplayBucket::PendingAck => true,
                DisplayBucket::History => history_visible(message, seen_watermark),
            },
            ReadSelection::All => true,
        })
        .collect()
}

fn history_visible(message: &ClassifiedMessage, seen_watermark: Option<IsoTimestamp>) -> bool {
    match seen_watermark {
        Some(watermark) => message.envelope.timestamp > watermark,
        None => true,
    }
}
