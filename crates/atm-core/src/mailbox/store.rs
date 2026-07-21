//! Mailbox owner-layer write boundaries for the Claude-owned inbox surface.

use std::path::Path;

#[cfg(test)]
use crate::config;
#[cfg(test)]
use crate::error::AtmError;
#[cfg(test)]
use crate::mailbox::atomic;
#[cfg(test)]
use crate::schema::InboxMessage;
#[cfg(test)]
use crate::schema::inbox_message::SharedAppendPolicy;

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "mailbox format detection remains test-only after compat retirement"
    )
)]
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
#[cfg(test)]
pub(crate) fn write_compat_mailbox_projection(
    path: &Path,
    messages: &[InboxMessage],
) -> Result<(), AtmError> {
    let export_policy = export_policy_for_path(path)?;
    write_compat_mailbox_projection_with_policy(path, messages, export_policy)
}

/// Repair/rebuild only — not reachable from normal runtime send or ack paths.
#[cfg(test)]
fn write_compat_mailbox_projection_with_policy(
    path: &Path,
    messages: &[InboxMessage],
    export_policy: SharedAppendPolicy,
) -> Result<(), AtmError> {
    atomic::write_messages(path, messages, export_policy)
}

#[cfg(test)]
pub(crate) fn export_policy_for_path(path: &Path) -> Result<SharedAppendPolicy, AtmError> {
    let config_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let atm_authored_body_export_max_bytes = config::load_config(config_dir)?
        .map(|config| config.claude_jsonl_body_export_max_bytes)
        .unwrap_or_else(|| SharedAppendPolicy::default().atm_authored_body_export_max_bytes);
    Ok(SharedAppendPolicy {
        atm_authored_body_export_max_bytes,
    })
}

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "mailbox format detection remains test-only after compat retirement"
    )
)]
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

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::write_compat_mailbox_projection;
    use crate::mailbox::load_compat_mailbox_messages;
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

    fn sample_message(from: &str, text: &str) -> InboxMessage {
        let message_id = AtmMessageId::new();

        InboxMessage {
            from: from.parse::<AgentName>().expect("agent name"),
            source_chat_id: None,
            text: text.to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: None,
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
