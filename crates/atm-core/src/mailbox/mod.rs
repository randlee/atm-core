pub(crate) mod atomic;
pub(crate) mod hash;
pub(crate) mod lock;
pub(crate) mod source;
pub(crate) mod store;
pub(crate) mod surface;

use std::fs;
use std::io::BufRead;
use std::path::Path;

use serde_json::Value;
use tracing::warn;

use crate::error::{AtmError, AtmErrorKind};
use crate::schema::{LegacyMessageId, MessageEnvelope};
use uuid::Uuid;

pub(crate) fn temp_file_suffix() -> String {
    format!("{}-{}", std::process::id(), Uuid::new_v4().simple())
}

/// Append one message to a mailbox JSONL file under the mailbox lock.
///
/// # Errors
///
/// Returns [`AtmError`] with mailbox read/write or lock codes when the mailbox
/// cannot be loaded, locked, or atomically replaced.
pub fn append_message(path: &Path, envelope: &MessageEnvelope) -> Result<(), AtmError> {
    locked_read_modify_write(path, lock::DEFAULT_LOCK_TIMEOUT, |messages| {
        messages.push(envelope.clone());
        Ok(())
    })
}

pub(crate) fn locked_read_modify_write<F>(
    path: &Path,
    timeout: std::time::Duration,
    mutate: F,
) -> Result<(), AtmError>
where
    F: FnOnce(&mut Vec<MessageEnvelope>) -> Result<(), AtmError>,
{
    let _guard = lock::acquire(path, timeout)?;
    let mut messages = read_messages(path)?;
    mutate(&mut messages)?;
    atomic::write_messages(path, &messages)
}

/// Read all valid mailbox records from a mailbox JSONL file.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::MailboxReadFailed`] when the mailbox
/// file cannot be opened or read.
pub fn read_messages(path: &Path) -> Result<Vec<MessageEnvelope>, AtmError> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = fs::File::open(path).map_err(|error| {
        AtmError::new(
            AtmErrorKind::MailboxRead,
            format!("failed to open mailbox file {}: {error}", path.display()),
        )
        .with_recovery("Retry after concurrent ATM activity completes, or verify the mailbox file still exists and is readable.")
        .with_source(error)
    })?;
    let reader = std::io::BufReader::new(file);
    let mut messages = Vec::new();

    for (index, line) in reader.lines().enumerate() {
        let line = line.map_err(|error| {
            AtmError::new(
                AtmErrorKind::MailboxRead,
                format!(
                    "failed to read mailbox line {} from {}: {error}",
                    index + 1,
                    path.display()
                ),
            )
            .with_recovery("Retry after concurrent ATM activity completes, or inspect the mailbox file for truncation or permission issues.")
            .with_source(error)
        })?;
        if line.trim().is_empty() {
            continue;
        }

        match parse_mailbox_record(&line, path, index + 1) {
            Ok(Some(message)) => messages.push(message),
            Ok(None) => {}
            Err(error) => warn!(
                line = index + 1,
                mailbox_path = %path.display(),
                raw_record = %line,
                %error,
                "skipping malformed mailbox record"
            ),
        }
    }

    Ok(messages)
}

fn parse_mailbox_record(
    line: &str,
    path: &Path,
    line_number: usize,
) -> Result<Option<MessageEnvelope>, serde_json::Error> {
    let mut value = serde_json::from_str::<Value>(line)?;
    sanitize_legacy_message_id(&mut value, path, line_number);
    serde_json::from_value::<MessageEnvelope>(value).map(Some)
}

fn sanitize_legacy_message_id(value: &mut Value, path: &Path, line_number: usize) {
    let Some(object) = value.as_object_mut() else {
        return;
    };

    let Some(raw_message_id) = object.get("message_id").cloned() else {
        return;
    };

    if raw_message_id.is_null() {
        return;
    }

    if serde_json::from_value::<LegacyMessageId>(raw_message_id.clone()).is_err() {
        warn!(
            mailbox_path = %path.display(),
            line = line_number,
            field = "message_id",
            expected_format = "UUID",
            raw_value = %raw_message_id,
            "treating malformed ATM-owned field as absent during mailbox read"
        );
        object.remove("message_id");
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Barrier};
    use std::thread;

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
    fn read_messages_preserves_duplicate_message_ids_for_surface_canonicalization() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("dedupe.jsonl");
        let message_id = Uuid::new_v4();
        let first = sample_message(message_id, "first");
        let mut second = sample_message(message_id, "second");
        second.timestamp = IsoTimestamp::from_datetime(
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
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].text, "first");
        assert_eq!(messages[1].text, "second");
    }

    #[test]
    fn read_messages_treats_malformed_legacy_message_id_as_absent() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("malformed-message-id.jsonl");
        let contents = serde_json::json!({
            "from": "team-lead",
            "text": "valid body",
            "timestamp": "2026-03-30T00:00:00Z",
            "read": false,
            "message_id": "01JABCDEF0123456789ABCDEF0"
        });
        fs::write(
            &path,
            format!("{}\n", serde_json::to_string(&contents).expect("json")),
        )
        .expect("write");

        let messages = read_messages(&path).expect("read");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "valid body");
        assert!(messages[0].message_id.is_none());
    }

    #[test]
    fn append_message_preserves_both_records_under_concurrent_writers() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("append-message-concurrent.jsonl");
        let barrier = Arc::new(Barrier::new(3));

        let mut handles = Vec::new();
        for body in ["first", "second"] {
            let path = path.clone();
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                let envelope = sample_message(Uuid::new_v4(), body);
                barrier.wait();
                append_message(&path, &envelope).expect("append");
            }));
        }

        barrier.wait();
        for handle in handles {
            handle.join().expect("thread");
        }

        let messages = read_messages(&path).expect("read");
        assert_eq!(messages.len(), 2);
        assert!(messages.iter().any(|message| message.text == "first"));
        assert!(messages.iter().any(|message| message.text == "second"));
    }

    fn sample_message(message_id: Uuid, body: &str) -> MessageEnvelope {
        MessageEnvelope {
            from: "arch-ctm".into(),
            text: body.into(),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: Some("atm-dev".into()),
            summary: None,
            message_id: Some(message_id.into()),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            task_id: None,
            extra: serde_json::Map::new(),
        }
    }
}
