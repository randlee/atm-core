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
    let source_snapshot = delivery_policy.resolve_recipient_snapshot(runtime, &team, &actor)?;
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
            source_snapshot: &source_snapshot,
            reply_target: &reply_target,
        },
    )?;
    finalize_ack_outcome(
        runtime,
        observability,
        config,
        FinalizeAckContext {
            delivery_policy: &delivery_policy,
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
    record: boundary::MailStoreMessageRecord,
}

struct PersistedAckReply {
    reply_message_id: AtmMessageId,
    reply_text: String,
    task_id: Option<TaskId>,
}

struct FinalizeAckContext<'a> {
    delivery_policy: &'a DeliveryPolicyCoordinator,
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
    source_snapshot: &'a crate::delivery_policy::DeliveryRecipientSnapshot,
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
    context: AckPersistenceContext<'_>,
) -> Result<PersistedAckReply, AtmError> {
    let ack_timestamp = IsoTimestamp::now();
    let reply_text = input::validate_message_text(context.request.reply_body.clone())?;
    let reply_message_id = AtmMessageId::new();
    let reply_message = MessageEnvelope {
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
    runtime.refresh_compat_inbox_projection(home_dir(context.request), context.source_snapshot)?;

    let reply_inbox_path = runtime.inbox_path(
        home_dir(context.request),
        &context.reply_target.team,
        &context.reply_target.agent,
    )?;
    persist_message_and_seed_workflow(
        runtime,
        home_dir(context.request),
        &context.reply_target.team,
        &context.reply_target.agent,
        &reply_inbox_path,
        &reply_message,
        false,
    )?;

    Ok(PersistedAckReply {
        reply_message_id,
        reply_text,
        task_id: context.source.record.envelope.task_id.clone(),
    })
}

fn home_dir(request: &AckRequest) -> &std::path::Path {
    request.home_dir.as_path()
}

fn finalize_ack_outcome<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    observability: &dyn ObservabilityPort,
    config: Option<&crate::config::AtmConfig>,
    context: FinalizeAckContext<'_>,
) -> Result<AckOutcome, AtmError> {
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
    outcome.warnings = collect_ack_hook_warnings(
        runtime,
        config,
        AckHookDispatch {
            actor: context.actor,
            team: context.team,
            reply_target: context.reply_target,
            reply_snapshot: context.reply_snapshot,
            reply_message_id: context.persisted.reply_message_id,
            task_id: outcome.task_id.as_ref(),
        },
    );
    emit_ack_delivery_transitions(observability, &context);
    record_ack_telemetry(
        observability,
        context.actor,
        context.team.clone(),
        context.request_message_id,
        context.persisted.task_id.clone(),
    );
    Ok(outcome)
}

fn emit_ack_delivery_transitions(
    observability: &dyn ObservabilityPort,
    context: &FinalizeAckContext<'_>,
) {
    let route = context
        .delivery_policy
        .route_persisted_delivery(DeliveryEventFamily::AckReply, context.reply_snapshot);
    for transition in
        persisted_success_transition_names(DeliveryEventFamily::AckReply, route.harness)
    {
        context.delivery_policy.emit_transition(
            observability,
            DeliveryTransitionEvent {
                family: DeliveryEventFamily::AckReply,
                outcome: transition,
                team: &context.reply_target.team,
                agent: &context.reply_target.agent,
                sender: context.actor,
                message_id: Some(context.persisted.reply_message_id),
                task_id: context.persisted.task_id.clone(),
            },
        );
    }
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
    use super::{canonical_sender_identity, resolve_reply_target};
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::schema::MessageEnvelope;
    use crate::test_support::TEST_TEAM;
    use crate::types::{AgentName, IsoTimestamp, TeamName};

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
