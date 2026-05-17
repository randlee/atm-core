//! Mailbox read/write helpers, compatibility parsing, and lock-scoped mutation.

pub(crate) mod atomic;
pub(crate) mod hash;
pub(crate) mod lock;
pub(crate) mod source;
pub(crate) mod store;
pub(crate) mod surface;

use serde_json::Value;
use std::fs;
use std::path::Path;
use tracing::warn;

use crate::error::{AtmError, AtmErrorCode, AtmErrorKind};
use crate::mailbox::source::SourceFile;
use crate::schema::{AtmMessageId, MessageEnvelope};
use crate::types::{AgentName, TeamName};

const MAX_MAILBOX_READ_BYTES: u64 = 10 * 1024 * 1024;
/// Append one message to a shared inbox file as one JSONL record.
///
/// Production send flows use the same append-only compatibility writer through
/// the retained runtime boundary. This helper stays test-only because
/// production callers must also coordinate workflow persistence and delivery
/// policy routing.
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
    store::append_compat_mailbox_message(path, envelope)
}

/// Lock, load, mutate, and atomically rewrite one mailbox file.
///
/// Production mutation paths use equivalent lock coverage through
/// `workflow::commit_workflow_state()` plus
/// `mailbox::store::write_compat_source_projections()`.
/// This helper stays test-only so unit tests can exercise the shared mailbox
/// lock contract directly without the workflow/state sidecars required in
/// production commands.
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
    let mut messages = load_compat_mailbox_messages(path)?;
    mutate(&mut messages)?;
    // ATM accepts Claude-authored JSONL as ingress, but test-only mutations
    // rewrite through the same array-shaped compatibility projection ATM uses
    // for its own exports.
    store::write_compat_mailbox_projection(path, &messages)
}

/// Read all valid mailbox records from one shared inbox file.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::MailboxReadFailed`] when the mailbox
/// file cannot be opened or read.
pub(crate) fn load_compat_mailbox_messages(path: &Path) -> Result<Vec<MessageEnvelope>, AtmError> {
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

pub(crate) fn import_source_projections(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<SourceFile>, AtmError> {
    store::load_source_projections(home_dir, team, agent)
}

pub(crate) fn export_compat_source_projections(
    source_files: &[SourceFile],
) -> Result<(), AtmError> {
    store::write_compat_source_projections(source_files)
}

pub(crate) fn export_compat_mailbox_projection(
    path: &Path,
    messages: &[MessageEnvelope],
) -> Result<(), AtmError> {
    store::write_compat_mailbox_projection(path, messages)
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
) -> Result<Option<MessageEnvelope>, AtmError> {
    let mut value = serde_json::from_str::<Value>(raw_record)
        .map_err(|error| mailbox_record_parse_error(path, line_number, error))?;
    parse_mailbox_value(&mut value, path, line_number)
}

fn parse_mailbox_value(
    value: &mut Value,
    path: &Path,
    line_number: usize,
) -> Result<Option<MessageEnvelope>, AtmError> {
    sanitize_message_id(value, path, line_number);
    serde_json::from_value::<MessageEnvelope>(value.take())
        .map(Some)
        .map_err(|error| mailbox_record_parse_error(path, line_number, error))
}

fn sanitize_message_id(value: &mut Value, path: &Path, line_number: usize) {
    let Some(object) = value.as_object_mut() else {
        return;
    };

    let Some(raw_message_id) = object.get("message_id").cloned() else {
        return;
    };

    if raw_message_id.is_null() {
        return;
    }

    let valid_message_id = raw_message_id
        .as_str()
        .and_then(|value| value.parse::<AtmMessageId>().ok())
        .is_some();

    if !valid_message_id {
        warn!(
            code = %AtmErrorCode::WarningMalformedAtmFieldIgnored,
            mailbox_path = %path.display(),
            line = line_number,
            field = "message_id",
            expected_format = "ULID or UUID wire string",
            raw_value = %raw_message_id,
            "treating malformed ATM-owned field as absent during mailbox read"
        );
        object.remove("message_id");
    }
}

fn mailbox_record_parse_error(
    path: &Path,
    line_number: usize,
    error: serde_json::Error,
) -> AtmError {
    AtmError::new(
        AtmErrorKind::MailboxRead,
        format!(
            "failed to parse mailbox JSONL record {}:{}: {error}",
            path.display(),
            line_number
        ),
    )
    .with_source(error)
    .with_recovery("Inspect the mailbox file for malformed JSON records or partial writes, then retry atm read. If corruption persists, archive or remove the malformed mailbox file.")
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::sync::{Arc, Barrier};
    use std::thread;

    use chrono::{TimeZone, Utc};
    use tempfile::TempDir;
    use uuid::Uuid;

    use crate::roles::ROLE_TEAM_LEAD;
    use crate::schema::MessageEnvelope;
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, IsoTimestamp, TeamName};

    use super::{
        MAX_MAILBOX_READ_BYTES, append_message, load_compat_mailbox_messages,
        locked_read_modify_write,
    };
    use crate::mailbox::lock;

    #[test]
    fn append_message_persists_one_jsonl_record() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("append-message.json");
        let envelope = sample_message(Uuid::new_v4(), "first");

        append_message(&path, &envelope).expect("append");

        let raw = fs::read_to_string(&path).expect("raw contents");
        assert_eq!(raw.lines().count(), 1);
        let read_back = load_compat_mailbox_messages(&path).expect("read back");
        assert_eq!(read_back.len(), 1);
        assert_eq!(read_back[0].text, envelope.text);
        assert_eq!(read_back[0].message_id, envelope.message_id);
        assert!(read_back[0].source_team.is_none());
    }

    #[test]
    fn append_message_keeps_approved_machine_fields_top_level() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("append-message-top-level.json");
        let envelope = sample_message(Uuid::new_v4(), "first");

        append_message(&path, &envelope).expect("append");

        let raw = fs::read_to_string(&path).expect("raw contents");
        let value: serde_json::Value = serde_json::from_str(raw.trim_end()).expect("json line");
        let object = value.as_object().expect("message object");
        assert!(!object.contains_key("metadata"));
        assert!(object.contains_key("message_id"));
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

        let messages = load_compat_mailbox_messages(&path).expect("read");
        assert_eq!(messages.len(), 2);
        assert!(messages[0].read);
        assert_eq!(messages[1].text, "second");
    }

    #[test]
    fn append_message_does_not_create_lock_sentinel() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("append-removes-lock.json");

        assert!(!lock::sentinel_path(&path).exists());
        append_message(&path, &sample_message(Uuid::new_v4(), "first")).expect("append");

        assert!(!lock::sentinel_path(&path).exists());
    }

    #[test]
    fn append_message_does_not_remove_preexisting_lock_sentinel() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("append-cleans-stale-lock.json");
        fs::write(lock::sentinel_path(&path), u32::MAX.to_string()).expect("stale lock");

        append_message(&path, &sample_message(Uuid::new_v4(), "first")).expect("append");

        assert!(lock::sentinel_path(&path).exists());
    }

    #[test]
    fn load_compat_mailbox_messages_skips_malformed_lines() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("skip-malformed.jsonl");
        let valid =
            serde_json::to_string(&sample_message(Uuid::new_v4(), "valid")).expect("valid json");
        fs::write(&path, format!("{valid}\n{{not-json}}\n")).expect("write");

        let messages = load_compat_mailbox_messages(&path).expect("read");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "valid");
    }

    #[test]
    fn read_messages_jsonl_format_still_works() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("jsonl-ingress-still-works.jsonl");
        let first = sample_message(Uuid::new_v4(), "first");
        let second = sample_message(Uuid::new_v4(), "second");
        let contents = format!(
            "{}\n{}\n",
            serde_json::to_string(&first).expect("json"),
            serde_json::to_string(&second).expect("json")
        );
        fs::write(&path, contents).expect("write");

        let messages = load_compat_mailbox_messages(&path).expect("read");
        assert_eq!(messages, vec![first, second]);
    }

    #[test]
    fn append_message_appends_after_existing_jsonl_record() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("jsonl-appends-second-record.jsonl");
        let first = sample_message(Uuid::new_v4(), "first");
        let second = sample_message(Uuid::new_v4(), "second");
        fs::write(
            &path,
            format!("{}\n", serde_json::to_string(&first).expect("json line")),
        )
        .expect("write");

        append_message(&path, &second).expect("append");

        let raw = fs::read_to_string(&path).expect("raw contents");
        assert_eq!(raw.lines().count(), 2);
        let messages = load_compat_mailbox_messages(&path).expect("read");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].text, first.text);
        assert_eq!(messages[0].message_id, first.message_id);
        assert_eq!(messages[0].source_team, first.source_team);
        assert_eq!(messages[1].text, second.text);
        assert_eq!(messages[1].message_id, second.message_id);
        assert!(messages[1].source_team.is_none());
    }

    #[test]
    fn load_compat_mailbox_messages_rejects_oversized_mailbox_before_loading_contents() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("oversized-mailbox.jsonl");
        File::create(&path)
            .and_then(|file| file.set_len(MAX_MAILBOX_READ_BYTES + 1))
            .expect("oversized mailbox");

        let error = load_compat_mailbox_messages(&path).expect_err("oversized mailbox should fail");

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
    fn load_compat_mailbox_messages_preserves_duplicate_message_ids_for_surface_canonicalization() {
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

        let messages = load_compat_mailbox_messages(&path).expect("read");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].text, "first");
        assert_eq!(messages[1].text, "second");
    }

    #[test]
    fn load_compat_mailbox_messages_treats_malformed_message_id_as_absent() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("malformed-message-id.jsonl");
        let contents = serde_json::json!({
            "from": ROLE_TEAM_LEAD,
            "text": "valid body",
            "timestamp": "2026-03-30T00:00:00Z",
            "read": false,
            "message_id": "not-a-valid-message-id"
        });
        fs::write(
            &path,
            format!("{}\n", serde_json::to_string(&contents).expect("json")),
        )
        .expect("write");

        let messages = load_compat_mailbox_messages(&path).expect("read");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "valid body");
        assert!(messages[0].message_id.is_none());
    }

    #[test]
    fn load_compat_mailbox_messages_supports_json_array_mailboxes_without_message_id() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("array-no-message-id.json");
        let contents = serde_json::json!([
            {
                "from": ROLE_TEAM_LEAD,
                "text": "from claude array",
                "timestamp": "2026-03-30T00:00:00Z",
                "read": false
            }
        ]);
        fs::write(&path, serde_json::to_vec(&contents).expect("json")).expect("write");

        let messages = load_compat_mailbox_messages(&path).expect("read");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "from claude array");
        assert!(messages[0].message_id.is_none());
    }

    #[test]
    fn load_compat_mailbox_messages_supports_json_array_mailboxes_with_atm_fields() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("array-with-atm-fields.json");
        let message = sample_message(Uuid::new_v4(), "array with id");
        fs::write(
            &path,
            serde_json::to_vec(&vec![message.clone()]).expect("json"),
        )
        .expect("write");

        let messages = load_compat_mailbox_messages(&path).expect("read");
        assert_eq!(messages, vec![message]);
    }

    #[test]
    fn load_compat_mailbox_messages_use_top_level_compatibility_fields() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("top-level-compatibility.json");
        let contents = serde_json::json!({
            "from": ROLE_TEAM_LEAD,
            "text": "hello",
            "timestamp": "2026-03-30T00:00:00Z",
            "read": false,
            "summary": "hello",
            "message_id": "11111111-1111-4111-8111-111111111111",
            "source_team": TEST_TEAM,
            "pendingAckAt": "2026-03-30T00:00:01Z",
            "taskId": "TASK-123"
        });
        fs::write(&path, serde_json::to_vec(&contents).expect("json")).expect("write");

        let messages = load_compat_mailbox_messages(&path).expect("read");
        assert_eq!(messages.len(), 1);
        assert!(messages[0].message_id.is_some());
        assert_eq!(messages[0].source_team.as_deref(), Some(TEST_TEAM));
        assert!(messages[0].pending_ack_at.is_some());
        assert_eq!(
            messages[0].task_id.as_ref().map(|task_id| task_id.as_str()),
            Some("TASK-123")
        );
    }

    #[test]
    fn load_compat_mailbox_messages_preserves_metadata_atm_as_opaque_extra() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("metadata-atm-pass-through.jsonl");
        let contents = serde_json::json!({
            "from": ROLE_TEAM_LEAD,
            "text": "hello",
            "timestamp": "2026-03-30T00:00:00Z",
            "read": false,
            "metadata": {
                "atm": {
                    "messageId": "01JQYVB6W51Q2E7E6T3Y4Q9N2M",
                    "sourceTeam": TEST_TEAM
                },
                "foreign": {
                    "keep": true
                }
            }
        });
        fs::write(&path, serde_json::to_vec(&contents).expect("json")).expect("write");

        let messages = load_compat_mailbox_messages(&path).expect("read");
        assert_eq!(messages.len(), 1);
        assert!(messages[0].message_id.is_none());
        assert!(messages[0].source_team.is_none());
        assert_eq!(
            messages[0].extra.get("metadata"),
            Some(&serde_json::json!({
                "atm": {
                    "messageId": "01JQYVB6W51Q2E7E6T3Y4Q9N2M",
                    "sourceTeam": TEST_TEAM
                },
                "foreign": {
                    "keep": true
                }
            }))
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

        let messages = load_compat_mailbox_messages(&path).expect("read");
        assert_eq!(messages.len(), 2);
        assert!(messages.iter().any(|message| message.text == "first"));
        assert!(messages.iter().any(|message| message.text == "second"));
    }

    fn sample_message(message_id: Uuid, body: &str) -> MessageEnvelope {
        let atm_message_id = crate::schema::AtmMessageId::from(message_id);

        MessageEnvelope {
            from: TEST_SENDER.parse::<AgentName>().expect("agent"),
            text: body.into(),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: Some(TEST_TEAM.parse::<TeamName>().expect("team")),
            summary: None,
            message_id: Some(atm_message_id),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            extra: serde_json::Map::new(),
        }
    }
}
