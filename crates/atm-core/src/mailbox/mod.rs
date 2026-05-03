//! Mailbox read/write helpers, compatibility parsing, and lock-scoped mutation.

pub(crate) mod atomic;
pub(crate) mod hash;
pub(crate) mod lock;
pub(crate) mod source;
pub(crate) mod store;
pub(crate) mod surface;

use std::fs;
use std::path::Path;

use serde_json::Value;
use tracing::warn;

use crate::error::{AtmError, AtmErrorCode, AtmErrorKind};
use crate::schema::inbox_message::hydrate_legacy_fields_from_metadata;
use crate::schema::{LegacyMessageId, MessageEnvelope};

const MAX_MAILBOX_READ_BYTES: u64 = 10 * 1024 * 1024;
/// Append one message to a shared inbox file under the mailbox lock.
///
/// Production send flows use the same lock discipline through
/// `mailbox::store::append_mailbox_message_and_seed_workflow()`. This helper is
/// test-only because production callers must also coordinate workflow seeding.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::MailboxReadFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxWriteFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxLockFailed`], or
/// [`crate::error_codes::AtmErrorCode::MailboxLockTimeout`] when the mailbox
/// cannot be loaded, locked, or atomically replaced.
#[cfg(test)]
pub fn append_message(path: &Path, envelope: &MessageEnvelope) -> Result<(), AtmError> {
    locked_read_modify_write(path, lock::default_lock_timeout(), |messages| {
        messages.push(envelope.clone());
        Ok(())
    })
}

/// Lock, load, mutate, and atomically rewrite one mailbox file.
///
/// Production mutation paths use equivalent lock coverage through
/// `mailbox::store::with_locked_source_files()` plus
/// `mailbox::store::commit_source_files()`. This helper stays test-only so unit
/// tests can exercise the shared mailbox lock contract directly without the
/// workflow/state sidecars required in production commands.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::MailboxLockFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxLockTimeout`],
/// [`crate::error_codes::AtmErrorCode::MailboxReadFailed`], or
/// [`crate::error_codes::AtmErrorCode::MailboxWriteFailed`] when ATM cannot
/// acquire the mailbox lock, read the current mailbox contents, or atomically
/// persist the rewritten file.
#[cfg(test)]
pub(crate) fn locked_read_modify_write<F>(
    path: &Path,
    timeout: std::time::Duration,
    mutate: F,
) -> Result<(), AtmError>
where
    F: FnOnce(&mut Vec<MessageEnvelope>) -> Result<(), AtmError>,
{
    let _guard = lock::acquire_many_sorted([path.to_path_buf()], timeout)?;
    let mut messages = read_messages(path)?;
    mutate(&mut messages)?;
    store::commit_mailbox_state(path, &messages)
}

/// Read all valid mailbox records from one shared inbox file.
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

    let file_size = fs::metadata(path).map_err(|error| {
        AtmError::new(
            AtmErrorKind::MailboxRead,
            format!("failed to inspect mailbox file {}: {error}", path.display()),
        )
        .with_recovery(
            "Retry after concurrent ATM activity completes, or verify the mailbox file still exists and is readable.",
        )
        .with_source(error)
    })?;
    if file_size.len() > MAX_MAILBOX_READ_BYTES {
        return Err(
            AtmError::new(
                AtmErrorKind::MailboxRead,
                format!(
                    "mailbox file {} exceeds the {}-byte read limit",
                    path.display(),
                    MAX_MAILBOX_READ_BYTES
                ),
            )
            .with_recovery(
                "Trim or archive oversized mailbox contents before retrying `atm read` so ATM does not load an unbounded mailbox into memory.",
            ),
        );
    }

    let raw = fs::read_to_string(path).map_err(|error| {
        AtmError::new(
            AtmErrorKind::MailboxRead,
            format!("failed to read mailbox file {}: {error}", path.display()),
        )
        .with_recovery("Retry after concurrent ATM activity completes, or verify the mailbox file still exists and is readable.")
        .with_source(error)
    })?;

    parse_mailbox_contents(&raw, path)
}

fn parse_mailbox_contents(raw: &str, path: &Path) -> Result<Vec<MessageEnvelope>, AtmError> {
    match raw.chars().find(|ch| !ch.is_whitespace()) {
        None => Ok(Vec::new()),
        Some('[') => parse_mailbox_array(raw, path),
        Some(_) => Ok(parse_mailbox_jsonl(raw, path)),
    }
}

fn parse_mailbox_array(raw: &str, path: &Path) -> Result<Vec<MessageEnvelope>, AtmError> {
    let records = serde_json::from_str::<Vec<Value>>(raw).map_err(|error| {
        AtmError::new(
            AtmErrorKind::MailboxRead,
            format!("failed to parse mailbox array {}: {error}", path.display()),
        )
        .with_recovery(
            "Inspect the mailbox file for malformed JSON array syntax or partial writes before retrying `atm read`.",
        )
        .with_source(error)
    })?;

    Ok(records
        .into_iter()
        .enumerate()
        .filter_map(
            |(index, mut value)| match parse_mailbox_value(&mut value, path, index + 1) {
                Ok(Some(message)) => Some(message),
                Ok(None) => None,
                Err(error) => {
                    warn!(
                        code = %AtmErrorCode::WarningMailboxRecordSkipped,
                        line = index + 1,
                        mailbox_path = %path.display(),
                        raw_record = %value,
                        %error,
                        "skipping malformed mailbox record"
                    );
                    None
                }
            },
        )
        .collect())
}

fn parse_mailbox_jsonl(raw: &str, path: &Path) -> Vec<MessageEnvelope> {
    raw.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            if line.trim().is_empty() {
                return None;
            }

            match parse_mailbox_record(line, path, index + 1) {
                Ok(Some(message)) => Some(message),
                Ok(None) => None,
                Err(error) => {
                    warn!(
                        code = %AtmErrorCode::WarningMailboxRecordSkipped,
                        line = index + 1,
                        mailbox_path = %path.display(),
                        raw_record = %line,
                        %error,
                        "skipping malformed mailbox record"
                    );
                    None
                }
            }
        })
        .collect()
}

fn parse_mailbox_record(
    raw_record: &str,
    path: &Path,
    line_number: usize,
) -> Result<Option<MessageEnvelope>, serde_json::Error> {
    let mut value = serde_json::from_str::<Value>(raw_record)?;
    parse_mailbox_value(&mut value, path, line_number)
}

fn parse_mailbox_value(
    value: &mut Value,
    path: &Path,
    line_number: usize,
) -> Result<Option<MessageEnvelope>, serde_json::Error> {
    hydrate_legacy_fields_from_metadata(value);
    sanitize_legacy_message_id(value, path, line_number);
    serde_json::from_value::<MessageEnvelope>(value.take()).map(Some)
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
            code = %AtmErrorCode::WarningMalformedAtmFieldIgnored,
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
    use std::fs::{self, File};
    use std::sync::{Arc, Barrier};
    use std::thread;

    use chrono::{TimeZone, Utc};
    use tempfile::TempDir;
    use uuid::Uuid;

    use crate::schema::MessageEnvelope;
    use crate::types::{AgentName, IsoTimestamp, TeamName};

    use super::{MAX_MAILBOX_READ_BYTES, append_message, locked_read_modify_write, read_messages};
    use crate::mailbox::lock;

    #[test]
    fn append_message_persists_one_array_record() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("append-message.json");
        let envelope = sample_message(Uuid::new_v4(), "first");

        append_message(&path, &envelope).expect("append");

        let raw = fs::read_to_string(&path).expect("raw contents");
        let values: Vec<serde_json::Value> = serde_json::from_str(&raw).expect("json array");
        assert_eq!(values.len(), 1);
        assert_eq!(values[0]["text"], "first");
        let read_back = read_messages(&path).expect("read back");
        assert_eq!(read_back, vec![envelope]);
    }

    #[test]
    fn append_message_serializes_metadata_atm_without_top_level_machine_fields() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("append-message-metadata.json");
        let envelope = sample_message(Uuid::new_v4(), "first");

        append_message(&path, &envelope).expect("append");

        let raw = fs::read_to_string(&path).expect("raw contents");
        let values: Vec<serde_json::Value> = serde_json::from_str(&raw).expect("json array");
        let object = values[0].as_object().expect("message object");
        assert!(object.contains_key("metadata"));
        assert!(!object.contains_key("message_id"));
        assert!(!object.contains_key("source_team"));
    }

    #[test]
    fn locked_read_modify_write_reads_mutates_and_rewrites_under_lock() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("locked-rmw.json");
        let first = sample_message(Uuid::new_v4(), "first");
        append_message(&path, &first).expect("seed");

        locked_read_modify_write(&path, lock::DEFAULT_LOCK_TIMEOUT, |messages| {
            assert_eq!(messages.len(), 1);
            messages[0].read = true;
            messages.push(sample_message(Uuid::new_v4(), "second"));
            Ok(())
        })
        .expect("locked read modify write");

        let messages = read_messages(&path).expect("read");
        assert_eq!(messages.len(), 2);
        assert!(messages[0].read);
        assert_eq!(messages[1].text, "second");
    }

    #[test]
    fn append_message_removes_lock_sentinel_after_write() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("append-removes-lock.json");

        append_message(&path, &sample_message(Uuid::new_v4(), "first")).expect("append");

        assert!(!lock::sentinel_path(&path).exists());
    }

    #[test]
    fn append_message_cleans_preexisting_stale_lock_sentinel() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("append-cleans-stale-lock.json");
        fs::write(lock::sentinel_path(&path), u32::MAX.to_string()).expect("stale lock");

        append_message(&path, &sample_message(Uuid::new_v4(), "first")).expect("append");

        assert!(!lock::sentinel_path(&path).exists());
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
    fn read_messages_rejects_oversized_mailbox_before_loading_contents() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("oversized-mailbox.jsonl");
        File::create(&path)
            .and_then(|file| file.set_len(MAX_MAILBOX_READ_BYTES + 1))
            .expect("oversized mailbox");

        let error = read_messages(&path).expect_err("oversized mailbox should fail");

        assert!(error.is_mailbox_read());
        assert!(error.message.contains("exceeds"));
        assert!(
            error
                .recovery
                .as_deref()
                .is_some_and(|value| value.contains("oversized mailbox"))
        );
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
    fn read_messages_supports_json_array_mailboxes_without_message_id() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("array-no-message-id.json");
        let contents = serde_json::json!([
            {
                "from": "team-lead",
                "text": "from claude array",
                "timestamp": "2026-03-30T00:00:00Z",
                "read": false
            }
        ]);
        fs::write(&path, serde_json::to_vec(&contents).expect("json")).expect("write");

        let messages = read_messages(&path).expect("read");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "from claude array");
        assert!(messages[0].message_id.is_none());
    }

    #[test]
    fn read_messages_supports_json_array_mailboxes_with_atm_fields() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("array-with-atm-fields.json");
        let message = sample_message(Uuid::new_v4(), "array with id");
        fs::write(
            &path,
            serde_json::to_vec(&vec![message.clone()]).expect("json"),
        )
        .expect("write");

        let messages = read_messages(&path).expect("read");
        assert_eq!(messages, vec![message]);
    }

    #[test]
    fn read_messages_hydrates_fields_from_metadata_atm() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("metadata-atm.json");
        fs::write(
            &path,
            r#"{"from":"team-lead","text":"hello","timestamp":"2026-03-30T00:00:00Z","read":false,"summary":"hello","metadata":{"atm":{"messageId":"01JQYVB6W51Q2E7E6T3Y4Q9N2M","sourceTeam":"atm-dev","pendingAckAt":"2026-03-30T00:00:01Z","taskId":"TASK-123"}}}"#,
        )
        .expect("write");

        let messages = read_messages(&path).expect("read");
        assert_eq!(messages.len(), 1);
        assert!(messages[0].message_id.is_some());
        assert_eq!(messages[0].source_team.as_deref(), Some("atm-dev"));
        assert!(messages[0].pending_ack_at.is_some());
        assert_eq!(
            messages[0].task_id.as_ref().map(|task_id| task_id.as_str()),
            Some("TASK-123")
        );
    }

    #[test]
    fn append_message_preserves_both_records_under_concurrent_writers() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("append-message-concurrent.json");
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
        let legacy_message_id = crate::schema::LegacyMessageId::from(message_id);
        let atm_message_id = legacy_message_id.into_lossy_atm_message_id_approximation();
        let message_id = crate::schema::LegacyMessageId::from_atm_message_id(atm_message_id);
        let mut extra = serde_json::Map::new();
        let mut metadata = serde_json::Map::new();
        let mut atm = serde_json::Map::new();
        atm.insert(
            "messageId".to_string(),
            serde_json::Value::String(atm_message_id.to_string()),
        );
        atm.insert(
            "sourceTeam".to_string(),
            serde_json::Value::String("atm-dev".to_string()),
        );
        metadata.insert("atm".to_string(), serde_json::Value::Object(atm));
        extra.insert("metadata".to_string(), serde_json::Value::Object(metadata));

        MessageEnvelope {
            from: "arch-ctm".parse::<AgentName>().expect("agent"),
            text: body.into(),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: Some("atm-dev".parse::<TeamName>().expect("team")),
            summary: None,
            message_id: Some(message_id),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            task_id: None,
            extra,
        }
    }
}
