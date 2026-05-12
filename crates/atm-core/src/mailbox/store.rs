//! Mailbox owner-layer write boundaries for the Claude-owned inbox surface.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config;
use crate::error::AtmError;
use crate::mailbox::atomic;
use crate::mailbox::lock;
use crate::mailbox::source::{
    SourceFile, SummarySourceFile, discover_source_paths, load_source_files,
    load_summary_source_files, rediscover_and_validate_source_paths,
};
use crate::schema::MessageEnvelope;
use crate::schema::inbox_message::SharedInboxExportPolicy;
use crate::types::{AgentName, TeamName};

/// Commit one mailbox file through the mailbox-layer write boundary.
///
/// The mailbox layer owns writes to the Claude-owned inbox compatibility
/// surface. Callers should express mailbox intent here instead of reaching
/// down to low-level atomic replacement directly.
pub(crate) fn commit_mailbox_state(
    path: &Path,
    messages: &[MessageEnvelope],
) -> Result<(), AtmError> {
    let export_policy = load_export_policy(path)?;
    commit_mailbox_state_with_policy(path, messages, export_policy)
}

fn commit_mailbox_state_with_policy(
    path: &Path,
    messages: &[MessageEnvelope],
    export_policy: SharedInboxExportPolicy,
) -> Result<(), AtmError> {
    atomic::write_messages(path, messages, export_policy)
}

/// Commit one already-loaded multi-source mailbox set through the mailbox layer.
pub(crate) fn commit_source_files(source_files: &[SourceFile]) -> Result<(), AtmError> {
    let mut export_policy_by_dir = BTreeMap::<PathBuf, SharedInboxExportPolicy>::new();
    for source in source_files {
        let config_dir = source
            .path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let export_policy = if let Some(policy) = export_policy_by_dir.get(&config_dir).copied() {
            policy
        } else {
            let policy = load_export_policy(&source.path)?;
            export_policy_by_dir.insert(config_dir, policy);
            policy
        };
        commit_mailbox_state_with_policy(&source.path, &source.messages, export_policy)?;
    }
    Ok(())
}

fn load_export_policy(path: &Path) -> Result<SharedInboxExportPolicy, AtmError> {
    let config_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let atm_authored_body_export_max_bytes = config::load_config(config_dir)?
        .map(|config| config.claude_jsonl_body_export_max_bytes)
        .unwrap_or_else(|| SharedInboxExportPolicy::default().atm_authored_body_export_max_bytes);
    Ok(SharedInboxExportPolicy {
        atm_authored_body_export_max_bytes,
    })
}

/// Load the current mailbox source set without taking any mailbox locks.
pub(crate) fn observe_source_files(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<SourceFile>, AtmError> {
    let source_paths = discover_source_paths(home_dir, team, agent)?;
    load_source_files(&source_paths)
}

pub(crate) fn observe_summary_source_files(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
    contains_filter: Option<&str>,
) -> Result<Vec<SummarySourceFile>, AtmError> {
    let source_paths = discover_source_paths(home_dir, team, agent)?;
    load_summary_source_files(&source_paths, contains_filter)
}

/// Reload one mailbox source set under the deterministic mailbox lock plan
/// without forcing the caller into an inbox rewrite.
pub(crate) fn with_locked_source_files<T, I, F>(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
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
    team: &TeamName,
    agent: &AgentName,
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
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::schema::{AtmMessageId, MessageEnvelope};
    use crate::test_support::{TEST_QA, TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, IsoTimestamp, TeamName};

    #[test]
    fn commit_mailbox_state_rewrites_mailbox_array_with_only_new_messages() {
        let tempdir = tempdir().expect("tempdir");
        let path = tempdir.path().join(format!("{TEST_SENDER}.json"));
        std::fs::write(&path, "{\"stale\":true}\n").expect("seed mailbox");
        let messages = vec![
            sample_message(ROLE_TEAM_LEAD, "first replacement"),
            sample_message(TEST_QA, "second replacement"),
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
    fn commit_mailbox_state_exports_retrieval_stub_when_config_cap_is_zero() {
        let tempdir = tempdir().expect("tempdir");
        std::fs::write(
            tempdir.path().join(".atm.toml"),
            "[atm]\nclaude_jsonl_body_export_max_bytes = 0\n",
        )
        .expect("config");
        let path = tempdir.path().join(format!("{TEST_SENDER}.json"));
        let mut message = sample_message(ROLE_TEAM_LEAD, "full body retained elsewhere");
        let message_id = message.message_id.expect("message id");
        message.summary = Some("stub summary".to_string());

        commit_mailbox_state(&path, std::slice::from_ref(&message)).expect("commit mailbox");

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
    fn commit_source_files_commits_each_source_path() {
        let tempdir = tempdir().expect("tempdir");
        let left_path = tempdir.path().join(format!("{TEST_SENDER}.json"));
        let right_path = tempdir.path().join(format!("{TEST_QA}.json"));
        let left_messages = vec![sample_message(ROLE_TEAM_LEAD, "left message")];
        let right_messages = vec![
            sample_message(TEST_SENDER, "right first"),
            sample_message(ROLE_TEAM_LEAD, "right second"),
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
                messages: vec![sample_message(ROLE_TEAM_LEAD, "first")],
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
        assert_eq!(read_messages(&first_path).expect("first inbox").len(), 1);
        assert!(!later_path.exists());
    }

    #[test]
    fn injected_before_load_hook_can_fail_closed_without_production_env_seam() {
        let tempdir = tempdir().expect("tempdir");
        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        let inbox_dir = team_dir.join("inboxes");
        std::fs::create_dir_all(&inbox_dir).expect("inbox dir");
        std::fs::write(
            team_dir.join("config.json"),
            serde_json::json!({
                "members": [{"name": TEST_SENDER}, {"name": ROLE_TEAM_LEAD}]
            })
            .to_string(),
        )
        .expect("config");
        let inbox_path = inbox_dir.join(format!("{TEST_SENDER}.json"));
        commit_mailbox_state(&inbox_path, &[sample_message(ROLE_TEAM_LEAD, "hello")])
            .expect("seed");

        let error = with_locked_source_files_hook(
            tempdir.path(),
            &TEST_TEAM.parse().expect("team"),
            &TEST_SENDER.parse().expect("sender"),
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
        let message_id = AtmMessageId::new();

        MessageEnvelope {
            from: from.parse::<AgentName>().expect("agent name"),
            text: text.to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(TEST_TEAM.parse::<TeamName>().expect("team name")),
            summary: None,
            message_id: Some(message_id),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            stale_at: None,
            task_id: None,
            extra: serde_json::Map::new(),
        }
    }
}
