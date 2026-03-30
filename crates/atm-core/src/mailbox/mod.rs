pub mod atomic;
pub mod hash;
pub mod lock;
pub mod store;

use std::collections::HashMap;
use std::fs;
use std::io::BufRead;
use std::path::Path;

use tracing::warn;

use crate::error::{AtmError, AtmErrorKind};
use crate::schema::MessageEnvelope;

pub fn append_message(path: &Path, envelope: &MessageEnvelope) -> Result<(), AtmError> {
    let mut messages = read_messages(path)?;
    messages.push(envelope.clone());
    atomic::write_messages(path, &messages)
}

pub fn read_messages(path: &Path) -> Result<Vec<MessageEnvelope>, AtmError> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = fs::File::open(path).map_err(|error| {
        AtmError::new(
            AtmErrorKind::MailboxRead,
            format!("failed to open mailbox file: {error}"),
        )
        .with_source(error)
    })?;
    let reader = std::io::BufReader::new(file);
    let mut messages = Vec::new();

    for (index, line) in reader.lines().enumerate() {
        let line = line.map_err(|error| {
            AtmError::new(
                AtmErrorKind::MailboxRead,
                format!("failed to read mailbox line: {error}"),
            )
            .with_source(error)
        })?;
        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<MessageEnvelope>(&line) {
            Ok(message) => messages.push(message),
            Err(error) => warn!(
                line = index + 1,
                %error,
                "skipping malformed mailbox record"
            ),
        }
    }

    let mut last_indices = HashMap::new();
    for (index, message) in messages.iter().enumerate() {
        if let Some(message_id) = message.message_id {
            last_indices.insert(message_id, index);
        }
    }

    Ok(messages
        .into_iter()
        .enumerate()
        .filter_map(|(index, message)| match message.message_id {
            Some(message_id) => (last_indices.get(&message_id) == Some(&index)).then_some(message),
            None => Some(message),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::{TimeZone, Utc};
    use tempfile::TempDir;
    use uuid::Uuid;

    use crate::schema::MessageEnvelope;
    use crate::types::IsoTimestamp;

    use super::{append_message, read_messages};

    #[test]
    fn append_message_persists_one_jsonl_record() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("append-message.jsonl");
        let envelope = sample_message(Uuid::new_v4(), "first");

        append_message(&path, &envelope).expect("append");

        let raw = fs::read_to_string(&path).expect("raw contents");
        assert!(raw.contains("\"text\":\"first\""));
        let read_back = read_messages(&path).expect("read back");
        assert_eq!(read_back, vec![envelope]);
    }

    #[test]
    fn read_messages_skips_malformed_lines() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("skip-malformed.jsonl");
        let valid =
            serde_json::to_string(&sample_message(Uuid::new_v4(), "valid")).expect("valid json");
        fs::write(&path, format!("{valid}\n{{not-json}}\n")).expect("write");

        let messages = read_messages(&path).expect("read");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "valid");
    }

    #[test]
    fn read_messages_deduplicates_by_message_id_last_wins() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("dedupe.jsonl");
        let message_id = Uuid::new_v4();
        let first = sample_message(message_id, "first");
        let mut second = sample_message(message_id, "second");
        second.timestamp = IsoTimestamp(
            Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 1)
                .single()
                .expect("timestamp"),
        );

        let contents = format!(
            "{}\n{}\n",
            serde_json::to_string(&first).expect("json"),
            serde_json::to_string(&second).expect("json")
        );
        fs::write(&path, contents).expect("write");

        let messages = read_messages(&path).expect("read");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "second");
    }

    fn sample_message(message_id: Uuid, body: &str) -> MessageEnvelope {
        MessageEnvelope {
            from: "arch-ctm".into(),
            text: body.into(),
            timestamp: IsoTimestamp(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: Some("atm-dev".into()),
            summary: None,
            message_id: Some(message_id),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            task_id: None,
            extra: serde_json::Map::new(),
        }
    }
}
