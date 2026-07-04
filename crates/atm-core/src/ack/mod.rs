use std::path::PathBuf;

use crate::boundary;
use crate::delivery_execution::{
    DeliveryTransitionContext, emit_reply_delivery_plan_transitions, execute_reply_delivery_plan,
};
use crate::delivery_plan::{
    DeliveryPlan, DeliveryPlanKind, LogicalMessage, delivery_plan_disposition,
    delivery_target_for_snapshot, logical_messages_from_persistence,
};
use crate::delivery_policy::{DeliveryEventFamily, DeliveryPolicyCoordinator};
use crate::error::AtmError;
use crate::observability::{CommandEvent, ObservabilityPort, action_name, outcome_label};
use crate::read::state;
use crate::schema::{AtmMessageId, InboxMessage};
use crate::send::{ResolvedRecipient, input, persist_message_and_seed_workflow, summary};
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
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
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
    pub warnings: Vec<crate::send::WarningEntry>,
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

fn ack_mail_with_runtime_impl<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    request: AckRequest,
    observability: &dyn ObservabilityPort,
    runtime: &R,
) -> Result<AckOutcome, AtmError> {
    let config = runtime.load_config(&request.current_dir)?;
    let actor = request.caller_identity.clone();
    let team = request.caller_team.clone();
    let team_dir = runtime.team_dir(&request.home_dir, &team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&team));
    }

    ensure_roster_member_exists(
        runtime,
        &team,
        &actor,
        "Repair or reload the ATM roster before retrying `atm ack`.",
    )?;
    ack_mail_with_runtime_sqlite(
        request,
        observability,
        runtime,
        config.as_ref(),
        actor,
        team,
    )
}

fn ack_mail_with_runtime_sqlite<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
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
    let persisted = persist_ack_reply(
        runtime,
        AckPersistenceContext {
            request: &request,
            actor: &actor,
            team: &team,
            source: &source,
            reply_target: &reply_target,
        },
    )?;
    finalize_ack_outcome(
        runtime,
        observability,
        config,
        FinalizeAckContext {
            actor: &actor,
            team: &team,
            request_message_id: request.message_id,
            reply_target: &reply_target,
            reply_snapshot: &reply_snapshot,
            persisted: &persisted,
        },
    )
}

#[derive(Clone)]
struct LoadedAckSource {
    row: boundary::MailStoreMailboxMetadataRow,
    record: boundary::Message,
}

struct PersistedAckReply {
    reply_message_id: AtmMessageId,
    reply_text: String,
    task_id: Option<TaskId>,
    reply_inbox_path: PathBuf,
    persistence: crate::send::DeliveryPersistenceResult,
}

struct FinalizeAckContext<'a> {
    actor: &'a AgentName,
    team: &'a TeamName,
    request_message_id: AtmMessageId,
    reply_target: &'a ReplyTarget,
    reply_snapshot: &'a crate::delivery_policy::DeliveryRecipientSnapshot,
    persisted: &'a PersistedAckReply,
}

struct AckPersistenceContext<'a> {
    request: &'a AckRequest,
    actor: &'a AgentName,
    team: &'a TeamName,
    source: &'a LoadedAckSource,
    reply_target: &'a ReplyTarget,
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
) -> Result<boundary::Message, AtmError> {
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

fn ensure_ack_is_pending(message_id: AtmMessageId, source: &InboxMessage) -> Result<(), AtmError> {
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
    source_record: &boundary::Message,
    current_team: &TeamName,
) -> Result<ReplyTarget, AtmError> {
    let (reply_agent, reply_team) = resolve_reply_target(&source_record.envelope, current_team);
    let reply_team_dir = runtime.team_dir(home_dir, &reply_team)?;
    if !reply_team_dir.exists() {
        return Err(AtmError::team_not_found(&reply_team));
    }

    ensure_roster_member_exists(
        runtime,
        &reply_team,
        &reply_agent,
        "Repair or reload the ATM roster before retrying the acknowledgement reply.",
    )?;

    Ok(ReplyTarget::new(reply_agent, reply_team))
}

fn ensure_roster_member_exists<R: RetainedServiceRuntime>(
    runtime: &R,
    team: &TeamName,
    agent: &AgentName,
    recovery: &str,
) -> Result<(), AtmError> {
    if runtime.load_roster_member(team, agent)?.is_none() {
        return Err(AtmError::agent_not_found(agent, team).with_recovery(recovery));
    }

    Ok(())
}

fn persist_ack_reply<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    context: AckPersistenceContext<'_>,
) -> Result<PersistedAckReply, AtmError> {
    let ack_timestamp = IsoTimestamp::now();
    let reply_text = input::validate_message_text(context.request.reply_body.clone())?;
    let reply_message_id = AtmMessageId::new();
    let reply_message = InboxMessage {
        from: context.actor.clone(),
        text: reply_text.clone(),
        timestamp: ack_timestamp,
        read: false,
        source_team: Some(context.team.clone()),
        summary: Some(summary::build_summary(&reply_text, None)),
        message_id: Some(reply_message_id),
        pending_ack_at: None,
        acknowledged_at: None,
        acknowledges_message_id: Some(context.request.message_id),
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        task_id: None,
        extra: Map::new(),
    };

    runtime.persist_message_state(boundary::MailMessageState {
        team: context.team.clone(),
        agent: context.actor.clone(),
        actor: context.actor.clone(),
        message_key: context.source.row.message_key.clone(),
        read: true,
        pending_ack_at: None,
        acknowledged_at: Some(ack_timestamp),
        expires_at: context.source.record.envelope.expires_at,
        deleted_at: None,
        updated_at: Some(ack_timestamp),
    })?;

    let reply_inbox_path = runtime.inbox_path(
        home_dir(context.request),
        &context.reply_target.team,
        &context.reply_target.agent,
    )?;
    let reply_snapshot = DeliveryPolicyCoordinator::new().resolve_recipient_snapshot(
        runtime,
        &context.reply_target.team,
        &context.reply_target.agent,
    )?;
    let persistence = persist_message_and_seed_workflow(
        runtime,
        home_dir(context.request),
        &reply_snapshot,
        &reply_inbox_path,
        &reply_message,
        false,
    )?;

    Ok(PersistedAckReply {
        reply_message_id,
        reply_text,
        task_id: context.source.record.envelope.task_id.clone(),
        reply_inbox_path,
        persistence,
    })
}

fn home_dir(request: &AckRequest) -> &std::path::Path {
    request.home_dir.as_path()
}

fn finalize_ack_outcome<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    runtime: &R,
    observability: &dyn ObservabilityPort,
    config: Option<&crate::config::AtmConfig>,
    context: FinalizeAckContext<'_>,
) -> Result<AckOutcome, AtmError> {
    let plan = build_reply_delivery_plan(&context)?;
    let execution = execute_reply_delivery_plan(runtime, config, &plan)?;
    let mut outcome = AckOutcome {
        action: CommandAction::Ack,
        team: context.team.clone(),
        agent: context.actor.clone(),
        message_id: context.request_message_id,
        task_id: context.persisted.task_id.clone(),
        reply_target: context.reply_target.clone(),
        reply_message_id: context.persisted.reply_message_id,
        reply_text: context.persisted.reply_text.clone(),
        warnings: Vec::new(),
    };
    outcome.warnings.extend(plan.warnings.iter().cloned());
    emit_reply_delivery_plan_transitions(
        observability,
        DeliveryTransitionContext {
            family: DeliveryEventFamily::AckReply,
            team: &context.reply_target.team,
            agent: &context.reply_target.agent,
            sender: context.actor,
            message_id: context.persisted.reply_message_id,
            task_id: context.persisted.task_id.clone(),
        },
        &plan,
        &execution,
    )?;
    outcome.warnings.extend(execution.warnings);
    record_ack_telemetry(
        observability,
        context.actor,
        context.team.clone(),
        context.request_message_id,
        context.persisted.task_id.clone(),
    );
    Ok(outcome)
}

// Distinct from `crate::delivery_policy::AckReplyStateMachine`, which
// documents the transition inventory. This seam owns typed reply-plan
// construction from persisted delivery results.
enum AckReplyStateMachine {
    Persisted {
        messages: Vec<LogicalMessage>,
        warnings: Vec<crate::send::WarningEntry>,
    },
    SqliteFailedRecovered {
        messages: Vec<LogicalMessage>,
        warnings: Vec<crate::send::WarningEntry>,
    },
}

impl AckReplyStateMachine {
    fn from_persistence(
        persistence: &crate::send::DeliveryPersistenceResult,
    ) -> Result<Self, AtmError> {
        let messages = logical_messages_from_persistence(persistence, false, true)
            .map_err(|error| {
                AtmError::mailbox_write(error.to_string()).with_recovery(
                    "Repair the persisted reply-delivery record shape before retrying ack reply execution.",
                )
            })?;
        let warnings = persistence.warnings.clone();
        Ok(match persistence.disposition {
            crate::send::DeliveryPersistenceDisposition::Persisted => {
                Self::Persisted { messages, warnings }
            }
            crate::send::DeliveryPersistenceDisposition::SqliteFailedRecovered => {
                Self::SqliteFailedRecovered { messages, warnings }
            }
        })
    }

    fn into_reply_delivery_plan(
        self,
        reply_target: &ReplyTarget,
        reply_snapshot: &crate::delivery_policy::DeliveryRecipientSnapshot,
        reply_inbox_path: &std::path::Path,
    ) -> DeliveryPlan {
        let (disposition, messages, warnings) = match self {
            Self::Persisted { messages, warnings } => (
                crate::send::DeliveryPersistenceDisposition::Persisted,
                messages,
                warnings,
            ),
            Self::SqliteFailedRecovered { messages, warnings } => (
                crate::send::DeliveryPersistenceDisposition::SqliteFailedRecovered,
                messages,
                warnings,
            ),
        };
        DeliveryPlan::new(
            DeliveryPlanKind::Reply,
            delivery_plan_disposition(disposition),
            delivery_target_for_snapshot(reply_inbox_path, reply_snapshot),
            ResolvedRecipient {
                agent: reply_target.agent.clone(),
                team: reply_target.team.clone(),
            },
            reply_snapshot.recipient_pane_id.clone(),
            messages,
            warnings,
        )
    }
}

fn build_reply_delivery_plan(context: &FinalizeAckContext<'_>) -> Result<DeliveryPlan, AtmError> {
    Ok(
        AckReplyStateMachine::from_persistence(&context.persisted.persistence)?
            .into_reply_delivery_plan(
                context.reply_target,
                context.reply_snapshot,
                &context.persisted.reply_inbox_path,
            ),
    )
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
        action: action_name("ack"),
        outcome: outcome_label("ok"),
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

fn resolve_reply_target(message: &InboxMessage, current_team: &TeamName) -> (AgentName, TeamName) {
    let identity = canonical_sender_identity(message);
    let team = message
        .source_team
        .clone()
        .unwrap_or_else(|| current_team.clone());
    (identity, team)
}

fn canonical_sender_identity(message: &InboxMessage) -> AgentName {
    crate::threading::canonical_sender_identity(message)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::Duration;

    use super::{
        AckReplyStateMachine, FinalizeAckContext, PersistedAckReply, ReplyTarget,
        canonical_sender_identity, finalize_ack_outcome, resolve_reply_target,
    };
    use crate::boundary::{
        self, MessageKey, NonClaudeOutboundDeliveryRequest, ProjectionAppendMode,
    };
    use crate::delivery_plan::{DeliveryPlanDisposition, DeliveryTarget};
    use crate::observability::NullObservability;
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::schema::{AtmMessageId, InboxMessage, TeamConfig};
    use crate::send::{DeliveryPersistenceDisposition, DeliveryPersistenceResult, WarningEntry};
    use crate::service_runtime::{RetainedMailboxTimeoutPolicy, RetainedServiceRuntime};
    use crate::service_runtime_store::RetainedMailboxRuntime;
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, IsoTimestamp, TeamName};
    use crate::workflow::WorkflowStateFile;
    use serde_json::Map;

    struct AckRuntime {
        outbound_deliveries: Mutex<Vec<NonClaudeOutboundDeliveryRequest>>,
    }

    impl AckRuntime {
        fn outbound_messages(&self) -> Vec<InboxMessage> {
            self.outbound_deliveries
                .lock()
                .expect("non-claude deliveries")
                .iter()
                .flat_map(|request| request.messages.clone())
                .collect()
        }
    }

    struct AckRosterRuntime {
        team_dir: PathBuf,
        roster_members: Vec<(TeamName, AgentName)>,
        inbox_path: PathBuf,
        source_row: boundary::MailStoreMailboxMetadataRow,
        source_record: boundary::Message,
        outbound_deliveries: Mutex<Vec<NonClaudeOutboundDeliveryRequest>>,
    }

    impl crate::boundary::sealed::Sealed for AckRuntime {}

    impl crate::boundary::sealed::Sealed for AckRosterRuntime {}

    impl RetainedServiceRuntime for AckRuntime {
        fn load_config(
            &self,
            _current_dir: &Path,
        ) -> Result<Option<crate::config::AtmConfig>, crate::error::AtmError> {
            Ok(None)
        }

        fn load_team_config_for_doctor_compare(
            &self,
            _team_dir: &Path,
        ) -> Result<TeamConfig, crate::error::AtmError> {
            unreachable!("ack writer-path test does not load team config")
        }

        fn team_dir(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
        ) -> Result<PathBuf, crate::error::AtmError> {
            unreachable!("ack writer-path test does not resolve team directories")
        }

        fn inbox_path(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<PathBuf, crate::error::AtmError> {
            unreachable!("ack writer-path test does not resolve inbox paths")
        }

        fn load_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<Option<IsoTimestamp>, crate::error::AtmError> {
            Ok(None)
        }

        fn save_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _timestamp: IsoTimestamp,
        ) -> Result<(), crate::error::AtmError> {
            Ok(())
        }

        fn mailbox_timeout_policy(&self) -> RetainedMailboxTimeoutPolicy {
            RetainedMailboxTimeoutPolicy {
                workflow_lock_timeout: Duration::from_millis(1),
            }
        }

        fn rebuild_compat_inbox_projection(
            &self,
            _inbox_path: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<(), crate::error::AtmError> {
            unreachable!("ack writer-path test should not rebuild compatibility inboxes")
        }

        fn append_compat_inbox_message(
            &self,
            _inbox_path: &Path,
            _message: &InboxMessage,
        ) -> Result<(), crate::error::AtmError> {
            panic!("ack writer-path test should not use the retired Claude inbox append path")
        }

        fn append_compat_inbox_message_set(
            &self,
            _inbox_path: &Path,
            _mode: ProjectionAppendMode,
            _messages: &[InboxMessage],
        ) -> Result<(), crate::error::AtmError> {
            panic!("ack writer-path test should use the single-message Claude inbox writer path")
        }

        fn deliver_non_claude_payloads(
            &self,
            recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
            messages: &[InboxMessage],
        ) -> Result<(), crate::error::AtmError> {
            self.outbound_deliveries
                .lock()
                .expect("non-claude deliveries")
                .push(NonClaudeOutboundDeliveryRequest {
                    team: recipient.team.clone(),
                    agent: recipient.agent.clone(),
                    recipient_pane_id: recipient.recipient_pane_id.clone(),
                    messages: messages.to_vec(),
                });
            Ok(())
        }

        fn load_roster_member(
            &self,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<Option<boundary::RosterEntry>, crate::error::AtmError> {
            Ok(None)
        }

        fn load_team_roster(
            &self,
            _team: &TeamName,
        ) -> Result<Vec<boundary::RosterEntry>, crate::error::AtmError> {
            Ok(Vec::new())
        }

        fn commit_workflow_state<T, I, F>(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _extra_write_paths: I,
            _timeout: Duration,
            _body: F,
        ) -> Result<T, crate::error::AtmError>
        where
            I: IntoIterator<Item = PathBuf>,
            F: FnOnce(&mut WorkflowStateFile) -> Result<(T, bool), crate::error::AtmError>,
        {
            unreachable!("ack writer-path test does not commit workflow state")
        }
    }

    impl RetainedServiceRuntime for AckRosterRuntime {
        fn load_config(
            &self,
            _current_dir: &Path,
        ) -> Result<Option<crate::config::AtmConfig>, crate::error::AtmError> {
            Ok(Some(crate::config::AtmConfig {
                default_team: Some(TEST_TEAM.parse::<TeamName>().expect("team")),
                ..Default::default()
            }))
        }

        fn load_team_config_for_doctor_compare(
            &self,
            _team_dir: &Path,
        ) -> Result<TeamConfig, crate::error::AtmError> {
            unreachable!("ack roster-gate tests must not load team config")
        }

        fn team_dir(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
        ) -> Result<PathBuf, crate::error::AtmError> {
            Ok(self.team_dir.clone())
        }

        fn inbox_path(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<PathBuf, crate::error::AtmError> {
            Ok(self.inbox_path.clone())
        }

        fn load_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<Option<IsoTimestamp>, crate::error::AtmError> {
            Ok(None)
        }

        fn save_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _timestamp: IsoTimestamp,
        ) -> Result<(), crate::error::AtmError> {
            Ok(())
        }

        fn mailbox_timeout_policy(&self) -> RetainedMailboxTimeoutPolicy {
            RetainedMailboxTimeoutPolicy {
                workflow_lock_timeout: Duration::from_millis(1),
            }
        }

        fn rebuild_compat_inbox_projection(
            &self,
            _inbox_path: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<(), crate::error::AtmError> {
            unreachable!("ack roster-gate tests do not rebuild compatibility inboxes")
        }

        fn append_compat_inbox_message(
            &self,
            _inbox_path: &Path,
            _message: &InboxMessage,
        ) -> Result<(), crate::error::AtmError> {
            unreachable!(
                "ack roster-gate tests should not use the retired Claude inbox append path"
            )
        }

        fn append_compat_inbox_message_set(
            &self,
            _inbox_path: &Path,
            _mode: ProjectionAppendMode,
            _messages: &[InboxMessage],
        ) -> Result<(), crate::error::AtmError> {
            unreachable!("ack roster-gate tests do not append compatibility inbox message sets")
        }

        fn deliver_non_claude_payloads(
            &self,
            recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
            messages: &[InboxMessage],
        ) -> Result<(), crate::error::AtmError> {
            self.outbound_deliveries
                .lock()
                .expect("non-claude deliveries")
                .push(NonClaudeOutboundDeliveryRequest {
                    team: recipient.team.clone(),
                    agent: recipient.agent.clone(),
                    recipient_pane_id: recipient.recipient_pane_id.clone(),
                    messages: messages.to_vec(),
                });
            Ok(())
        }

        fn load_roster_member(
            &self,
            team: &TeamName,
            agent: &AgentName,
        ) -> Result<Option<boundary::RosterEntry>, crate::error::AtmError> {
            Ok(self
                .roster_members
                .iter()
                .any(|(member_team, member_agent)| member_team == team && member_agent == agent)
                .then(|| boundary::RosterEntry {
                    team_name: team.clone(),
                    agent_name: agent.clone(),
                    member_kind: boundary::RosterMemberKind::Permanent,
                    harness: boundary::RosterHarness::ClaudeCode,
                    agent_type: crate::schema::AgentType::default(),
                    model: crate::types::ModelName::default(),
                    recipient_pane_id: None,
                    metadata_json: Map::new(),
                }))
        }

        fn load_team_roster(
            &self,
            _team: &TeamName,
        ) -> Result<Vec<boundary::RosterEntry>, crate::error::AtmError> {
            Ok(Vec::new())
        }

        fn commit_workflow_state<T, I, F>(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _extra_write_paths: I,
            _timeout: Duration,
            body: F,
        ) -> Result<T, crate::error::AtmError>
        where
            I: IntoIterator<Item = PathBuf>,
            F: FnOnce(&mut WorkflowStateFile) -> Result<(T, bool), crate::error::AtmError>,
        {
            let mut workflow = WorkflowStateFile::default();
            let (result, _changed) = body(&mut workflow)?;
            Ok(result)
        }
    }

    impl RetainedMailboxRuntime for AckRosterRuntime {
        fn query_mailbox_metadata_rows(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _limit: Option<usize>,
        ) -> Result<Vec<boundary::MailStoreMailboxMetadataRow>, crate::error::AtmError> {
            Ok(vec![self.source_row.clone()])
        }

        fn load_message_record(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            message_key: &MessageKey,
        ) -> Result<Option<boundary::Message>, crate::error::AtmError> {
            Ok((message_key == &self.source_row.message_key).then(|| self.source_record.clone()))
        }

        fn persist_message_record(
            &self,
            _record: boundary::Message,
        ) -> Result<(), crate::error::AtmError> {
            Ok(())
        }

        fn persist_message_state(
            &self,
            _state: boundary::MailMessageState,
        ) -> Result<(), crate::error::AtmError> {
            Ok(())
        }
    }

    impl RetainedMailboxRuntime for AckRuntime {
        fn query_mailbox_metadata_rows(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _limit: Option<usize>,
        ) -> Result<Vec<boundary::MailStoreMailboxMetadataRow>, crate::error::AtmError> {
            unreachable!("ack writer-path test does not query mailbox metadata")
        }

        fn load_message_record(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _message_key: &MessageKey,
        ) -> Result<Option<boundary::Message>, crate::error::AtmError> {
            unreachable!("ack writer-path test does not load mailbox records")
        }

        fn persist_message_record(
            &self,
            _record: boundary::Message,
        ) -> Result<(), crate::error::AtmError> {
            unreachable!("ack writer-path test does not persist mailbox records")
        }

        fn persist_message_state(
            &self,
            _state: boundary::MailMessageState,
        ) -> Result<(), crate::error::AtmError> {
            unreachable!("ack writer-path test does not persist mailbox state")
        }
    }

    fn message_with_from(from: &str) -> InboxMessage {
        InboxMessage {
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

    #[test]
    fn ack_reply_state_machine_builds_reply_plan_with_original_and_companion() {
        let team = TEST_TEAM.parse::<TeamName>().expect("team");
        let agent = ROLE_TEAM_LEAD.parse::<AgentName>().expect("agent");
        let mut original = message_with_from(ROLE_TEAM_LEAD);
        original.message_id = Some(AtmMessageId::new());
        let mut companion = message_with_from("atm-system");
        companion.message_id = Some(AtmMessageId::new());
        companion.source_team = Some(team.clone());
        let persistence = DeliveryPersistenceResult {
            disposition: DeliveryPersistenceDisposition::SqliteFailedRecovered,
            original_message: original.clone(),
            companion_message: Some(companion.clone()),
            warnings: vec![WarningEntry::new("warning".to_string(), Some("recovery"))],
        };
        let snapshot = crate::delivery_policy::DeliveryRecipientSnapshot {
            agent: agent.clone(),
            team: team.clone(),
            harness: crate::delivery_policy::DeliveryHarnessPath::ClaudeCode,
            recipient_pane_id: None,
            roster_backed: true,
        };
        let machine = AckReplyStateMachine::from_persistence(&persistence).expect("state machine");
        let plan = machine.into_reply_delivery_plan(
            &super::ReplyTarget::new(agent.clone(), team.clone()),
            &snapshot,
            std::path::Path::new("reply.jsonl"),
        );

        assert_eq!(
            plan.disposition,
            DeliveryPlanDisposition::SqliteFailedRecovered
        );
        assert_eq!(plan.messages.len(), 2);
        assert_eq!(plan.messages[0].envelope, original);
        assert_eq!(plan.messages[1].envelope, companion);
        assert_eq!(plan.notifications.len(), 2);
        assert_eq!(plan.warnings.len(), 1);
        match plan.delivery_target {
            DeliveryTarget::NonClaude { .. } => {}
            DeliveryTarget::ClaudeCode { .. } => {
                panic!("expected non-Claude target for retired Claude harness path")
            }
        }
    }

    #[test]
    fn ack_write_goes_through_non_claude_outbound_boundary() {
        let team = TEST_TEAM.parse::<TeamName>().expect("team");
        let agent = ROLE_TEAM_LEAD.parse::<AgentName>().expect("agent");
        let reply_message_id = AtmMessageId::new();
        let request_message_id = AtmMessageId::new();
        let reply_text = "ack reply".to_string();
        let reply_message = InboxMessage {
            from: "sender".parse::<AgentName>().expect("agent"),
            text: reply_text.clone(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(team.clone()),
            summary: None,
            message_id: Some(reply_message_id),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: Some(request_message_id),
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            extra: serde_json::Map::new(),
        };
        let persistence = DeliveryPersistenceResult {
            disposition: DeliveryPersistenceDisposition::Persisted,
            original_message: reply_message.clone(),
            companion_message: None,
            warnings: Vec::new(),
        };
        let reply_snapshot = crate::delivery_policy::DeliveryRecipientSnapshot {
            agent: agent.clone(),
            team: team.clone(),
            harness: crate::delivery_policy::DeliveryHarnessPath::ClaudeCode,
            recipient_pane_id: None,
            roster_backed: true,
        };
        let reply_target = ReplyTarget::new(agent.clone(), team.clone());
        let persisted = PersistedAckReply {
            reply_message_id,
            reply_text: reply_text.clone(),
            task_id: None,
            reply_inbox_path: PathBuf::from("reply.jsonl"),
            persistence,
        };
        let runtime = AckRuntime {
            outbound_deliveries: Mutex::new(Vec::new()),
        };

        let outcome = finalize_ack_outcome(
            &runtime,
            &NullObservability,
            None,
            FinalizeAckContext {
                actor: &"sender".parse::<AgentName>().expect("agent"),
                team: &team,
                request_message_id,
                reply_target: &reply_target,
                reply_snapshot: &reply_snapshot,
                persisted: &persisted,
            },
        )
        .expect("finalize ack outcome");

        let outbound_messages = runtime.outbound_messages();
        assert_eq!(outbound_messages.len(), 1);
        assert_eq!(outbound_messages[0], reply_message);
        assert_eq!(outcome.reply_message_id, reply_message_id);
        assert_eq!(outcome.reply_text, reply_text);
    }

    #[test]
    fn ack_mail_rejects_actor_missing_from_atm_roster() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        std::fs::create_dir_all(&team_dir).expect("team dir");
        let runtime = AckRosterRuntime {
            team_dir,
            roster_members: Vec::new(),
            inbox_path: tempdir.path().join("reply.jsonl"),
            source_row: boundary::MailStoreMailboxMetadataRow {
                message_key: MessageKey::new("atm:source").expect("message key"),
                message_id: Some(AtmMessageId::new()),
                parent_message_id: None,
                thread_mode: None,
                from_agent: TEST_SENDER.parse().expect("agent"),
                summary: Some("summary".to_string()),
                message_at: IsoTimestamp::now(),
                read: false,
                pending_ack: true,
                acknowledged_at: None,
                expires_at: None,
                task_id: None,
            },
            source_record: boundary::Message {
                team: TEST_TEAM.parse().expect("team"),
                agent: TEST_SENDER.parse().expect("agent"),
                message_key: MessageKey::new("atm:source").expect("message key"),
                envelope: InboxMessage {
                    from: TEST_SENDER.parse().expect("agent"),
                    text: "source".to_string(),
                    timestamp: IsoTimestamp::now(),
                    read: false,
                    source_team: Some(TEST_TEAM.parse().expect("team")),
                    summary: Some("summary".to_string()),
                    message_id: Some(AtmMessageId::new()),
                    pending_ack_at: Some(IsoTimestamp::now()),
                    acknowledged_at: None,
                    acknowledges_message_id: None,
                    parent_message_id: None,
                    thread_mode: None,
                    expires_at: None,
                    task_id: None,
                    extra: Map::new(),
                },
            },
            outbound_deliveries: Mutex::new(Vec::new()),
        };

        let error = super::ack_mail_with_runtime_impl(
            crate::ack::AckRequest {
                home_dir: tempdir.path().to_path_buf(),
                current_dir: tempdir.path().to_path_buf(),
                caller_identity: TEST_SENDER.parse().expect("caller"),
                caller_team: TEST_TEAM.parse().expect("team"),
                message_id: AtmMessageId::new(),
                reply_body: "ack".to_string(),
            },
            &NullObservability,
            &runtime,
        )
        .expect_err("missing ATM roster member should fail");

        assert!(error.is_agent_not_found(), "{error:?}");
    }

    #[test]
    fn ack_mail_uses_atm_roster_truth_for_valid_actor() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let team = TEST_TEAM.parse::<TeamName>().expect("team");
        let actor = TEST_SENDER.parse::<AgentName>().expect("agent");
        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        std::fs::create_dir_all(team_dir.join("inboxes")).expect("team inbox dir");
        let source_message_id = AtmMessageId::new();
        let source_key = MessageKey::new(format!("atm:{source_message_id}")).expect("message key");
        let runtime = AckRosterRuntime {
            team_dir,
            roster_members: vec![(team.clone(), actor.clone())],
            inbox_path: tempdir.path().join("reply.jsonl"),
            source_row: boundary::MailStoreMailboxMetadataRow {
                message_key: source_key.clone(),
                message_id: Some(source_message_id),
                parent_message_id: None,
                thread_mode: None,
                from_agent: actor.clone(),
                summary: Some("summary".to_string()),
                message_at: IsoTimestamp::now(),
                read: false,
                pending_ack: true,
                acknowledged_at: None,
                expires_at: None,
                task_id: None,
            },
            source_record: boundary::Message {
                team: team.clone(),
                agent: actor.clone(),
                message_key: source_key,
                envelope: InboxMessage {
                    from: actor.clone(),
                    text: "source".to_string(),
                    timestamp: IsoTimestamp::now(),
                    read: false,
                    source_team: Some(team.clone()),
                    summary: Some("summary".to_string()),
                    message_id: Some(source_message_id),
                    pending_ack_at: Some(IsoTimestamp::now()),
                    acknowledged_at: None,
                    acknowledges_message_id: None,
                    parent_message_id: None,
                    thread_mode: None,
                    expires_at: None,
                    task_id: None,
                    extra: Map::new(),
                },
            },
            outbound_deliveries: Mutex::new(Vec::new()),
        };

        let outcome = super::ack_mail_with_runtime_impl(
            crate::ack::AckRequest {
                home_dir: tempdir.path().to_path_buf(),
                current_dir: tempdir.path().to_path_buf(),
                caller_identity: actor.clone(),
                caller_team: team.clone(),
                message_id: source_message_id,
                reply_body: "ack".to_string(),
            },
            &NullObservability,
            &runtime,
        )
        .expect("valid ATM roster member should ack successfully");

        assert_eq!(outcome.team, team);
        assert_eq!(outcome.agent, actor);
        assert_eq!(outcome.message_id, source_message_id);
        assert!(
            !runtime
                .outbound_deliveries
                .lock()
                .expect("non-claude deliveries")
                .is_empty()
        );
    }
}
