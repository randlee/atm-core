//! Mailbox owner-layer write boundaries for the Claude-owned inbox surface.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::AtmError;
use crate::mailbox::atomic;
use crate::mailbox::lock;
use crate::mailbox::source::{
    SourceFile, discover_source_paths, load_source_files, rediscover_and_validate_source_paths,
};
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

/// Load the current mailbox source set without taking any mailbox locks.
pub(crate) fn observe_source_files(
    home_dir: &Path,
    team: &str,
    agent: &str,
) -> Result<Vec<SourceFile>, AtmError> {
    let source_paths = discover_source_paths(home_dir, team, agent)?;
    load_source_files(&source_paths)
}

/// Reload one mailbox source set under the deterministic mailbox lock plan
/// without forcing the caller into an inbox rewrite.
pub(crate) fn with_locked_source_files<T, I, F>(
    home_dir: &Path,
    team: &str,
    agent: &str,
    extra_write_paths: I,
    timeout: Duration,
    body: F,
) -> Result<T, AtmError>
where
    I: IntoIterator<Item = PathBuf>,
    F: FnOnce(&[PathBuf], &mut Vec<SourceFile>) -> Result<T, AtmError>,
{
    with_locked_source_files_hook(
        home_dir,
        team,
        agent,
        extra_write_paths,
        timeout,
        |_| Ok(()),
        body,
    )
}

fn with_locked_source_files_hook<T, I, H, F>(
    home_dir: &Path,
    team: &str,
    agent: &str,
    extra_write_paths: I,
    timeout: Duration,
    before_load: H,
    body: F,
) -> Result<T, AtmError>
where
    I: IntoIterator<Item = PathBuf>,
    H: FnOnce(&[PathBuf]) -> Result<(), AtmError>,
    F: FnOnce(&[PathBuf], &mut Vec<SourceFile>) -> Result<T, AtmError>,
{
    let source_paths = discover_source_paths(home_dir, team, agent)?;
    let mut write_paths = source_paths.clone();
    write_paths.extend(extra_write_paths);
    let _locks = lock::acquire_many_sorted(write_paths, timeout)?;
    let source_paths = rediscover_and_validate_source_paths(&source_paths, home_dir, team, agent)?;
    before_load(&source_paths)?;
    let mut source_files = load_source_files(&source_paths)?;
    body(&source_paths, &mut source_files)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{commit_mailbox_state, commit_source_files, with_locked_source_files_hook};
    use crate::mailbox::read_messages;
    use crate::mailbox::source::SourceFile;
    use crate::schema::{AtmMessageId, LegacyMessageId, MessageEnvelope};
    use crate::types::{AgentName, IsoTimestamp, TeamName};

    #[test]
    fn commit_mailbox_state_rewrites_mailbox_array_with_only_new_messages() {
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
        let encoded: Vec<serde_json::Value> = serde_json::from_str(&raw).expect("json array");
        assert_eq!(encoded.len(), 2);
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

    #[test]
    fn injected_before_load_hook_can_fail_closed_without_production_env_seam() {
        let tempdir = tempdir().expect("tempdir");
        let team_dir = tempdir.path().join(".claude").join("teams").join("atm-dev");
        let inbox_dir = team_dir.join("inboxes");
        std::fs::create_dir_all(&inbox_dir).expect("inbox dir");
        std::fs::write(
            team_dir.join("config.json"),
            serde_json::json!({
                "members": [{"name": "arch-ctm"}, {"name": "team-lead"}]
            })
            .to_string(),
        )
        .expect("config");
        let inbox_path = inbox_dir.join("arch-ctm.json");
        commit_mailbox_state(&inbox_path, &[sample_message("team-lead", "hello")]).expect("seed");

        let error = with_locked_source_files_hook(
            tempdir.path(),
            "atm-dev",
            "arch-ctm",
            std::iter::empty::<std::path::PathBuf>(),
            std::time::Duration::from_secs(1),
            |paths| {
                let path = paths.first().expect("first path");
                std::fs::remove_file(path).map_err(|source| {
                    crate::error::AtmError::mailbox_write(format!(
                        "failed to remove locked inbox {} during test injection: {source}",
                        path.display()
                    ))
                    .with_source(source)
                })
            },
            |_paths, _source_files| Ok(()),
        )
        .expect_err("hook failure");

        assert!(error.is_mailbox_read());
        assert!(!inbox_path.exists());
    }

    fn sample_message(from: &str, text: &str) -> MessageEnvelope {
        let atm_message_id = AtmMessageId::new();
        let message_id = LegacyMessageId::from_atm_message_id(atm_message_id);
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
            from: from.parse::<AgentName>().expect("agent name"),
            text: text.to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some("atm-dev".parse::<TeamName>().expect("team name")),
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
