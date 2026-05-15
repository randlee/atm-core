use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::debug;

use crate::address::AgentAddress;
use crate::boundary;
use crate::error::AtmError;
use crate::identity;
use crate::mailbox::source::resolve_target;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::read::state;
use crate::schema::MessageEnvelope;
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::{RetainedMailboxRuntime, default_runtime};
use crate::types::{AgentName, CommandAction, IsoTimestamp, MessageClass, TeamName};

/// Parameters for clearing read or acknowledged mailbox messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RemovedByClass {
    pub acknowledged: usize,
    pub read: usize,
}

/// Result of one mailbox cleanup command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClearOutcome {
    pub action: CommandAction,
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
    let runtime = default_runtime()?;
    clear_mail_with_runtime(query, observability, &runtime)
}

pub fn clear_mail_with_runtime(
    query: ClearQuery,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
) -> Result<ClearOutcome, AtmError> {
    clear_mail_with_runtime_impl(query, observability, runtime)
}

fn clear_mail_with_runtime_impl<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    query: ClearQuery,
    observability: &dyn ObservabilityPort,
    runtime: &R,
) -> Result<ClearOutcome, AtmError> {
    let (actor, target) = resolve_clear_target(&query, runtime)?;

    let cutoff = cutoff_timestamp(query.older_than)?;
    let metadata_rows =
        runtime.query_mailbox_metadata_rows(&query.home_dir, &target.team, &target.agent, None)?;
    let removable = sqlite_removable_messages(
        runtime,
        &query.home_dir,
        &target.team,
        &target.agent,
        &metadata_rows,
        cutoff,
        query.idle_only,
    )?;
    if !query.dry_run {
        persist_deleted_messages(runtime, &target.team, &target.agent, &removable)?;
    }
    let mut removed_by_class = RemovedByClass::default();
    for (_, _, class) in &removable {
        count_removed(&mut removed_by_class, *class);
    }
    let removed_total = removable.len();
    let remaining_total = metadata_rows.len().saturating_sub(removed_total);

    let outcome = ClearOutcome {
        action: CommandAction::Clear,
        team: target.team.clone(),
        agent: target.agent.clone(),
        removed_total,
        remaining_total,
        removed_by_class,
    };

    if let Err(error) = observability.emit(CommandEvent {
        command: "clear",
        action: "clear",
        outcome: if query.dry_run { "dry_run" } else { "ok" },
        team: outcome.team.clone(),
        agent: outcome.agent.clone(),
        sender: actor,
        message_id: None,
        requires_ack: false,
        dry_run: query.dry_run,
        task_id: None,
        error_code: None,
        error_message: None,
    }) {
        tracing::warn!(%error, command = "clear", action = "clear", "failed to emit clear command event");
    }

    Ok(outcome)
}

fn resolve_clear_target<R: RetainedServiceRuntime>(
    query: &ClearQuery,
    runtime: &R,
) -> Result<(AgentName, crate::mailbox::source::ResolvedTarget), AtmError> {
    let config = runtime.load_config(&query.current_dir)?;
    let actor = identity::resolve_actor_identity(query.actor_override.as_deref(), config.as_ref())?;
    let target = resolve_target(
        query.target_address.as_ref(),
        &actor,
        query.team_override.as_ref(),
        config.as_ref(),
    )?;

    let team_dir = runtime.team_dir(&query.home_dir, &target.team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&target.team).with_recovery(
            "Create the team config for the requested team or target a different team before retrying `atm clear`.",
        ));
    }

    let team_config = runtime.load_team_config(&team_dir)?;
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

    Ok((actor, target))
}

fn persist_deleted_messages(
    runtime: &(impl RetainedMailboxRuntime + ?Sized),
    team: &TeamName,
    agent: &AgentName,
    removable: &[(boundary::MessageKey, MessageEnvelope, MessageClass)],
) -> Result<(), AtmError> {
    let deleted_at = IsoTimestamp::now();
    for (message_key, envelope, _) in removable {
        runtime.persist_message_state(boundary::MailMessageState {
            team: team.clone(),
            agent: agent.clone(),
            actor: agent.clone(),
            message_key: message_key.clone(),
            read: envelope.read,
            pending_ack_at: envelope.pending_ack_at,
            acknowledged_at: envelope.acknowledged_at,
            expires_at: envelope.expires_at,
            deleted_at: Some(deleted_at),
            updated_at: Some(deleted_at),
        })?;
    }
    Ok(())
}

fn sqlite_removable_messages<R: RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &std::path::Path,
    team: &TeamName,
    agent: &AgentName,
    metadata_rows: &[boundary::MailStoreMailboxMetadataRow],
    cutoff: Option<DateTime<Utc>>,
    idle_only: bool,
) -> Result<Vec<(boundary::MessageKey, MessageEnvelope, MessageClass)>, AtmError> {
    let mut removable = Vec::new();

    for row in metadata_rows {
        if cutoff
            .map(|cutoff| row.message_at.into_inner() > cutoff)
            .unwrap_or(false)
        {
            continue;
        }

        let Some(record) = runtime.load_message_record(home_dir, team, agent, &row.message_key)?
        else {
            return Err(AtmError::validation(format!(
                "sqlite mailbox metadata row {} could not be reloaded for clear",
                row.message_key
            ))
            .with_recovery(
                "Repair or remove the malformed sqlite mailbox row before retrying `atm clear`.",
            ));
        };

        let class = state::classify_message(&record.envelope);
        if !matches!(class, MessageClass::Read | MessageClass::Acknowledged) {
            continue;
        }
        if idle_only && !is_idle_notification(&record.envelope) {
            continue;
        }
        removable.push((row.message_key.clone(), record.envelope, class));
    }

    Ok(removable)
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

fn is_idle_notification(message: &MessageEnvelope) -> bool {
    // Claude Code currently defines idle notifications as JSON encoded in the
    // native `text` field. Do not replace this with an ATM-local schema here;
    // any ownership change must be documented in docs/claude-code-message-schema.md.
    match serde_json::from_str::<Value>(&message.text) {
        Ok(value) => value.get("type").and_then(Value::as_str) == Some("idle_notification"),
        Err(error) => {
            if message.text.contains("idle_notification") {
                debug!(
                    %error,
                    recovery = "Repair or remove the malformed Claude idle-notification JSON. ATM clear will continue treating the record as a normal mailbox message.",
                    message_text = %message.text,
                    "ignoring malformed idle-notification JSON while classifying clear surface"
                );
            }
            false
        }
    }
}

fn count_removed(counts: &mut RemovedByClass, class: MessageClass) {
    match class {
        MessageClass::Unread => unreachable!("unread messages are never clearable"),
        MessageClass::PendingAck => unreachable!("pending-ack messages are never clearable"),
        MessageClass::Acknowledged => counts.acknowledged += 1,
        MessageClass::Read => counts.read += 1,
    }
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, panic, panic::AssertUnwindSafe};

    use crate::test_support::{EnvGuard, remove_env_var, set_env_var};
    use serial_test::serial;
    #[test]
    #[serial]
    fn env_guard_restores_original_value_after_panic() {
        set_env_var("ATM_TEST_REMOVE_LOCKED_INBOX_BEFORE_LOAD", "original");

        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            let _guard = EnvGuard::set_raw("ATM_TEST_REMOVE_LOCKED_INBOX_BEFORE_LOAD", "1");
            panic!("boom");
        }));

        assert!(
            result.is_err(),
            "panic should propagate through catch_unwind"
        );
        assert_eq!(
            std::env::var_os("ATM_TEST_REMOVE_LOCKED_INBOX_BEFORE_LOAD"),
            Some(OsString::from("original"))
        );
        remove_env_var("ATM_TEST_REMOVE_LOCKED_INBOX_BEFORE_LOAD");
    }
}
