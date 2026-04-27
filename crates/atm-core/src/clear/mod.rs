use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::address::AgentAddress;
use crate::config;
use crate::error::AtmError;
use crate::home;
use crate::identity;
use crate::mailbox;
use crate::mailbox::source::{SourceFile, SourcedMessage, resolve_target};
use crate::mailbox::surface::dedupe_legacy_message_id_surface;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::read::state;
use crate::schema::MessageEnvelope;
use crate::types::{AgentName, MessageClass, SourceIndex, TeamName};
use crate::workflow;

/// Parameters for clearing read or acknowledged mailbox messages.
#[derive(Debug, Clone)]
pub struct ClearQuery {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub actor_override: Option<AgentName>,
    pub target_address: Option<AgentAddress>,
    pub team_override: Option<TeamName>,
    pub older_than: Option<Duration>,
    pub idle_only: bool,
    pub dry_run: bool,
}

/// Counts of removed mailbox messages by ATM display class.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RemovedByClass {
    pub acknowledged: usize,
    pub read: usize,
}

/// Result of one mailbox cleanup command.
#[derive(Debug, Clone, Serialize)]
pub struct ClearOutcome {
    pub action: &'static str,
    pub team: TeamName,
    pub agent: AgentName,
    pub removed_total: usize,
    pub remaining_total: usize,
    pub removed_by_class: RemovedByClass,
}

/// Remove read or acknowledged messages from one mailbox surface.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::IdentityUnavailable`],
/// [`crate::error_codes::AtmErrorCode::TeamUnavailable`],
/// [`crate::error_codes::AtmErrorCode::TeamNotFound`],
/// [`crate::error_codes::AtmErrorCode::AgentNotFound`],
/// [`crate::error_codes::AtmErrorCode::AddressParseFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxReadFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxWriteFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxLockFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxLockTimeout`], or
/// [`crate::error_codes::AtmErrorCode::MessageValidationFailed`] when actor or
/// target resolution fails, the team or agent cannot be validated, shared
/// mailbox locks cannot be acquired, or the selected source files cannot be
/// persisted safely.
pub fn clear_mail(
    query: ClearQuery,
    observability: &dyn ObservabilityPort,
) -> Result<ClearOutcome, AtmError> {
    let config = config::load_config(&query.current_dir)?;
    let actor = AgentName::from(identity::resolve_actor_identity(
        query.actor_override.as_deref(),
        config.as_ref(),
    )?);
    let target = resolve_target(
        query.target_address.as_ref(),
        &actor,
        query.team_override.as_ref(),
        config.as_ref(),
    )?;

    let team_dir = home::team_dir_from_home(&query.home_dir, &target.team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&target.team).with_recovery(
            "Create the team config for the requested team or target a different team before retrying `atm clear`.",
        ));
    }

    let team_config = config::load_team_config(&team_dir)?;
    if target.explicit
        && !team_config
            .members
            .iter()
            .any(|member| member.name == target.agent.as_str())
    {
        return Err(
            AtmError::agent_not_found(&target.agent, &target.team).with_recovery(
                "Update the team membership in config.json or clear a different mailbox target.",
            ),
        );
    }

    let cutoff = cutoff_timestamp(query.older_than)?;
    let workflow_path =
        home::workflow_state_path_from_home(&query.home_dir, &target.team, &target.agent)?;

    let (removed_total, remaining_total, removed_by_class) = if query.dry_run {
        let workflow_state =
            workflow::load_workflow_state(&query.home_dir, &target.team, &target.agent)?;
        let source_files =
            mailbox::store::observe_source_files(&query.home_dir, &target.team, &target.agent)?;
        // Clear intentionally does not apply read-surface idle-notification dedup.
        // Cleanup decisions must inspect the raw merged surface after legacy
        // message_id canonicalization only.
        let (removable, removed_by_class, merged_len) =
            removable_messages(&source_files, &workflow_state, cutoff, query.idle_only);
        (
            removable.len(),
            merged_len.saturating_sub(removable.len()),
            removed_by_class,
        )
    } else {
        mailbox::store::with_locked_source_files(
            &query.home_dir,
            &target.team,
            &target.agent,
            [workflow_path],
            mailbox::lock::default_lock_timeout(),
            |_source_paths, source_files| {
                let mut workflow_state =
                    workflow::load_workflow_state(&query.home_dir, &target.team, &target.agent)?;
                let (removable, removed_by_class, _) =
                    removable_messages(source_files, &workflow_state, cutoff, query.idle_only);
                let workflow_changed =
                    remove_workflow_state_entries(&mut workflow_state, source_files, &removable);
                apply_removals(source_files, &removable);
                if !removable.is_empty() {
                    mailbox::store::commit_source_files(source_files)?;
                }
                if workflow_changed {
                    workflow::save_workflow_state(
                        &query.home_dir,
                        &target.team,
                        &target.agent,
                        &workflow_state,
                    )?;
                }
                let remaining_total = dedupe_legacy_message_id_surface(
                    merged_surface(source_files, &workflow_state),
                    |message: &SourcedMessage| message.envelope.message_id,
                    |message: &SourcedMessage| message.envelope.timestamp,
                )
                .len();
                Ok((removable.len(), remaining_total, removed_by_class))
            },
        )?
    };

    let outcome = ClearOutcome {
        action: "clear",
        team: target.team.clone(),
        agent: target.agent.clone(),
        removed_total,
        remaining_total,
        removed_by_class,
    };

    let _ = observability.emit(CommandEvent {
        command: "clear",
        action: "clear",
        outcome: if query.dry_run { "dry_run" } else { "ok" },
        team: outcome.team.to_string(),
        agent: outcome.agent.to_string(),
        sender: actor.to_string(),
        message_id: None,
        requires_ack: false,
        dry_run: query.dry_run,
        task_id: None,
        error_code: None,
        error_message: None,
    });

    Ok(outcome)
}

fn merged_surface(
    source_files: &[SourceFile],
    workflow_state: &workflow::WorkflowStateFile,
) -> Vec<SourcedMessage> {
    source_files
        .iter()
        .flat_map(|source| {
            source
                .messages
                .iter()
                .cloned()
                .enumerate()
                .map(|(source_index, envelope)| SourcedMessage {
                    envelope: workflow::project_envelope(&envelope, workflow_state),
                    source_path: source.path.clone(),
                    source_index: source_index.into(),
                })
        })
        .collect()
}

fn cutoff_timestamp(
    older_than: Option<Duration>,
) -> Result<Option<chrono::DateTime<Utc>>, AtmError> {
    older_than
        .map(|duration| {
            TimeDelta::from_std(duration).map_err(|error| {
                AtmError::validation(format!("invalid duration filter: {error}")).with_recovery(
                    "Use --older-than with a positive duration like 30s, 10m, 2h, or 7d.",
                )
            })
        })
        .transpose()
        .map(|delta| delta.map(|delta| Utc::now() - delta))
}

fn is_clearable(message: &SourcedMessage, cutoff: Option<DateTime<Utc>>, idle_only: bool) -> bool {
    let class = state::classify_message(&message.envelope);
    matches!(class, MessageClass::Read | MessageClass::Acknowledged)
        && cutoff
            .map(|cutoff| message.envelope.timestamp.into_inner() <= cutoff)
            .unwrap_or(true)
        && (!idle_only || is_idle_notification(&message.envelope))
}

fn removable_messages(
    source_files: &[SourceFile],
    workflow_state: &workflow::WorkflowStateFile,
    cutoff: Option<DateTime<Utc>>,
    idle_only: bool,
) -> (HashSet<(PathBuf, SourceIndex)>, RemovedByClass, usize) {
    let merged = dedupe_legacy_message_id_surface(
        merged_surface(source_files, workflow_state),
        |message: &SourcedMessage| message.envelope.message_id,
        |message: &SourcedMessage| message.envelope.timestamp,
    );
    let mut removed_by_class = RemovedByClass::default();
    let removable = merged
        .iter()
        .filter(|message| is_clearable(message, cutoff, idle_only))
        .inspect(|message| {
            count_removed(
                &mut removed_by_class,
                state::classify_message(&message.envelope),
            )
        })
        .map(|message| (message.source_path.clone(), message.source_index))
        .collect::<HashSet<_>>();

    (removable, removed_by_class, merged.len())
}

fn remove_workflow_state_entries(
    workflow_state: &mut workflow::WorkflowStateFile,
    source_files: &[SourceFile],
    removable: &HashSet<(PathBuf, SourceIndex)>,
) -> bool {
    let mut changed = false;
    for source in source_files {
        for (index, message) in source.messages.iter().enumerate() {
            if removable.contains(&(source.path.clone(), index.into())) {
                changed |= workflow::remove_message_state(workflow_state, message);
            }
        }
    }
    changed
}

fn is_idle_notification(message: &MessageEnvelope) -> bool {
    // Claude Code currently defines idle notifications as JSON encoded in the
    // native `text` field. Do not replace this with an ATM-local schema here;
    // any ownership change must be documented in docs/claude-code-message-schema.md.
    serde_json::from_str::<Value>(&message.text)
        .ok()
        .map(|value| value.get("type").and_then(Value::as_str) == Some("idle_notification"))
        .unwrap_or(false)
}

fn count_removed(counts: &mut RemovedByClass, class: MessageClass) {
    match class {
        MessageClass::Unread => unreachable!("unread messages are never clearable"),
        MessageClass::PendingAck => unreachable!("pending-ack messages are never clearable"),
        MessageClass::Acknowledged => counts.acknowledged += 1,
        MessageClass::Read => counts.read += 1,
    }
}

fn apply_removals(source_files: &mut [SourceFile], removable: &HashSet<(PathBuf, SourceIndex)>) {
    for source in source_files {
        source.messages = source
            .messages
            .iter()
            .cloned()
            .enumerate()
            .filter_map(|(index, message)| {
                (!removable.contains(&(source.path.clone(), index.into()))).then_some(message)
            })
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::{OsStr, OsString};
    use std::sync::{Mutex, OnceLock};

    use serial_test::serial;
    use tempfile::tempdir;

    use super::{ClearQuery, clear_mail};
    use crate::observability::NullObservability;
    use crate::schema::{AgentMember, TeamConfig};
    use crate::types::TeamName;

    #[test]
    #[serial]
    fn locked_clear_source_removal_reports_disappearing_mailbox() {
        let _env_lock = env_lock().lock().expect("env lock");
        let tempdir = tempdir().expect("tempdir");
        let team_dir = tempdir.path().join(".claude").join("teams").join("atm-dev");
        let inboxes_dir = team_dir.join("inboxes");
        std::fs::create_dir_all(&inboxes_dir).expect("inboxes");
        let config = TeamConfig {
            members: vec![AgentMember {
                name: "arch-ctm".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        std::fs::write(
            team_dir.join("config.json"),
            serde_json::to_vec(&config).expect("team config"),
        )
        .expect("write config");
        std::fs::write(inboxes_dir.join("arch-ctm.json"), "").expect("mailbox");
        let error = {
            let _guard = EnvGuard::set_raw("ATM_TEST_REMOVE_LOCKED_INBOX_BEFORE_LOAD", "1");
            clear_mail(
                ClearQuery {
                    home_dir: tempdir.path().to_path_buf(),
                    current_dir: tempdir.path().to_path_buf(),
                    actor_override: Some("arch-ctm".into()),
                    target_address: None,
                    team_override: Some(TeamName::from("atm-dev")),
                    older_than: None,
                    idle_only: false,
                    dry_run: false,
                },
                &NullObservability,
            )
            .expect_err("missing mailbox")
        };

        assert!(error.is_mailbox_read());
        assert!(error.message.contains("disappeared"));
        assert!(
            std::env::var_os("ATM_TEST_REMOVE_LOCKED_INBOX_BEFORE_LOAD").is_none(),
            "scoped env guard leaked after failure path"
        );
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvGuard {
        key: &'static str,
        original: Option<OsString>,
    }

    impl EnvGuard {
        fn set_raw(key: &'static str, value: &str) -> Self {
            let original = std::env::var_os(key);
            set_env_var(key, value);
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.original.take() {
                Some(value) => set_env_var(self.key, value),
                None => remove_env_var(self.key),
            }
        }
    }

    fn set_env_var<K: AsRef<OsStr>, V: AsRef<OsStr>>(key: K, value: V) {
        // SAFETY: this test module uses #[serial] before mutating the process
        // environment, so these mutations are serialized within this process.
        unsafe { std::env::set_var(key, value) }
    }

    fn remove_env_var<K: AsRef<OsStr>>(key: K) {
        // SAFETY: this test module uses #[serial] before mutating the process
        // environment, so these mutations are serialized within this process.
        unsafe { std::env::remove_var(key) }
    }
}
