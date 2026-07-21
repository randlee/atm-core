//! Mailbox read/write helpers, compatibility parsing, and lock-scoped mutation.

pub(crate) mod atomic;
pub(crate) mod hash;
pub(crate) mod source;
pub(crate) mod store;
pub(crate) mod surface;

use serde_json::Value;
use std::fs;
use std::path::Path;
use tracing::warn;

use crate::error::{AtmError, AtmErrorCode};
use crate::schema::{AtmMessageId, InboxMessage};
const MAX_MAILBOX_READ_BYTES: u64 = 10 * 1024 * 1024;
const DEGRADED_RAW_FRAGMENT_MAX_CHARS: usize = 512;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum InboxReadItem {
    Message(Box<InboxMessage>),
    Degraded {
        summary: String,
        warning: String,
        raw_fragment: Option<String>,
    },
}

/// Append one message through the surviving mailbox projection rules.
///
/// This helper stays test-only because production callers persist mailbox state
/// through the retained SQLite runtime and delivery-policy routing.
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
pub fn append_message(path: &Path, envelope: &InboxMessage) -> Result<(), AtmError> {
    let export_policy = store::export_policy_for_path(path)?;
    if store::inbox_file_format(path) == store::InboxFileFormat::ClaudeJsonArray {
        let existing_messages = if path.exists() {
            load_compat_mailbox_messages_strict(path)?
        } else {
            Vec::new()
        };
        return atomic::write_message_iter(
            path,
            existing_messages.iter().chain(std::iter::once(envelope)),
            export_policy,
        );
    }

    atomic::append_message(path, envelope, export_policy)
}

/// Read all valid mailbox records from one shared inbox file.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::MailboxReadFailed`] when the mailbox
/// file cannot be opened or read.
pub(crate) fn load_compat_mailbox_messages(path: &Path) -> Result<Vec<InboxMessage>, AtmError> {
    Ok(load_compat_mailbox_items(path)?
        .into_iter()
        .filter_map(|item| match item {
            InboxReadItem::Message(message) => Some(*message),
            InboxReadItem::Degraded {
                summary,
                warning,
                raw_fragment,
            } => {
                warn!(
                    code = %AtmErrorCode::WarningMailboxRecordSkipped,
                    mailbox_path = %path.display(),
                    summary,
                    warning,
                    raw_fragment = raw_fragment.as_deref().unwrap_or("<none>"),
                    "mailbox read recovered valid messages while skipping malformed fragment"
                );
                None
            }
        })
        .collect())
}

pub(crate) fn load_compat_mailbox_items(path: &Path) -> Result<Vec<InboxReadItem>, AtmError> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file_size = fs::metadata(path).map_err(|error| {
        AtmError::new(
            AtmErrorCode::MailboxReadFailed,
            format!("failed to inspect mailbox file {}: {error}", path.display()),
        )
    })?;
    if file_size.len() > MAX_MAILBOX_READ_BYTES {
        return Err(AtmError::new(
            AtmErrorCode::MailboxReadFailed,
            format!(
                "mailbox file {} exceeds the {}-byte read limit",
                path.display(),
                MAX_MAILBOX_READ_BYTES
            ),
        ));
    }

    let raw = fs::read_to_string(path).map_err(|error| {
        AtmError::new(
            AtmErrorCode::MailboxReadFailed,
            format!("failed to read mailbox file {}: {error}", path.display()),
        )
    })?;

    parse_mailbox_contents(&raw, path)
}

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "strict mailbox compatibility reads remain test-only"
    )
)]
pub(crate) fn load_compat_mailbox_messages_strict(
    path: &Path,
) -> Result<Vec<InboxMessage>, AtmError> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file_size = fs::metadata(path).map_err(|error| {
        AtmError::new(
            AtmErrorCode::MailboxReadFailed,
            format!("failed to inspect mailbox file {}: {error}", path.display()),
        )
    })?;
    if file_size.len() > MAX_MAILBOX_READ_BYTES {
        return Err(AtmError::new(
            AtmErrorCode::MailboxReadFailed,
            format!(
                "mailbox file {} exceeds the {}-byte read limit",
                path.display(),
                MAX_MAILBOX_READ_BYTES
            ),
        ));
    }

    let raw = fs::read_to_string(path).map_err(|error| {
        AtmError::new(
            AtmErrorCode::MailboxReadFailed,
            format!("failed to read mailbox file {}: {error}", path.display()),
        )
    })?;

    parse_mailbox_contents_strict(&raw, path)
}

fn parse_mailbox_contents(raw: &str, path: &Path) -> Result<Vec<InboxReadItem>, AtmError> {
    match raw.chars().find(|ch| !ch.is_whitespace()) {
        None => Ok(Vec::new()),
        Some('[') => parse_mailbox_array(raw, path),
        Some(_) => Ok(parse_mailbox_jsonl(raw, path)),
    }
}

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "strict mailbox compatibility reads remain test-only"
    )
)]
fn parse_mailbox_contents_strict(raw: &str, path: &Path) -> Result<Vec<InboxMessage>, AtmError> {
    match raw.chars().find(|ch| !ch.is_whitespace()) {
        None => Ok(Vec::new()),
        Some('[') => parse_mailbox_array_strict(raw, path),
        Some(_) => Ok(parse_mailbox_jsonl(raw, path)
            .into_iter()
            .filter_map(|item| match item {
                InboxReadItem::Message(message) => Some(*message),
                InboxReadItem::Degraded { .. } => None,
            })
            .collect()),
    }
}

fn parse_mailbox_array(raw: &str, path: &Path) -> Result<Vec<InboxReadItem>, AtmError> {
    match serde_json::from_str::<Vec<Value>>(raw) {
        Ok(records) => Ok(records
            .into_iter()
            .enumerate()
            .map(|(index, mut value)| parse_mailbox_item(&mut value, path, index + 1))
            .collect()),
        Err(error) => salvage_mailbox_array(raw, path, error),
    }
}

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "strict mailbox compatibility reads remain test-only"
    )
)]
fn parse_mailbox_array_strict(raw: &str, path: &Path) -> Result<Vec<InboxMessage>, AtmError> {
    let records = serde_json::from_str::<Vec<Value>>(raw).map_err(|error| {
        AtmError::new(
            AtmErrorCode::MailboxReadFailed,
            format!("failed to parse mailbox array {}: {error}", path.display()),
        )
    })?;

    Ok(records
        .into_iter()
        .enumerate()
        .filter_map(
            |(index, mut value)| match parse_mailbox_item(&mut value, path, index + 1) {
                InboxReadItem::Message(message) => Some(*message),
                InboxReadItem::Degraded {
                    summary,
                    warning,
                    raw_fragment,
                } => {
                    warn!(
                        code = %AtmErrorCode::WarningMailboxRecordSkipped,
                        mailbox_path = %path.display(),
                        line = index + 1,
                        summary,
                        warning,
                        raw_fragment = raw_fragment.as_deref().unwrap_or("<none>"),
                        "strict mailbox parse skipped malformed record"
                    );
                    None
                }
            },
        )
        .collect())
}

fn parse_mailbox_jsonl(raw: &str, path: &Path) -> Vec<InboxReadItem> {
    raw.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            if line.trim().is_empty() {
                return None;
            }

            match parse_mailbox_record(line, path, index + 1) {
                Ok(item) => Some(item),
                Err(error) => Some(InboxReadItem::Degraded {
                    summary: format!(
                        "malformed JSONL mailbox record skipped at {}:{}",
                        path.display(),
                        index + 1
                    ),
                    warning: error.into_message(),
                    raw_fragment: Some(truncate_raw_fragment(line)),
                }),
            }
        })
        .collect()
}

fn parse_mailbox_record(
    raw_record: &str,
    path: &Path,
    line_number: usize,
) -> Result<InboxReadItem, AtmError> {
    let mut value = serde_json::from_str::<Value>(raw_record)
        .map_err(|error| mailbox_record_parse_error(path, line_number, error))?;
    Ok(parse_mailbox_item(&mut value, path, line_number))
}

fn parse_mailbox_item(value: &mut Value, path: &Path, line_number: usize) -> InboxReadItem {
    let raw_fragment = Some(truncate_raw_fragment(&value.to_string()));
    sanitize_message_id(value, path, line_number);
    strip_metadata_atm_namespace(value);
    match serde_json::from_value::<InboxMessage>(value.take()) {
        Ok(message) => InboxReadItem::Message(Box::new(message)),
        Err(error) => InboxReadItem::Degraded {
            summary: format!(
                "malformed mailbox record skipped at {}:{}",
                path.display(),
                line_number
            ),
            warning: mailbox_record_parse_error(path, line_number, error).into_message(),
            raw_fragment,
        },
    }
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
            expected_format = "ULID string",
            raw_value = %raw_message_id,
            "treating malformed ATM-owned field as absent during mailbox read"
        );
        object.remove("message_id");
    }
}

fn strip_metadata_atm_namespace(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };

    let Some(metadata) = object.get_mut("metadata").and_then(Value::as_object_mut) else {
        return;
    };

    metadata.remove("atm");
    if metadata.is_empty() {
        object.remove("metadata");
    }
}

fn mailbox_record_parse_error(
    path: &Path,
    line_number: usize,
    error: serde_json::Error,
) -> AtmError {
    AtmError::new(
        AtmErrorCode::MailboxReadFailed,
        format!(
            "failed to parse mailbox JSONL record {}:{}: {error}",
            path.display(),
            line_number
        ),
    )
}

fn parse_salvaged_array_fragment(raw: &str, path: &Path, object_index: usize) -> InboxReadItem {
    match serde_json::from_str::<Value>(raw) {
        Ok(mut value) => parse_mailbox_item(&mut value, path, object_index),
        Err(error) => InboxReadItem::Degraded {
            summary: format!(
                "malformed mailbox array fragment skipped at {} object {}",
                path.display(),
                object_index
            ),
            warning: mailbox_record_parse_error(path, object_index, error).into_message(),
            // bounded: one degraded fragment clone is capped independently
            raw_fragment: Some(truncate_raw_fragment(raw)),
        },
    }
}

fn push_salvaged_array_fragment(
    items: &mut Vec<InboxReadItem>,
    raw: &str,
    path: &Path,
    object_index: &mut usize,
    start: usize,
    end: usize,
) {
    *object_index += 1;
    items.push(parse_salvaged_array_fragment(
        &raw[start..=end],
        path,
        *object_index,
    ));
}

fn truncated_array_fragment(path: &Path, object_index: usize, raw_fragment: &str) -> InboxReadItem {
    InboxReadItem::Degraded {
        summary: format!(
            "truncated mailbox array fragment skipped at {} object {}",
            path.display(),
            object_index
        ),
        warning: format!(
            "mailbox array {} ended before object {} closed",
            path.display(),
            object_index
        ),
        raw_fragment: Some(truncate_raw_fragment(raw_fragment)),
    }
}

fn mailbox_array_parse_error(path: &Path, parse_error: serde_json::Error) -> AtmError {
    AtmError::new(
        AtmErrorCode::MailboxReadFailed,
        format!(
            "failed to parse mailbox array {}: {parse_error}",
            path.display()
        ),
    )
}

fn mailbox_array_recovery_banner(path: &Path, parse_error: &serde_json::Error) -> InboxReadItem {
    InboxReadItem::Degraded {
        summary: format!("mailbox array recovery activated for {}", path.display()),
        warning: format!(
            "ATM recovered valid message objects from malformed mailbox array {} after parse failure: {}",
            path.display(),
            parse_error
        ),
        raw_fragment: None,
    }
}

fn salvage_mailbox_array(
    raw: &str,
    path: &Path,
    parse_error: serde_json::Error,
) -> Result<Vec<InboxReadItem>, AtmError> {
    let mut items = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut depth = 0usize;
    let mut object_start = None;
    let mut object_index = 0usize;

    for (offset, ch) in raw.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    object_start = Some(offset);
                }
                depth += 1;
            }
            '}' => {
                if depth == 0 {
                    continue;
                }
                depth -= 1;
                if depth == 0
                    && let Some(start) = object_start.take()
                {
                    push_salvaged_array_fragment(
                        &mut items,
                        raw,
                        path,
                        &mut object_index,
                        start,
                        offset,
                    );
                }
            }
            _ => {}
        }
    }

    if let Some(start) = object_start {
        object_index += 1;
        items.push(truncated_array_fragment(path, object_index, &raw[start..]));
    }

    if items.is_empty() {
        return Err(mailbox_array_parse_error(path, parse_error));
    }

    items.insert(0, mailbox_array_recovery_banner(path, &parse_error));
    Ok(items)
}

fn truncate_raw_fragment(raw: &str) -> String {
    raw.chars().take(DEGRADED_RAW_FRAGMENT_MAX_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::sync::{Arc, Barrier};
    use std::thread;

    use chrono::{TimeZone, Utc};
    use tempfile::TempDir;

    use crate::schema::{AtmMessageId, InboxMessage};
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, IsoTimestamp, TeamName};

    use super::{
        InboxReadItem, MAX_MAILBOX_READ_BYTES, append_message, load_compat_mailbox_items,
        load_compat_mailbox_messages,
    };

    #[test]
    fn append_message_persists_one_jsonl_record() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("append-message.jsonl");
        let envelope = sample_message(AtmMessageId::new(), "first");

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
        let path = tempdir.path().join("append-message-top-level.jsonl");
        let envelope = sample_message(AtmMessageId::new(), "first");

        append_message(&path, &envelope).expect("append");

        let raw = fs::read_to_string(&path).expect("raw contents");
        let value: serde_json::Value = serde_json::from_str(raw.trim_end()).expect("json line");
        let object = value.as_object().expect("message object");
        assert!(!object.contains_key("metadata"));
        assert!(object.contains_key("message_id"));
        assert!(!object.contains_key("source_team"));
    }

    #[test]
    fn load_compat_mailbox_messages_skips_malformed_lines() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("skip-malformed.jsonl");
        let valid = serde_json::to_string(&sample_message(AtmMessageId::new(), "valid"))
            .expect("valid json");
        fs::write(&path, format!("{valid}\n{{not-json}}\n")).expect("write");

        let messages = load_compat_mailbox_messages(&path).expect("read");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "valid");
    }

    #[test]
    fn load_compat_mailbox_items_reports_malformed_jsonl_lines_without_hiding_valid_messages() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("jsonl-degraded-items.jsonl");
        let valid = serde_json::to_string(&sample_message(AtmMessageId::new(), "valid"))
            .expect("valid json");
        fs::write(&path, format!("{valid}\n{{not-json}}\n")).expect("write");

        let items = load_compat_mailbox_items(&path).expect("read items");
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[0], InboxReadItem::Message(message) if message.text == "valid"));
        assert!(matches!(
            &items[1],
            InboxReadItem::Degraded { summary, raw_fragment, .. }
            if summary.contains("malformed JSONL mailbox record skipped")
                && raw_fragment.as_deref() == Some("{not-json}")
        ));
    }

    #[test]
    fn read_messages_jsonl_format_still_works() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("jsonl-ingress-still-works.jsonl");
        let first = sample_message(AtmMessageId::new(), "first");
        let second = sample_message(AtmMessageId::new(), "second");
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
        let first = sample_message(AtmMessageId::new(), "first");
        let second = sample_message(AtmMessageId::new(), "second");
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

        assert!(error.code() == crate::error_codes::AtmErrorCode::MailboxReadFailed);
        assert!(error.message().contains("exceeds"));
        assert!(error.message().contains("Recovery:"));
    }

    #[test]
    fn load_compat_mailbox_messages_preserves_duplicate_message_ids_for_surface_canonicalization() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("dedupe.jsonl");
        let message_id = AtmMessageId::new();
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
            "from": TEST_SENDER,
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
                "from": TEST_SENDER,
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
        let message = sample_message(AtmMessageId::new(), "array with id");
        fs::write(
            &path,
            serde_json::to_vec(&vec![message.clone()]).expect("json"),
        )
        .expect("write");

        let messages = load_compat_mailbox_messages(&path).expect("read");
        assert_eq!(messages, vec![message]);
    }

    #[test]
    fn load_compat_mailbox_items_salvages_valid_messages_around_malformed_array_fragment() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("array-salvage-middle.json");
        let first =
            serde_json::to_string(&sample_message(AtmMessageId::new(), "first")).expect("json");
        let third =
            serde_json::to_string(&sample_message(AtmMessageId::new(), "third")).expect("json");
        fs::write(&path, format!("[{first}, {{not-json}}, {third}]")).expect("write");

        let items = load_compat_mailbox_items(&path).expect("read items");
        let texts = items
            .iter()
            .filter_map(|item| match item {
                InboxReadItem::Message(message) => Some(message.text.clone()),
                InboxReadItem::Degraded { .. } => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(texts, vec!["first".to_string(), "third".to_string()]);
        assert!(items.iter().any(|item| matches!(
            item,
            InboxReadItem::Degraded { summary, raw_fragment, .. }
            if summary.contains("mailbox array recovery activated")
                || raw_fragment.as_deref() == Some("{not-json}")
        )));
    }

    #[test]
    fn load_compat_mailbox_items_salvages_valid_messages_before_truncated_array_tail() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("array-salvage-tail.json");
        let first =
            serde_json::to_string(&sample_message(AtmMessageId::new(), "first")).expect("json");
        fs::write(&path, format!("[{first}, {{\"from\":\"broken\"")).expect("write");

        let items = load_compat_mailbox_items(&path).expect("read items");
        assert!(matches!(&items[1], InboxReadItem::Message(message) if message.text == "first"));
        assert!(matches!(
            items.last().expect("last item"),
            InboxReadItem::Degraded { summary, .. }
            if summary.contains("truncated mailbox array fragment skipped")
        ));
    }

    #[test]
    fn load_compat_mailbox_items_rejects_unreadable_array_without_segmentable_objects() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("array-terminal-error.json");
        fs::write(&path, "[not-even-one-object").expect("write");

        let error = load_compat_mailbox_items(&path).expect_err("terminal malformed array");
        assert!(error.code() == crate::error_codes::AtmErrorCode::MailboxReadFailed);
        assert!(error.message().contains("failed to parse mailbox array"));
    }

    #[test]
    fn load_compat_mailbox_messages_use_top_level_compatibility_fields() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("top-level-compatibility.json");
        let contents = serde_json::json!({
            "from": TEST_SENDER,
            "text": "hello",
            "timestamp": "2026-03-30T00:00:00Z",
            "read": false,
            "summary": "hello",
            "message_id": "01KRFK5QTF2R6NRS3Q0F8Z9K0S",
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
    fn load_compat_mailbox_messages_ignores_metadata_atm_but_keeps_foreign_metadata() {
        let tempdir = TempDir::new().expect("tempdir");
        let path = tempdir.path().join("metadata-atm-pass-through.jsonl");
        let contents = serde_json::json!({
            "from": TEST_SENDER,
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
                "foreign": {
                    "keep": true
                }
            }))
        );
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
                let envelope = sample_message(AtmMessageId::new(), body);
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

    fn sample_message(message_id: AtmMessageId, body: &str) -> InboxMessage {
        InboxMessage {
            from: TEST_SENDER.parse::<AgentName>().expect("agent"),
            source_chat_id: None,
            text: body.into(),
            timestamp: IsoTimestamp::from_datetime(
                Utc.with_ymd_and_hms(2026, 3, 30, 0, 0, 0)
                    .single()
                    .expect("timestamp"),
            ),
            read: false,
            source_team: Some(TEST_TEAM.parse::<TeamName>().expect("team")),
            destination_chat_id: None,
            summary: None,
            message_id: Some(message_id),
            requires_ack: false,
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
