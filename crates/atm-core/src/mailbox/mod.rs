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
use crate::schema::{AtmMessageId, LegacyMessageId, MessageEnvelope};

const MAX_MAILBOX_READ_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MailboxReadStats {
    pub skipped_records: usize,
    pub malformed_metadata_records: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct MailboxReadReport {
    pub messages: Vec<MessageEnvelope>,
    pub stats: MailboxReadStats,
}
/// Append one message to a shared inbox file under the mailbox lock.
///
/// Production send flows use the same lock discipline through the send-path
/// workflow commit helper. The single-file lock/load/mutate/rewrite boundary
/// itself is a production seam and must remain shared with test coverage.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::MailboxReadFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxWriteFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxLockFailed`], or
/// [`crate::error_codes::AtmErrorCode::MailboxLockTimeout`] when the mailbox
/// cannot be loaded, locked, or atomically replaced.
#[allow(dead_code)]
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
/// `mailbox::store::commit_source_files()`. Unit and integration tests also use
/// this seam directly to validate the shared mailbox lock contract.
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
#[allow(dead_code)]
pub fn locked_read_modify_write<F>(
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
    Ok(read_messages_report(path)?.messages)
}

pub(crate) fn read_messages_report(path: &Path) -> Result<MailboxReadReport, AtmError> {
    if !path.exists() {
        return Ok(MailboxReadReport::default());
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

/// Persist one complete shared inbox file through ATM's canonical write path.
///
/// This is the only supported mailbox export boundary for code that needs to
/// materialize `MessageEnvelope` values onto the Claude-owned inbox surface.
/// Callers should not serialize inbox JSON directly.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::MailboxWriteFailed`] when ATM cannot
/// serialize the envelopes or atomically replace the target inbox file.
pub fn write_messages(path: &Path, messages: &[MessageEnvelope]) -> Result<(), AtmError> {
    store::commit_mailbox_state(path, messages)
}

fn parse_mailbox_contents(raw: &str, path: &Path) -> Result<MailboxReadReport, AtmError> {
    match raw.chars().find(|ch| !ch.is_whitespace()) {
        None => Ok(MailboxReadReport::default()),
        Some('[') => parse_mailbox_array(raw, path),
        Some(_) => Ok(parse_mailbox_jsonl(raw, path)),
    }
}

fn parse_mailbox_array(raw: &str, path: &Path) -> Result<MailboxReadReport, AtmError> {
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

    let mut report = MailboxReadReport::default();
    for (index, mut value) in records.into_iter().enumerate() {
        match parse_mailbox_value(&mut value, path, index + 1) {
            Ok(Some((message, stats))) => {
                report.stats.malformed_metadata_records += stats.malformed_metadata_records;
                report.messages.push(message);
            }
            Ok(None) => {}
            Err(error) => {
                report.stats.skipped_records += 1;
                warn!(
                    code = %AtmErrorCode::WarningMailboxRecordSkipped,
                    line = index + 1,
                    mailbox_path = %path.display(),
                    raw_record = %value,
                    %error,
                    "skipping malformed mailbox record"
                );
            }
        }
    }
    Ok(report)
}

fn parse_mailbox_jsonl(raw: &str, path: &Path) -> MailboxReadReport {
    let mut report = MailboxReadReport::default();
    for (index, line) in raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }

        match parse_mailbox_record(line, path, index + 1) {
            Ok(Some((message, stats))) => {
                report.stats.malformed_metadata_records += stats.malformed_metadata_records;
                report.messages.push(message);
            }
            Ok(None) => {}
            Err(error) => {
                report.stats.skipped_records += 1;
                warn!(
                    code = %AtmErrorCode::WarningMailboxRecordSkipped,
                    line = index + 1,
                    mailbox_path = %path.display(),
                    raw_record = %line,
                    %error,
                    "skipping malformed mailbox record"
                );
            }
        }
    }
    report
}

fn parse_mailbox_record(
    raw_record: &str,
    path: &Path,
    line_number: usize,
) -> Result<Option<(MessageEnvelope, MailboxReadStats)>, serde_json::Error> {
    let mut value = serde_json::from_str::<Value>(raw_record)?;
    parse_mailbox_value(&mut value, path, line_number)
}

fn parse_mailbox_value(
    value: &mut Value,
    path: &Path,
    line_number: usize,
) -> Result<Option<(MessageEnvelope, MailboxReadStats)>, serde_json::Error> {
    let malformed_metadata_records = usize::from(detect_malformed_metadata(value));
    hydrate_legacy_fields_from_metadata(value);
    sanitize_legacy_message_id(value, path, line_number);
    serde_json::from_value::<MessageEnvelope>(value.take()).map(|message| {
        Some((
            message,
            MailboxReadStats {
                skipped_records: 0,
                malformed_metadata_records,
            },
        ))
    })
}

fn detect_malformed_metadata(value: &Value) -> bool {
    let Some(atm) = value
        .get("metadata")
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get("atm"))
        .and_then(Value::as_object)
    else {
        return false;
    };

    atm.get("messageId")
        .and_then(Value::as_str)
        .is_some_and(|raw| raw.parse::<AtmMessageId>().is_err())
        || atm
            .get("acknowledgesMessageId")
            .and_then(Value::as_str)
            .is_some_and(|raw| raw.parse::<AtmMessageId>().is_err())
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

    fn assert_round_trip_matches(actual: &[MessageEnvelope], expected: &[MessageEnvelope]) {
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert_eq!(actual.from, expected.from);
            assert_eq!(actual.text, expected.text);
            assert_eq!(actual.timestamp, expected.timestamp);
            assert_eq!(actual.read, expected.read);
            assert_eq!(actual.source_team, expected.source_team);
            assert_eq!(actual.summary, expected.summary);
            assert_eq!(actual.message_id, expected.message_id);
            assert_eq!(actual.pending_ack_at, expected.pending_ack_at);
            assert_eq!(actual.acknowledged_at, expected.acknowledged_at);
            assert_eq!(
                actual.acknowledges_message_id,
                expected.acknowledges_message_id
            );
            assert_eq!(actual.task_id, expected.task_id);
            assert!(actual.atm_message_id().is_some());
        }
    }

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
        assert_round_trip_matches(&read_back, &[envelope]);
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
            message_id: Some(legacy_message_id),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            task_id: None,
            extra: serde_json::Map::new(),
        }
    }
}
