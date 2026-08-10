use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::debug;

use crate::boundary;
use crate::error::AtmError;
use crate::mailbox::source::ResolvedTarget;
use crate::mailbox::source::resolve_target;
use crate::observability::{CommandEvent, ObservabilityPort, action_name, outcome_label};
use crate::read::state;
use crate::schema::InboxMessage;
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::{RetainedMailboxRuntime, default_runtime};
use crate::types::{AgentName, CommandAction, IsoTimestamp, MessageClass, TeamName};

/// Parameters for clearing read or acknowledged mailbox messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClearQuery {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    pub older_than: Option<Duration>,
    pub idle_only: bool,
    pub dry_run: bool,
}

impl ClearQuery {
    /// Replaces caller-supplied filesystem roots with the daemon-owned root
    /// before a request crosses the long-lived service boundary.
    #[must_use]
    pub fn with_daemon_paths(mut self, daemon_home: PathBuf) -> Self {
        self.home_dir = daemon_home.clone();
        self.current_dir = daemon_home;
        self
    }
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

struct ClearRuntimeContext {
    actor: AgentName,
    target: ResolvedTarget,
    metadata_rows: Vec<boundary::MailStoreMailboxMetadataRow>,
    removable: Vec<(boundary::MessageKey, InboxMessage, MessageClass)>,
}

fn clear_mail_with_runtime_impl<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    query: ClearQuery,
    observability: &dyn ObservabilityPort,
    runtime: &R,
) -> Result<ClearOutcome, AtmError> {
    let context = load_clear_runtime_context(runtime, &query)?;
    if !query.dry_run {
        persist_deleted_messages(runtime, &context.target, &context.removable)?;
    }
    let mut removed_by_class = RemovedByClass::default();
    for (_, _, class) in &context.removable {
        count_removed(&mut removed_by_class, *class);
    }
    let removed_total = context.removable.len();
    let remaining_total = context.metadata_rows.len().saturating_sub(removed_total);

    let outcome = ClearOutcome {
        action: CommandAction::Clear,
        team: context.target.team.clone(),
        agent: context.target.agent.clone(),
        removed_total,
        remaining_total,
        removed_by_class,
    };

    if let Err(error) = observability.emit(CommandEvent {
        command: "clear",
        action: action_name("clear"),
        outcome: outcome_label(if query.dry_run { "dry_run" } else { "ok" }),
        team: outcome.team.clone(),
        agent: outcome.agent.clone(),
        sender: context.actor,
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

fn load_clear_runtime_context<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    query: &ClearQuery,
) -> Result<ClearRuntimeContext, AtmError> {
    let config = runtime.load_config(&query.current_dir)?;
    let actor = query.caller_identity.clone();
    let target = resolve_target(None, &actor, &query.caller_team, config.as_ref())?;

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
    Ok(ClearRuntimeContext {
        actor,
        target,
        metadata_rows,
        removable,
    })
}

fn persist_deleted_messages<R: RetainedMailboxRuntime>(
    runtime: &R,
    target: &ResolvedTarget,
    removable: &[(boundary::MessageKey, InboxMessage, MessageClass)],
) -> Result<(), AtmError> {
    let deleted_at = IsoTimestamp::now();
    for (message_key, envelope, _) in removable {
        runtime.persist_message_state(boundary::MailMessageState {
            team: target.team.clone(),
            agent: target.agent.clone(),
            actor: target.agent.clone(),
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
) -> Result<Vec<(boundary::MessageKey, InboxMessage, MessageClass)>, AtmError> {
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
            )));
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
            TimeDelta::from_std(duration)
                .map_err(|error| AtmError::validation(format!("invalid duration filter: {error}")))
        })
        .transpose()
        .map(|delta| delta.map(|delta| Utc::now() - delta))
}

fn is_idle_notification(message: &InboxMessage) -> bool {
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
    use std::{
        ffi::OsString,
        panic,
        panic::AssertUnwindSafe,
        path::{Path, PathBuf},
    };

    use crate::test_support::{EnvGuard, lock_env, remove_env_var, set_env_var};
    use serde_json::Map;
    use tempfile::tempdir;

    use super::{ClearQuery, clear_mail_with_runtime_impl};
    use crate::boundary::{self, RosterHarness, RosterMemberKind};
    use crate::error::AtmError;
    use crate::observability::NullObservability;
    use crate::schema::InboxMessage;
    use crate::service_runtime::RetainedServiceRuntime;
    use crate::service_runtime_store::RetainedMailboxRuntime;
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, TeamName};
    #[test]
    #[serial_test::serial(env)]
    fn env_guard_restores_original_value_after_panic() {
        {
            let _env_lock = lock_env();
            set_env_var("ATM_TEST_REMOVE_LOCKED_INBOX_BEFORE_LOAD", "original");
        }

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
        {
            let _env_lock = lock_env();
            remove_env_var("ATM_TEST_REMOVE_LOCKED_INBOX_BEFORE_LOAD");
        }
    }

    struct ClearRuntime {
        roster_present: bool,
    }

    impl crate::boundary::sealed::Sealed for ClearRuntime {}

    impl RetainedServiceRuntime for ClearRuntime {
        fn load_config(
            &self,
            _current_dir: &Path,
        ) -> Result<Option<crate::config::AtmConfig>, AtmError> {
            Ok(None)
        }

        fn inbox_path(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<PathBuf, AtmError> {
            unreachable!("clear roster-truth tests do not resolve inbox paths")
        }

        fn load_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<Option<crate::types::IsoTimestamp>, AtmError> {
            Ok(None)
        }

        fn save_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _timestamp: crate::types::IsoTimestamp,
        ) -> Result<(), AtmError> {
            Ok(())
        }

        fn deliver_non_claude_payloads(
            &self,
            _recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
            _messages: &[InboxMessage],
        ) -> Result<(), AtmError> {
            unreachable!("clear roster-truth tests do not route outbound payloads")
        }

        fn load_roster_member(
            &self,
            team: &TeamName,
            agent: &AgentName,
        ) -> Result<Option<boundary::RosterEntry>, AtmError> {
            Ok(self.roster_present.then(|| boundary::RosterEntry {
                team_name: team.clone(),
                agent_name: agent.clone(),
                member_kind: RosterMemberKind::Permanent,
                harness: RosterHarness::ClaudeCode,
                agent_type: crate::schema::AgentType::default(),
                model: crate::types::ModelName::default(),
                recipient_pane_id: None,
                metadata_json: Map::new(),
            }))
        }

        fn load_team_roster(
            &self,
            _team: &TeamName,
        ) -> Result<Vec<boundary::RosterEntry>, AtmError> {
            Ok(Vec::new())
        }
    }

    impl RetainedMailboxRuntime for ClearRuntime {
        fn acknowledge_message_atomically(
            &self,
            _source: &atm_storage::contract::AcknowledgementSource,
            _builder: std::sync::Arc<dyn atm_storage::contract::AcknowledgementReplyBuilder>,
        ) -> Result<atm_storage::contract::AcknowledgementCommit, AtmError> {
            unreachable!("clear roster-truth tests do not admit acknowledgements")
        }

        fn query_mailbox_metadata_rows(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _limit: Option<usize>,
        ) -> Result<Vec<boundary::MailStoreMailboxMetadataRow>, AtmError> {
            Ok(Vec::new())
        }

        fn load_message_record(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _message_key: &boundary::MessageKey,
        ) -> Result<Option<boundary::Message>, AtmError> {
            unreachable!("clear roster-truth tests do not load message records")
        }

        fn persist_message_record(&self, _record: boundary::Message) -> Result<(), AtmError> {
            unreachable!("clear roster-truth tests do not persist message records")
        }

        fn persist_message_state(
            &self,
            _state: boundary::MailMessageState,
        ) -> Result<(), AtmError> {
            unreachable!("clear roster-truth tests do not persist message state")
        }
    }

    fn clear_query(home_dir: PathBuf, current_dir: PathBuf) -> ClearQuery {
        ClearQuery {
            home_dir,
            current_dir,
            caller_identity: AgentName::from_validated(TEST_SENDER),
            caller_team: TeamName::from_validated(TEST_TEAM),
            older_than: None,
            idle_only: false,
            dry_run: true,
        }
    }

    #[test]
    fn clear_mail_targets_only_the_owner_mailbox() {
        let tempdir = tempdir().expect("tempdir");
        let runtime = ClearRuntime {
            roster_present: true,
        };

        let outcome = clear_mail_with_runtime_impl(
            clear_query(tempdir.path().to_path_buf(), tempdir.path().to_path_buf()),
            &NullObservability,
            &runtime,
        )
        .expect("clear outcome");

        assert_eq!(outcome.team, TeamName::from_validated(TEST_TEAM));
        assert_eq!(outcome.agent, AgentName::from_validated(TEST_SENDER));
        assert_eq!(outcome.removed_total, 0);
    }
}
