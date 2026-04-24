//! Mailbox owner-layer write boundaries for the Claude-owned inbox surface.

use std::path::Path;

use crate::error::AtmError;
use crate::mailbox::atomic;
use crate::mailbox::source::SourceFile;
use crate::schema::MessageEnvelope;

/// Commit one mailbox file through the mailbox-layer write boundary.
///
/// The mailbox layer owns writes to the Claude-owned inbox compatibility
/// surface. Callers should express mailbox intent here instead of reaching
/// down to low-level atomic replacement directly.
pub(crate) fn commit_mailbox_state(
    path: &Path,
    messages: &[MessageEnvelope],
) -> Result<(), AtmError> {
    atomic::write_messages(path, messages)
}

/// Commit one already-loaded multi-source mailbox set through the mailbox layer.
pub(crate) fn commit_source_files(source_files: &[SourceFile]) -> Result<(), AtmError> {
    for source in source_files {
        commit_mailbox_state(&source.path, &source.messages)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use uuid::Uuid;

    use super::{commit_mailbox_state, commit_source_files};
    use crate::mailbox::read_messages;
    use crate::mailbox::source::SourceFile;
    use crate::schema::{LegacyMessageId, MessageEnvelope};
    use crate::types::IsoTimestamp;

    #[test]
    fn commit_mailbox_state_rewrites_mailbox_jsonl_with_only_new_messages() {
        let tempdir = tempdir().expect("tempdir");
        let path = tempdir.path().join("arch-ctm.json");
        std::fs::write(&path, "{\"stale\":true}\n").expect("seed mailbox");
        let messages = vec![
            sample_message("team-lead", "first replacement"),
            sample_message("qa", "second replacement"),
        ];

        commit_mailbox_state(&path, &messages).expect("commit mailbox");

        let raw = std::fs::read_to_string(&path).expect("mailbox contents");
        assert!(!raw.contains("stale"));
        assert_eq!(raw.lines().count(), 2);
        assert!(raw.ends_with('\n'));
        assert_eq!(read_messages(&path).expect("read mailbox"), messages);
    }

    #[test]
    fn commit_source_files_commits_each_source_path() {
        let tempdir = tempdir().expect("tempdir");
        let left_path = tempdir.path().join("arch-ctm.json");
        let right_path = tempdir.path().join("qa.json");
        let left_messages = vec![sample_message("team-lead", "left message")];
        let right_messages = vec![
            sample_message("arch-ctm", "right first"),
            sample_message("team-lead", "right second"),
        ];

        commit_source_files(&[
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

        assert_eq!(
            read_messages(&left_path).expect("left inbox"),
            left_messages
        );
        assert_eq!(
            read_messages(&right_path).expect("right inbox"),
            right_messages
        );
    }

    #[test]
    fn commit_source_files_stops_after_first_write_error() {
        let tempdir = tempdir().expect("tempdir");
        let first_path = tempdir.path().join("first.json");
        let invalid_path = tempdir.path().to_path_buf();
        let later_path = tempdir.path().join("later.json");

        let error = commit_source_files(&[
            SourceFile {
                path: first_path.clone(),
                messages: vec![sample_message("team-lead", "first")],
            },
            SourceFile {
                path: invalid_path,
                messages: vec![sample_message("qa", "broken")],
            },
            SourceFile {
                path: later_path.clone(),
                messages: vec![sample_message("arch-ctm", "later")],
            },
        ])
        .expect_err("write failure");

        assert!(error.is_mailbox_write());
        assert_eq!(read_messages(&first_path).expect("first inbox").len(), 1);
        assert!(!later_path.exists());
    }

    fn sample_message(from: &str, text: &str) -> MessageEnvelope {
        MessageEnvelope {
            from: from.to_string(),
            text: text.to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some("atm-dev".to_string()),
            summary: None,
            message_id: Some(LegacyMessageId::from(Uuid::new_v4())),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            task_id: None,
            extra: serde_json::Map::new(),
        }
    }
}
