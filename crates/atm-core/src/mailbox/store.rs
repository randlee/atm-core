//! Mailbox owner-layer write boundaries for the Claude-owned inbox surface.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config;
use crate::error::AtmError;
use crate::mailbox::atomic;
use crate::mailbox::source::{SourceFile, discover_source_paths, load_source_files};
use crate::schema::InboxMessage;
use crate::schema::inbox_message::SharedAppendPolicy;
use crate::types::{AgentName, TeamName};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InboxFileFormat {
    ClaudeJsonArray,
    JsonLines,
    Other,
}

/// Write one compatibility mailbox file projection through the mailbox layer.
///
/// The mailbox layer owns writes to the Claude-owned inbox compatibility
/// surface. Callers should express mailbox intent here instead of reaching
/// down to low-level atomic replacement directly.
///
/// Repair/rebuild only — not reachable from normal runtime send or ack paths.
pub(crate) fn write_compat_mailbox_projection(
    path: &Path,
    messages: &[InboxMessage],
) -> Result<(), AtmError> {
    let export_policy = export_policy_for_path(path)?;
    write_compat_mailbox_projection_with_policy(path, messages, export_policy)
}

/// Repair/rebuild only — not reachable from normal runtime send or ack paths.
fn write_compat_mailbox_projection_with_policy(
    path: &Path,
    messages: &[InboxMessage],
    export_policy: SharedAppendPolicy,
) -> Result<(), AtmError> {
    atomic::write_messages(path, messages, export_policy)
}

/// Write one already-loaded multi-source compatibility inbox projection set.
pub(crate) fn write_compat_source_projections(source_files: &[SourceFile]) -> Result<(), AtmError> {
    let mut export_policy_by_dir = BTreeMap::<PathBuf, SharedAppendPolicy>::new();
    for source in source_files {
        let config_dir = source
            .path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let export_policy = if let Some(policy) = export_policy_by_dir.get(&config_dir).copied() {
            policy
        } else {
            let policy = export_policy_for_path(&source.path)?;
            export_policy_by_dir.insert(config_dir, policy);
            policy
        };
        write_compat_mailbox_projection_with_policy(&source.path, &source.messages, export_policy)?;
    }
    Ok(())
}

pub(crate) fn export_policy_for_path(path: &Path) -> Result<SharedAppendPolicy, AtmError> {
    let config_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let atm_authored_body_export_max_bytes = config::load_config(config_dir)?
        .map(|config| config.claude_jsonl_body_export_max_bytes)
        .unwrap_or_else(|| SharedAppendPolicy::default().atm_authored_body_export_max_bytes);
    Ok(SharedAppendPolicy {
        atm_authored_body_export_max_bytes,
    })
}

pub(crate) fn inbox_file_format(path: &Path) -> InboxFileFormat {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| {
            if value.eq_ignore_ascii_case("json") {
                InboxFileFormat::ClaudeJsonArray
            } else if value.eq_ignore_ascii_case("jsonl") {
                InboxFileFormat::JsonLines
            } else {
                InboxFileFormat::Other
            }
        })
        .unwrap_or(InboxFileFormat::Other)
}

/// Load the current inbox projection set without mailbox locks.
pub(crate) fn load_source_projections(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<SourceFile>, AtmError> {
    let source_paths = discover_source_paths(home_dir, team, agent)?;
    load_source_files(&source_paths)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{write_compat_mailbox_projection, write_compat_source_projections};
    use crate::mailbox::load_compat_mailbox_messages;
    use crate::mailbox::source::SourceFile;
    use crate::schema::{AtmMessageId, InboxMessage};
    use crate::test_support::{TEST_QA, TEST_SENDER};
    use crate::types::{AgentName, IsoTimestamp};

    #[test]
    fn write_compat_mailbox_projection_rewrites_mailbox_array_with_only_new_messages() {
        let tempdir = tempdir().expect("tempdir");
        let path = tempdir.path().join(format!("{TEST_SENDER}.json"));
        std::fs::write(&path, "{\"stale\":true}\n").expect("seed mailbox");
        let messages = vec![
            sample_message(TEST_SENDER, "first replacement"),
            sample_message(TEST_QA, "second replacement"),
        ];

        write_compat_mailbox_projection(&path, &messages).expect("commit mailbox");

        let raw = std::fs::read_to_string(&path).expect("mailbox contents");
        assert!(!raw.contains("stale"));
        let encoded: Vec<serde_json::Value> = serde_json::from_str(&raw).expect("json array");
        assert_eq!(encoded.len(), 2);
        assert!(raw.ends_with('\n'));
        let read_back = load_compat_mailbox_messages(&path).expect("read mailbox");
        assert_eq!(read_back.len(), messages.len());
        assert_eq!(read_back[0].text, messages[0].text);
        assert_eq!(read_back[1].text, messages[1].text);
        assert!(
            read_back
                .iter()
                .all(|message| message.source_team.is_none())
        );
        assert!(
            read_back.iter().all(
                |message| message.pending_ack_at.is_none() && message.acknowledged_at.is_none()
            )
        );
    }

    #[test]
    fn write_compat_mailbox_projection_exports_retrieval_stub_when_config_cap_is_zero() {
        let tempdir = tempdir().expect("tempdir");
        std::fs::write(
            tempdir.path().join(".atm.toml"),
            "[atm]\nclaude_jsonl_body_export_max_bytes = 0\n",
        )
        .expect("config");
        let path = tempdir.path().join(format!("{TEST_SENDER}.json"));
        let mut message = sample_message(TEST_SENDER, "full body retained elsewhere");
        let message_id = message.message_id.expect("message id");
        message.summary = Some("stub summary".to_string());

        write_compat_mailbox_projection(&path, std::slice::from_ref(&message))
            .expect("commit mailbox");

        let raw = std::fs::read_to_string(&path).expect("mailbox contents");
        let encoded: Vec<serde_json::Value> = serde_json::from_str(&raw).expect("json array");
        assert_eq!(
            encoded[0]["text"],
            serde_json::Value::String(format!("atm read --message-id {message_id}"))
        );
        assert_eq!(
            encoded[0]["summary"],
            serde_json::Value::String("stub summary".into())
        );
    }

    #[test]
    fn write_compat_source_projections_commits_each_source_path() {
        let tempdir = tempdir().expect("tempdir");
        let left_path = tempdir.path().join(format!("{TEST_SENDER}.json"));
        let right_path = tempdir.path().join(format!("{TEST_QA}.json"));
        let left_messages = vec![sample_message(TEST_SENDER, "left message")];
        let right_messages = vec![
            sample_message(TEST_SENDER, "right first"),
            sample_message(TEST_QA, "right second"),
        ];

        write_compat_source_projections(&[
            SourceFile {
                path: left_path.clone(),
                messages: left_messages.clone(),
            },
            SourceFile {
                path: right_path.clone(),
                messages: right_messages.clone(),
            },
        ])
        .expect("commit source files");

        let left = load_compat_mailbox_messages(&left_path).expect("left inbox");
        let right = load_compat_mailbox_messages(&right_path).expect("right inbox");
        assert_eq!(left.len(), left_messages.len());
        assert_eq!(right.len(), right_messages.len());
        assert_eq!(left[0].text, left_messages[0].text);
        assert_eq!(right[0].text, right_messages[0].text);
        assert_eq!(right[1].text, right_messages[1].text);
        assert!(left.iter().all(|message| message.source_team.is_none()));
        assert!(right.iter().all(|message| message.source_team.is_none()));
    }

    #[test]
    fn write_compat_source_projections_stops_after_first_write_error() {
        let tempdir = tempdir().expect("tempdir");
        let first_path = tempdir.path().join("first.json");
        let invalid_path = tempdir.path().to_path_buf();
        let later_path = tempdir.path().join("later.json");

        let error = write_compat_source_projections(&[
            SourceFile {
                path: first_path.clone(),
                messages: vec![sample_message(TEST_SENDER, "first")],
            },
            SourceFile {
                path: invalid_path,
                messages: vec![sample_message(TEST_QA, "broken")],
            },
            SourceFile {
                path: later_path.clone(),
                messages: vec![sample_message(TEST_SENDER, "later")],
            },
        ])
        .expect_err("write failure");

        assert!(error.is_mailbox_write());
        assert_eq!(
            load_compat_mailbox_messages(&first_path)
                .expect("first inbox")
                .len(),
            1
        );
        assert!(!later_path.exists());
    }

    fn sample_message(from: &str, text: &str) -> InboxMessage {
        let message_id = AtmMessageId::new();

        InboxMessage {
            from: from.parse::<AgentName>().expect("agent name"),
            text: text.to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: None,
            summary: None,
            message_id: Some(message_id),
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
