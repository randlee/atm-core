use std::path::PathBuf;

use crate::boundary;
use crate::config;
use crate::delivery_policy::{
    DeliveryEventFamily, DeliveryPolicyCoordinator, DeliveryTransitionEvent,
    persisted_success_transition_names,
};
use crate::error::AtmError;
use crate::identity;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::read::state;
use crate::schema::{AtmMessageId, MessageEnvelope};
use crate::send::{
    PostSendHookContext, ResolvedRecipient, input, persist_message_and_seed_workflow, summary,
};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::{RetainedMailboxRuntime, default_runtime};
use crate::types::{AgentName, CommandAction, IsoTimestamp, TaskId, TeamName};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Map;

/// Parameters for acknowledging one pending-ack mailbox message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckRequest {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub actor_override: Option<AgentName>,
    pub team_override: Option<TeamName>,
    pub message_id: AtmMessageId,
    pub reply_body: String,
}

/// Summary of one successful acknowledgement and reply emission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckOutcome {
    pub action: CommandAction,
    pub team: TeamName,
    pub agent: AgentName,
    pub message_id: AtmMessageId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    pub reply_target: ReplyTarget,
    pub reply_message_id: AtmMessageId,
    pub reply_text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyTarget {
    agent: AgentName,
    team: TeamName,
}

impl ReplyTarget {
    fn new(agent: AgentName, team: TeamName) -> Self {
        Self { agent, team }
    }
}

impl std::fmt::Display for ReplyTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.agent, self.team)
    }
}

impl Serialize for ReplyTarget {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ReplyTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let (agent, team) = value
            .split_once('@')
            .ok_or_else(|| serde::de::Error::custom("expected <agent>@<team> reply target"))?;
        Ok(Self::new(
            agent.parse().map_err(serde::de::Error::custom)?,
            team.parse().map_err(serde::de::Error::custom)?,
        ))
    }
}

/// Acknowledge one previously read pending-ack message and append a reply.
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
/// team resolution fails, the message is missing or no longer pending
/// acknowledgement, reply-target validation fails, or either the source or
/// reply inbox cannot be persisted.
pub fn ack_mail(
    request: AckRequest,
    observability: &dyn ObservabilityPort,
) -> Result<AckOutcome, AtmError> {
    let runtime = default_runtime()?;
    ack_mail_with_runtime(request, observability, &runtime)
}

pub fn ack_mail_with_runtime(
    request: AckRequest,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
) -> Result<AckOutcome, AtmError> {
    ack_mail_with_runtime_impl(request, observability, runtime)
}

fn ack_mail_with_runtime_impl<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    request: AckRequest,
    observability: &dyn ObservabilityPort,
    runtime: &R,
) -> Result<AckOutcome, AtmError> {
    let config = runtime.load_config(&request.current_dir)?;
    let actor =
        identity::resolve_actor_identity(request.actor_override.as_deref(), config.as_ref())?;
    let team = config::resolve_team(request.team_override.as_deref(), config.as_ref())
        .ok_or_else(AtmError::team_unavailable)?;
    let team_dir = runtime.team_dir(&request.home_dir, &team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&team));
    }

    let team_config = runtime.load_team_config(&team_dir)?;
    if !team_config
        .members
        .iter()
        .any(|member| member.name == actor.as_str())
    {
        return Err(AtmError::agent_not_found(&actor, &team));
    }
    ack_mail_with_runtime_sqlite(
        request,
        observability,
        runtime,
        config.as_ref(),
        actor,
        team,
    )
}

fn ack_mail_with_runtime_sqlite<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    request: AckRequest,
    observability: &dyn ObservabilityPort,
    runtime: &R,
    config: Option<&crate::config::AtmConfig>,
    actor: AgentName,
    team: TeamName,
) -> Result<AckOutcome, AtmError> {
    let delivery_policy = DeliveryPolicyCoordinator::new();
    let source = load_ack_source(
        runtime,
        &request.home_dir,
        &team,
        &actor,
        request.message_id,
    )?;
    let reply_target = validate_reply_target(runtime, &request.home_dir, &source.record, &team)?;
    let reply_snapshot = delivery_policy.resolve_recipient_snapshot(
        runtime,
        &reply_target.team,
        &reply_target.agent,
    )?;
    let persisted = persist_ack_reply(runtime, &request, &actor, &team, &source, &reply_target)?;

    let mut outcome = AckOutcome {
        action: CommandAction::Ack,
        team: team.clone(),
        agent: actor.clone(),
        message_id: request.message_id,
        task_id: persisted.task_id.clone(),
        reply_target: reply_target.clone(),
        reply_message_id: persisted.reply_message_id,
        reply_text: persisted.reply_text.clone(),
        warnings: Vec::new(),
    };

    outcome.warnings = collect_ack_hook_warnings(
        runtime,
        config,
        AckHookDispatch {
            actor: &actor,
            team: &team,
            reply_target: &reply_target,
            reply_snapshot: &reply_snapshot,
            reply_message_id: persisted.reply_message_id,
            task_id: outcome.task_id.as_ref(),
        },
    );
    let route =
        delivery_policy.route_persisted_delivery(DeliveryEventFamily::AckReply, &reply_snapshot);
    for transition in
        persisted_success_transition_names(DeliveryEventFamily::AckReply, route.harness)
    {
        delivery_policy.emit_transition(
            observability,
            DeliveryTransitionEvent {
                family: DeliveryEventFamily::AckReply,
                outcome: transition,
                team: &reply_target.team,
                agent: &reply_target.agent,
                sender: &actor,
                message_id: Some(persisted.reply_message_id),
                task_id: persisted.task_id.clone(),
            },
        );
    }

    record_ack_telemetry(
        observability,
        &actor,
        team,
        request.message_id,
        persisted.task_id,
    );

    Ok(outcome)
}

#[derive(Clone)]
struct LoadedAckSource {
    row: boundary::MailStoreMailboxMetadataRow,
    record: boundary::MailStoreMessageRecord,
}

struct PersistedAckReply {
    reply_message_id: AtmMessageId,
    reply_text: String,
    task_id: Option<TaskId>,
}

fn load_ack_source<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &std::path::Path,
    team: &TeamName,
    actor: &AgentName,
    message_id: AtmMessageId,
) -> Result<LoadedAckSource, AtmError> {
    let metadata_rows = runtime.query_mailbox_metadata_rows(home_dir, team, actor, None)?;
    let source_row = find_ack_source_row(&metadata_rows, message_id, actor, team)?;
    ensure_ack_target_is_terminal(&metadata_rows, message_id)?;
    let source_record = load_ack_source_record(runtime, home_dir, team, actor, source_row)?;
    ensure_ack_is_pending(message_id, &source_record.envelope)?;
    Ok(LoadedAckSource {
        row: source_row.clone(),
        record: source_record,
    })
}

fn find_ack_source_row<'a>(
    metadata_rows: &'a [boundary::MailStoreMailboxMetadataRow],
    message_id: AtmMessageId,
    actor: &AgentName,
    team: &TeamName,
) -> Result<&'a boundary::MailStoreMailboxMetadataRow, AtmError> {
    metadata_rows
        .iter()
        .find(|row| row.message_id == Some(message_id))
        .ok_or_else(|| {
            AtmError::validation(format!(
                "message {} was not found in {}@{}",
                message_id, actor, team
            ))
            .with_recovery(
                "Refresh the mailbox with `atm read` and choose a message that is still present in the pending-ack surface.",
            )
        })
}

fn ensure_ack_target_is_terminal(
    metadata_rows: &[boundary::MailStoreMailboxMetadataRow],
    message_id: AtmMessageId,
) -> Result<(), AtmError> {
    if metadata_rows
        .iter()
        .any(|row| row.parent_message_id == Some(message_id))
    {
        return Err(AtmError::validation(format!(
            "message {} has been updated; acknowledge the current terminal message instead",
            message_id
        ))
        .with_recovery(
            "Refresh the mailbox with `atm read` and acknowledge the latest message in the thread instead of an older parent message.",
        ));
    }
    Ok(())
}

fn load_ack_source_record<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &std::path::Path,
    team: &TeamName,
    actor: &AgentName,
    source_row: &boundary::MailStoreMailboxMetadataRow,
) -> Result<boundary::MailStoreMessageRecord, AtmError> {
    runtime
        .load_message_record(home_dir, team, actor, &source_row.message_key)?
        .ok_or_else(|| {
            AtmError::validation(format!(
                "message {} metadata could not be reloaded from sqlite",
                source_row
                    .message_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "<unknown>".to_string())
            ))
            .with_recovery(
                "Repair or remove the malformed sqlite mailbox row before retrying `atm ack`.",
            )
        })
}

fn ensure_ack_is_pending(
    message_id: AtmMessageId,
    source: &MessageEnvelope,
) -> Result<(), AtmError> {
    match state::derive_ack_state(source) {
        crate::types::AckState::PendingAck => Ok(()),
        crate::types::AckState::Acknowledged => Err(AtmError::validation(format!(
            "message {} is already acknowledged",
            message_id
        ))
        .with_recovery(
            "Refresh the mailbox with `atm read` and choose a message that is still pending acknowledgement.",
        )),
        crate::types::AckState::NoAckRequired => Err(AtmError::validation(format!(
            "message {} is not pending acknowledgement",
            message_id
        ))
        .with_recovery(
            "Refresh the mailbox with `atm read` and choose a message that is still pending acknowledgement.",
        )),
    }
}

fn validate_reply_target<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &std::path::Path,
    source_record: &boundary::MailStoreMessageRecord,
    current_team: &TeamName,
) -> Result<ReplyTarget, AtmError> {
    let (reply_agent, reply_team) = resolve_reply_target(&source_record.envelope, current_team);
    let reply_team_dir = runtime.team_dir(home_dir, &reply_team)?;
    if !reply_team_dir.exists() {
        return Err(AtmError::team_not_found(&reply_team));
    }

    let reply_team_config = runtime.load_team_config(&reply_team_dir)?;
    if !reply_team_config
        .members
        .iter()
        .any(|member| member.name == reply_agent.as_str())
    {
        return Err(AtmError::agent_not_found(&reply_agent, &reply_team));
    }

    Ok(ReplyTarget::new(reply_agent, reply_team))
}

fn persist_ack_reply<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    request: &AckRequest,
    actor: &AgentName,
    team: &TeamName,
    source: &LoadedAckSource,
    reply_target: &ReplyTarget,
) -> Result<PersistedAckReply, AtmError> {
    let ack_timestamp = IsoTimestamp::now();
    let reply_text = input::validate_message_text(request.reply_body.clone())?;
    let reply_message_id = AtmMessageId::new();
    let reply_message = MessageEnvelope {
        from: actor.clone(),
        text: reply_text.clone(),
        timestamp: ack_timestamp,
        read: false,
        source_team: Some(team.clone()),
        summary: Some(summary::build_summary(&reply_text, None)),
        message_id: Some(reply_message_id),
        pending_ack_at: None,
        acknowledged_at: None,
        acknowledges_message_id: Some(request.message_id),
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        task_id: None,
        extra: Map::new(),
    };

    runtime.persist_message_state(boundary::MailMessageState {
        team: team.clone(),
        agent: actor.clone(),
        actor: actor.clone(),
        message_key: source.row.message_key.clone(),
        read: true,
        pending_ack_at: None,
        acknowledged_at: Some(ack_timestamp),
        expires_at: source.record.envelope.expires_at,
        deleted_at: None,
        updated_at: Some(ack_timestamp),
    })?;

    let reply_inbox_path =
        runtime.inbox_path(home_dir(request), &reply_target.team, &reply_target.agent)?;
    persist_message_and_seed_workflow(
        runtime,
        home_dir(request),
        &reply_target.team,
        &reply_target.agent,
        &reply_inbox_path,
        &reply_message,
        false,
    )?;

    Ok(PersistedAckReply {
        reply_message_id,
        reply_text,
        task_id: source.record.envelope.task_id.clone(),
    })
}

fn home_dir(request: &AckRequest) -> &std::path::Path {
    request.home_dir.as_path()
}

fn collect_ack_hook_warnings<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    config: Option<&crate::config::AtmConfig>,
    context: AckHookDispatch<'_>,
) -> Vec<String> {
    let reply_recipient = ResolvedRecipient {
        agent: context.reply_target.agent.clone(),
        team: context.reply_target.team.clone(),
    };
    let mut warnings = Vec::new();
    runtime.maybe_run_post_send_hook(
        &mut warnings,
        config,
        PostSendHookContext {
            sender: context.actor,
            sender_team: Some(context.team),
            recipient: &reply_recipient,
            recipient_pane_id: context.reply_snapshot.recipient_pane_id.as_deref(),
            message_id: context.reply_message_id,
            requires_ack: false,
            is_ack: true,
            task_id: context.task_id,
        },
    );
    warnings
        .into_iter()
        .map(|warning| warning.render())
        .collect()
}

struct AckHookDispatch<'a> {
    actor: &'a AgentName,
    team: &'a TeamName,
    reply_target: &'a ReplyTarget,
    reply_snapshot: &'a crate::delivery_policy::DeliveryRecipientSnapshot,
    reply_message_id: AtmMessageId,
    task_id: Option<&'a TaskId>,
}

fn record_ack_telemetry(
    observability: &dyn ObservabilityPort,
    actor: &AgentName,
    team: TeamName,
    message_id: AtmMessageId,
    task_id: Option<TaskId>,
) {
    if let Err(error) = observability.emit(CommandEvent {
        command: "ack",
        action: "ack",
        outcome: "ok",
        team,
        agent: actor.clone(),
        sender: actor.clone(),
        message_id: Some(message_id),
        requires_ack: false,
        dry_run: false,
        task_id,
        error_code: None,
        error_message: None,
    }) {
        tracing::warn!(
            %error,
            subsystem = "ack",
            outcome = "emit_failed",
            command = "ack",
            action = "ack",
            "failed to emit ack command event"
        );
    }
}

fn resolve_reply_target(
    message: &MessageEnvelope,
    current_team: &TeamName,
) -> (AgentName, TeamName) {
    let identity = canonical_sender_identity(message);
    let team = message
        .source_team
        .clone()
        .unwrap_or_else(|| current_team.clone());
    (identity, team)
}

fn canonical_sender_identity(message: &MessageEnvelope) -> AgentName {
    crate::threading::canonical_sender_identity(message)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::Duration;
    use tempfile::tempdir;

    use super::{
        AckRequest, ack_mail_with_runtime_impl, canonical_sender_identity, resolve_reply_target,
    };
    use crate::boundary;
    use crate::config::AtmConfig;
    use crate::error::AtmError;
    use crate::observability::NullObservability;
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::schema::{AtmMessageId, MessageEnvelope, TeamConfig};
    use crate::send::WarningEntry;
    use crate::service_runtime::{RetainedMailboxTimeoutPolicy, RetainedServiceRuntime};
    use crate::service_runtime_store::RetainedMailboxRuntime;
    use crate::test_support::TEST_TEAM;
    use crate::types::{AgentName, IsoTimestamp, TeamName};
    use crate::workflow::WorkflowStateFile;

    /// Minimal test double for `RetainedServiceRuntime + RetainedMailboxRuntime` used
    /// to verify the ack command path does not rewrite the source inbox.
    struct AckStubRuntime {
        home_dir: PathBuf,
        write_called: Cell<bool>,
        compat_export_called: Cell<bool>,
    }

    impl AckStubRuntime {
        fn new(home_dir: PathBuf, team: &str, agent: &str) -> Self {
            let team_dir = home_dir.join("teams").join(team);
            fs::create_dir_all(&team_dir).expect("team dir");
            let config_json = format!(
                r#"{{"members":[{{"name":"{agent}","agent_id":"","agent_type":"","model":"","tmux_pane_id":"","cwd":""}}]}}"#
            );
            fs::write(team_dir.join("config.json"), config_json).expect("team config");
            Self {
                home_dir,
                write_called: Cell::new(false),
                compat_export_called: Cell::new(false),
            }
        }
    }

    impl RetainedServiceRuntime for AckStubRuntime {
        fn load_config(&self, _current_dir: &Path) -> Result<Option<AtmConfig>, AtmError> {
            Ok(None)
        }

        fn load_team_config(&self, team_dir: &Path) -> Result<TeamConfig, AtmError> {
            let path = team_dir.join("config.json");
            let raw = fs::read_to_string(&path).map_err(|_| {
                AtmError::missing_document(format!("team config not found at {}", path.display()))
            })?;
            serde_json::from_str(&raw).map_err(|e| AtmError::config(e.to_string()))
        }

        fn team_dir(&self, _home_dir: &Path, team: &TeamName) -> Result<PathBuf, AtmError> {
            Ok(self.home_dir.join("teams").join(team.as_str()))
        }

        fn inbox_path(
            &self,
            _home_dir: &Path,
            team: &TeamName,
            agent: &AgentName,
        ) -> Result<PathBuf, AtmError> {
            Ok(self
                .home_dir
                .join("teams")
                .join(team.as_str())
                .join(agent.as_str()))
        }

        fn load_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<Option<IsoTimestamp>, AtmError> {
            Ok(None)
        }

        fn save_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _timestamp: IsoTimestamp,
        ) -> Result<(), AtmError> {
            Ok(())
        }

        fn mailbox_timeout_policy(&self) -> RetainedMailboxTimeoutPolicy {
            RetainedMailboxTimeoutPolicy {
                workflow_lock_timeout: Duration::from_secs(5),
            }
        }

        fn maybe_run_post_send_hook(
            &self,
            _warnings: &mut Vec<WarningEntry>,
            _config: Option<&AtmConfig>,
            _context: crate::send::PostSendHookContext<'_>,
        ) {
        }

        fn refresh_compat_inbox_projection(
            &self,
            _home_dir: &Path,
            _recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
        ) -> Result<(), AtmError> {
            self.compat_export_called.set(true);
            Ok(())
        }

        fn load_roster_member(
            &self,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<Option<crate::boundary::RosterMemberRecord>, AtmError> {
            Ok(None)
        }

        fn commit_workflow_state<T, I, F>(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _extra_write_paths: I,
            _timeout: Duration,
            _body: F,
        ) -> Result<T, AtmError>
        where
            I: IntoIterator<Item = PathBuf>,
            F: FnOnce(&mut WorkflowStateFile) -> Result<(T, bool), AtmError>,
        {
            Err(AtmError::validation(
                "AckStubRuntime: commit_workflow_state not exercised in boundary tests",
            ))
        }
    }

    impl RetainedMailboxRuntime for AckStubRuntime {
        fn query_mailbox_metadata_rows(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _limit: Option<usize>,
        ) -> Result<Vec<boundary::MailStoreMailboxMetadataRow>, AtmError> {
            // Return empty rows so ack fails at source-not-found validation,
            // never reaching any write operation.
            Ok(vec![])
        }

        fn load_message_record(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _message_key: &boundary::MessageKey,
        ) -> Result<Option<boundary::MailStoreMessageRecord>, AtmError> {
            Ok(None)
        }

        fn persist_message_record(
            &self,
            _record: boundary::MailStoreMessageRecord,
        ) -> Result<(), AtmError> {
            self.write_called.set(true);
            Ok(())
        }

        fn persist_message_state(
            &self,
            _state: boundary::MailMessageState,
        ) -> Result<(), AtmError> {
            self.write_called.set(true);
            Ok(())
        }
    }

    #[test]
    fn ack_succeeds_without_command_owned_mailbox_rewrite() {
        // Verify the ack command path does not invoke any mailbox write operations
        // when the message to acknowledge is absent.  The ack path must not rewrite
        // the source (actor) inbox — all writes are owned by `persist_ack_reply`
        // which is only reached after the message is validated as pending-ack.
        let tempdir = tempdir().expect("tempdir");
        let stub = AckStubRuntime::new(tempdir.path().to_path_buf(), TEST_TEAM, ROLE_TEAM_LEAD);
        let request = AckRequest {
            home_dir: tempdir.path().to_path_buf(),
            current_dir: PathBuf::from("."),
            actor_override: Some(AgentName::from_validated(ROLE_TEAM_LEAD)),
            team_override: Some(TeamName::from_validated(TEST_TEAM)),
            message_id: AtmMessageId::new(),
            reply_body: "acknowledged".to_string(),
        };

        let result = ack_mail_with_runtime_impl(request, &NullObservability, &stub);

        // Ack fails because the message does not exist — but no write must occur.
        assert!(result.is_err(), "ack should fail when message is not found");
        assert!(
            !stub.write_called.get(),
            "ack command path must not invoke mailbox write operations before message is validated"
        );
    }

    fn message_with_from(from: &str) -> MessageEnvelope {
        MessageEnvelope {
            from: from.parse::<AgentName>().expect("agent"),
            text: "hello".to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(TEST_TEAM.parse::<TeamName>().expect("team")),
            summary: None,
            message_id: None,
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

    #[test]
    fn canonical_sender_identity_uses_from_field() {
        let message = message_with_from(ROLE_TEAM_LEAD);
        assert_eq!(canonical_sender_identity(&message).as_str(), ROLE_TEAM_LEAD);
    }

    #[test]
    fn resolve_reply_target_uses_from_field() {
        let mut message = message_with_from(ROLE_TEAM_LEAD);
        message.source_team = Some(TEST_TEAM.parse::<TeamName>().expect("team"));

        let target = resolve_reply_target(&message, &TeamName::from_validated(TEST_TEAM));
        assert_eq!(
            target,
            (
                ROLE_TEAM_LEAD.parse().expect("agent"),
                TEST_TEAM.parse().expect("team"),
            )
        );
    }
}
