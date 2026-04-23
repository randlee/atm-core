use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::config;
use crate::error::AtmError;
use crate::home;
use crate::identity;
use crate::mailbox;
use crate::mailbox::source::{
    SourceFile, SourcedMessage, discover_source_paths, load_source_files,
    rediscover_and_validate_source_paths, resolve_target,
};
use crate::mailbox::surface::dedupe_legacy_message_id_surface;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::read::state;
use crate::schema::MessageEnvelope;
use crate::types::{AgentName, MessageClass, SourceIndex, TeamName};

/// Parameters for clearing read or acknowledged mailbox messages.
#[derive(Debug, Clone)]
pub struct ClearQuery {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub actor_override: Option<AgentName>,
    pub target_address: Option<String>,
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
    let actor = identity::resolve_actor_identity(query.actor_override.as_deref(), config.as_ref())?;
    let target = resolve_target(
        query.target_address.as_deref(),
        &actor,
        query.team_override.as_deref(),
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
            .any(|member| member.name == target.agent)
    {
        return Err(
            AtmError::agent_not_found(&target.agent, &target.team).with_recovery(
                "Update the team membership in config.json or clear a different mailbox target.",
            ),
        );
    }

    let cutoff = cutoff_timestamp(query.older_than)?;

    let (removed_total, remaining_total, removed_by_class) = if query.dry_run {
        let source_paths = discover_source_paths(&query.home_dir, &target.team, &target.agent)?;
        let source_files = load_source_files(&source_paths)?;
        // Clear intentionally does not apply read-surface idle-notification dedup.
        // Cleanup decisions must inspect the raw merged surface after legacy
        // message_id canonicalization only.
        let merged = dedupe_legacy_message_id_surface(
            merged_surface(&source_files),
            |message: &SourcedMessage| message.envelope.message_id,
            |message: &SourcedMessage| message.envelope.timestamp,
        );
        let mut removed_by_class = RemovedByClass::default();
        let removable = merged
            .iter()
            .filter(|message| is_clearable(message, cutoff, query.idle_only))
            .inspect(|message| {
                count_removed(
                    &mut removed_by_class,
                    state::classify_message(&message.envelope),
                )
            })
            .map(|message| (message.source_path.clone(), message.source_index))
            .collect::<HashSet<_>>();
        (
            removable.len(),
            merged.len().saturating_sub(removable.len()),
            removed_by_class,
        )
    } else {
        let source_paths = discover_source_paths(&query.home_dir, &target.team, &target.agent)?;
        let _locks = mailbox::lock::acquire_many_sorted(
            source_paths.clone(),
            mailbox::lock::default_lock_timeout(),
        )?;
        let source_paths = rediscover_and_validate_source_paths(
            &source_paths,
            &query.home_dir,
            &target.team,
            &target.agent,
        )?;
        #[cfg(test)]
        maybe_remove_locked_source_file_for_test(&source_paths)?;
        let mut source_files = load_source_files(&source_paths)?;
        let merged = dedupe_legacy_message_id_surface(
            merged_surface(&source_files),
            |message: &SourcedMessage| message.envelope.message_id,
            |message: &SourcedMessage| message.envelope.timestamp,
        );
        let mut locked_removed_by_class = RemovedByClass::default();
        let removable = merged
            .iter()
            .filter(|message| is_clearable(message, cutoff, query.idle_only))
            .inspect(|message| {
                count_removed(
                    &mut locked_removed_by_class,
                    state::classify_message(&message.envelope),
                )
            })
            .map(|message| (message.source_path.clone(), message.source_index))
            .collect::<HashSet<_>>();

        apply_removals(&mut source_files, &removable);
        mailbox::store::commit_source_files(&source_files)?;
        let remaining_total = dedupe_legacy_message_id_surface(
            merged_surface(&source_files),
            |message: &SourcedMessage| message.envelope.message_id,
            |message: &SourcedMessage| message.envelope.timestamp,
        )
        .len();
        (removable.len(), remaining_total, locked_removed_by_class)
    };

    let outcome = ClearOutcome {
        action: "clear",
        team: target.team.clone().into(),
        agent: target.agent.clone().into(),
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
        sender: actor,
        message_id: None,
        requires_ack: false,
        dry_run: query.dry_run,
        task_id: None,
        error_code: None,
        error_message: None,
    });

    Ok(outcome)
}

fn merged_surface(source_files: &[SourceFile]) -> Vec<SourcedMessage> {
    source_files
        .iter()
        .flat_map(|source| {
            source
                .messages
                .iter()
                .cloned()
                .enumerate()
                .map(|(source_index, envelope)| SourcedMessage {
                    envelope,
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

fn is_idle_notification(message: &MessageEnvelope) -> bool {
    // Claude Code currently defines idle notifications as JSON encoded in the
    // native `text` field. Do not replace this with an ATM-local schema here;
    // any ownership change must be documented in docs/claude-code-message-schema.md.
    serde_json::from_str::<Value>(&message.text)
        .ok()
        .map(|value| value.get("type").and_then(Value::as_str) == Some("idle_notification"))
        .unwrap_or(false)
}

#[cfg(test)]
fn maybe_remove_locked_source_file_for_test(source_paths: &[PathBuf]) -> Result<(), AtmError> {
    if std::env::var_os("ATM_TEST_REMOVE_LOCKED_INBOX_BEFORE_LOAD").is_none() {
        return Ok(());
    }

    let Some(path) = source_paths.first() else {
        return Ok(());
    };
    std::fs::remove_file(path).map_err(|error| {
        AtmError::mailbox_write(format!(
            "failed to remove locked inbox {} during test injection: {error}",
            path.display()
        ))
        .with_recovery(
            "Clear ATM_TEST_REMOVE_LOCKED_INBOX_BEFORE_LOAD or restore the missing inbox file before retrying the injected test path.",
        )
        .with_source(error)
    })
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
    use serial_test::serial;
    use tempfile::tempdir;

    use super::maybe_remove_locked_source_file_for_test;
    use crate::mailbox::source::load_source_files;

    #[test]
    #[serial]
    fn locked_clear_source_removal_reports_disappearing_mailbox() {
        let tempdir = tempdir().expect("tempdir");
        let path = tempdir.path().join("arch-ctm.json");
        std::fs::write(&path, "").expect("mailbox");
        // Test-only env mutation is scoped to this process and reset below.
        unsafe {
            std::env::set_var("ATM_TEST_REMOVE_LOCKED_INBOX_BEFORE_LOAD", "1");
        }

        maybe_remove_locked_source_file_for_test(std::slice::from_ref(&path)).expect("remove");
        let error = load_source_files(&[path]).expect_err("missing mailbox");

        unsafe {
            std::env::remove_var("ATM_TEST_REMOVE_LOCKED_INBOX_BEFORE_LOAD");
        }
        assert!(error.is_mailbox_read());
        assert!(error.message.contains("disappeared"));
    }
}
