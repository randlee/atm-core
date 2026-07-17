use std::path::PathBuf;

use crate::boundary;
use crate::boundary::{AckReplyDeliveryPort, PostSendHookEmitter};
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
use crate::schema::{AckIntentFields, AtmMessageId, InboxMessage};
use crate::send::{
    RemoteTargetHost, ResolvedRecipient, SendMessageSource, SendRequest, input, persist_message,
    summary,
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
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    pub message_id: AtmMessageId,
    pub reply_body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AckReplyDisposition {
    SuppressedSelfAck,
    Sent {
        reply_message_id: AtmMessageId,
        reply_target: ReplyTarget,
    },
}

impl AckReplyDisposition {
    pub fn is_suppressed_self_ack(&self) -> bool {
        matches!(self, Self::SuppressedSelfAck)
    }
}

/// Summary of one successful acknowledgement and reply handling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckOutcome {
    pub action: CommandAction,
    pub team: TeamName,
    pub agent: AgentName,
    pub message_id: AtmMessageId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    pub reply_disposition: AckReplyDisposition,
    pub reply_text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<crate::send::WarningEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyTarget {
    agent: AgentName,
    team: TeamName,
    host: Option<RemoteTargetHost>,
}

impl ReplyTarget {
    fn new(agent: AgentName, team: TeamName, host: Option<RemoteTargetHost>) -> Self {
        Self { agent, team, host }
    }
}

impl std::fmt::Display for ReplyTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.agent, self.team)?;
        if let Some(host) = &self.host {
            write!(f, ".{}", host.as_str())?;
        }
        Ok(())
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
        let (agent, team_and_host) = value
            .split_once('@')
            .ok_or_else(|| serde::de::Error::custom("expected <agent>@<team> reply target"))?;
        let (team, host) = match team_and_host.split_once('.') {
            Some((team, host)) => (
                team,
                Some(
                    RemoteTargetHost::parse(host)
                        .map_err(|error| serde::de::Error::custom(error.message))?,
                ),
            ),
            None => (team_and_host, None),
        };
        Ok(Self::new(
            agent.parse().map_err(serde::de::Error::custom)?,
            team.parse().map_err(serde::de::Error::custom)?,
            host,
        ))
    }
}

/// Acknowledge one previously read pending-ack message and emit the documented
/// reply disposition.
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
    ack_mail_with_runtime_impl(request, observability, runtime, None, None)
}

pub fn ack_mail_with_runtime_and_post_send_emitter(
    request: AckRequest,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
    post_send_emitter: &dyn PostSendHookEmitter,
) -> Result<AckOutcome, AtmError> {
    ack_mail_with_runtime_impl(
        request,
        observability,
        runtime,
        Some(post_send_emitter),
        None,
    )
}

pub fn ack_mail_with_runtime_and_reply_delivery(
    request: AckRequest,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
    post_send_emitter: &dyn PostSendHookEmitter,
    ack_reply_delivery: &dyn AckReplyDeliveryPort,
) -> Result<AckOutcome, AtmError> {
    ack_mail_with_runtime_impl(
        request,
        observability,
        runtime,
        Some(post_send_emitter),
        Some(ack_reply_delivery),
    )
}

fn ack_mail_with_runtime_impl<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    request: AckRequest,
    observability: &dyn ObservabilityPort,
    runtime: &R,
    post_send_emitter: Option<&dyn PostSendHookEmitter>,
    ack_reply_delivery: Option<&dyn AckReplyDeliveryPort>,
) -> Result<AckOutcome, AtmError> {
    let actor = request.caller_identity.clone();
    let team = request.caller_team.clone();
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
        actor,
        team,
        ack_reply_delivery,
    )
    .and_then(|context| finalize_ack_outcome(runtime, observability, post_send_emitter, context))
}

fn ack_mail_with_runtime_sqlite<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    request: AckRequest,
    _observability: &dyn ObservabilityPort,
    runtime: &R,
    actor: AgentName,
    team: TeamName,
    ack_reply_delivery: Option<&dyn AckReplyDeliveryPort>,
) -> Result<FinalizeAckContextOwned, AtmError> {
    let (post_send_config, warnings) =
        match crate::send::hook::load_post_send_config_for_sender(runtime, &team, &actor) {
            Ok(config) => (config, Vec::new()),
            Err(error) => (
                None,
                vec![crate::send::WarningEntry::with_code(
                    error.code,
                    format!(
                        "warning: post-send hook config lookup failed for {}@{}: {}.",
                        actor, team, error.message
                    ),
                    error.primary_recovery().map(str::to_owned),
                )],
            ),
        };
    let source = load_ack_source(
        runtime,
        &request.home_dir,
        &team,
        &actor,
        request.message_id,
    )?;
    let persisted = persist_ack_reply(
        runtime,
        AckPersistenceContext {
            request: &request,
            actor: &actor,
            team: &team,
            source: &source,
        },
        ack_reply_delivery,
    )?;
    Ok(FinalizeAckContextOwned {
        actor,
        team,
        request_message_id: request.message_id,
        persisted,
        post_send_config,
        warnings,
    })
}

#[derive(Clone)]
struct LoadedAckSource {
    row: boundary::MailStoreMailboxMetadataRow,
}

enum PersistedAckReply {
    SuppressedSelfAck {
        reply_text: String,
        task_id: Option<TaskId>,
    },
    Sent(SentAckReply),
    SentRemote(SentRemoteAckReply),
}

struct SentAckReply {
    reply_target: ReplyTarget,
    reply_snapshot: crate::delivery_policy::DeliveryRecipientSnapshot,
    reply_message_id: AtmMessageId,
    reply_text: String,
    task_id: Option<TaskId>,
    reply_inbox_path: PathBuf,
    persistence: Box<crate::send::DeliveryPersistenceResult>,
}

/// Ack reply that was routed back to the original remote sender's host via
/// [`AckReplyDeliveryPort`] instead of being persisted into a local mailbox.
struct SentRemoteAckReply {
    reply_target: ReplyTarget,
    reply_message_id: AtmMessageId,
    reply_text: String,
    task_id: Option<TaskId>,
}

struct FinalizeAckContext<'a> {
    actor: &'a AgentName,
    team: &'a TeamName,
    request_message_id: AtmMessageId,
    persisted: &'a PersistedAckReply,
    post_send_config: Option<crate::config::AtmConfig>,
    warnings: Vec<crate::send::WarningEntry>,
}

struct FinalizeAckContextOwned {
    actor: AgentName,
    team: TeamName,
    request_message_id: AtmMessageId,
    persisted: PersistedAckReply,
    post_send_config: Option<crate::config::AtmConfig>,
    warnings: Vec<crate::send::WarningEntry>,
}

struct AckPersistenceContext<'a> {
    request: &'a AckRequest,
    actor: &'a AgentName,
    team: &'a TeamName,
    source: &'a LoadedAckSource,
}

struct PersistedSourceAck {
    reply_target: ReplyTarget,
    task_id: Option<TaskId>,
    suppressed_self_ack: bool,
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
    source_record: &boundary::Message,
    current_team: &TeamName,
) -> Result<ReplyTarget, AtmError> {
    let (reply_agent, reply_team, reply_host) =
        resolve_reply_target(&source_record.envelope, current_team)?;
    if reply_host.is_none() {
        // Cross-host reply targets are validated against the remote host's
        // roster when the reply is routed there; the local roster only knows
        // about same-host agents.
        ensure_roster_member_exists(
            runtime,
            &reply_team,
            &reply_agent,
            "Repair or reload the ATM roster before retrying the acknowledgement reply.",
        )?;
    }

    Ok(ReplyTarget::new(reply_agent, reply_team, reply_host))
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
    ack_reply_delivery: Option<&dyn AckReplyDeliveryPort>,
) -> Result<PersistedAckReply, AtmError> {
    let ack_timestamp = IsoTimestamp::now();
    let ack_intent = AckIntentFields::not_required();
    let reply_text = input::validate_message_text(context.request.reply_body.clone())?;
    let persisted_source = persist_source_ack_state(runtime, &context, ack_timestamp)?;

    if persisted_source.suppressed_self_ack {
        return Ok(PersistedAckReply::SuppressedSelfAck {
            reply_text,
            task_id: persisted_source.task_id,
        });
    }

    persist_sent_ack_reply(
        runtime,
        &context,
        persisted_source,
        ack_timestamp,
        reply_text,
        ack_intent,
        ack_reply_delivery,
    )
}

fn persist_source_ack_state<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    context: &AckPersistenceContext<'_>,
    ack_timestamp: IsoTimestamp,
) -> Result<PersistedSourceAck, AtmError> {
    let source_record = load_ack_source_record(
        runtime,
        home_dir(context.request),
        context.team,
        context.actor,
        &context.source.row,
    )?;
    ensure_ack_is_pending(context.request.message_id, &source_record.envelope)?;
    let reply_target = validate_reply_target(runtime, &source_record, context.team)?;

    let mut projected_envelope = source_record.envelope.clone();
    projected_envelope.read = true;
    projected_envelope.pending_ack_at = None;
    projected_envelope.acknowledged_at = Some(ack_timestamp);

    runtime.persist_message_state(boundary::MailMessageState {
        team: context.team.clone(),
        agent: context.actor.clone(),
        actor: context.actor.clone(),
        message_key: context.source.row.message_key.clone(),
        read: true,
        pending_ack_at: None,
        acknowledged_at: Some(ack_timestamp),
        expires_at: source_record.envelope.expires_at,
        deleted_at: None,
        updated_at: Some(ack_timestamp),
    })?;

    Ok(PersistedSourceAck {
        task_id: source_record.envelope.task_id.clone(),
        suppressed_self_ack: is_self_ack_reply_target(context.actor, context.team, &reply_target),
        reply_target,
    })
}

fn persist_sent_ack_reply<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    context: &AckPersistenceContext<'_>,
    persisted_source: PersistedSourceAck,
    ack_timestamp: IsoTimestamp,
    reply_text: String,
    ack_intent: AckIntentFields,
    ack_reply_delivery: Option<&dyn AckReplyDeliveryPort>,
) -> Result<PersistedAckReply, AtmError> {
    if let Some(host) = persisted_source.reply_target.host.clone() {
        return persist_remote_sent_ack_reply(
            context,
            persisted_source,
            reply_text,
            ack_intent,
            host,
            ack_reply_delivery,
        );
    }

    let reply_message_id = AtmMessageId::new();
    let reply_message = InboxMessage {
        from: context.actor.clone(),
        text: reply_text.clone(),
        timestamp: ack_timestamp,
        read: false,
        source_team: Some(context.team.clone()),
        summary: Some(summary::build_summary(&reply_text, None)),
        message_id: Some(reply_message_id),
        requires_ack: ack_intent.requires_ack,
        pending_ack_at: ack_intent.pending_ack_at,
        acknowledged_at: ack_intent.acknowledged_at,
        acknowledges_message_id: Some(context.request.message_id),
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        task_id: None,
        origin_host: None,
        extra: Map::new(),
    };
    let reply_inbox_path = runtime.inbox_path(
        home_dir(context.request),
        &persisted_source.reply_target.team,
        &persisted_source.reply_target.agent,
    )?;
    let reply_snapshot = DeliveryPolicyCoordinator::new().resolve_recipient_snapshot(
        runtime,
        &persisted_source.reply_target.team,
        &persisted_source.reply_target.agent,
    )?;
    let persistence = persist_message(
        runtime,
        home_dir(context.request),
        &reply_snapshot,
        &reply_inbox_path,
        &reply_message,
        false,
    )?;

    Ok(PersistedAckReply::Sent(SentAckReply {
        reply_target: persisted_source.reply_target,
        reply_snapshot,
        reply_message_id,
        reply_text,
        task_id: persisted_source.task_id,
        reply_inbox_path,
        persistence: Box::new(persistence),
    }))
}

/// Route an ack reply back to the original remote sender's host instead of
/// persisting into a local mailbox. The local source-message ack state has
/// already been persisted by [`persist_source_ack_state`] before this is
/// called.
fn persist_remote_sent_ack_reply(
    context: &AckPersistenceContext<'_>,
    persisted_source: PersistedSourceAck,
    reply_text: String,
    ack_intent: AckIntentFields,
    host: RemoteTargetHost,
    ack_reply_delivery: Option<&dyn AckReplyDeliveryPort>,
) -> Result<PersistedAckReply, AtmError> {
    let port = ack_reply_delivery.ok_or_else(|| {
        AtmError::daemon_unavailable(
            "cross-host acknowledgement reply delivery is not available in this runtime context",
        )
        .with_recovery(
            "Retry `atm ack` through atm-daemon so the acknowledgement reply can be routed to the remote sender's host.",
        )
    })?;
    let reply_message_id = AtmMessageId::new();
    let to = format!(
        "{}@{}",
        persisted_source.reply_target.agent, persisted_source.reply_target.team
    )
    .parse()?;
    let reply_request = SendRequest {
        home_dir: home_dir(context.request).to_path_buf(),
        current_dir: home_dir(context.request).to_path_buf(),
        caller_identity: context.actor.clone(),
        caller_team: context.team.clone(),
        to,
        message_source: SendMessageSource::Inline(reply_text.clone()),
        summary_override: None,
        requires_ack: ack_intent.requires_ack,
        task_id: None,
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        remote_host: Some(host),
        origin_host: None,
        acknowledges_message_id: Some(context.request.message_id),
        dry_run: false,
    };
    port.deliver_reply(reply_request)?;

    Ok(PersistedAckReply::SentRemote(SentRemoteAckReply {
        reply_target: persisted_source.reply_target,
        reply_message_id,
        reply_text,
        task_id: persisted_source.task_id,
    }))
}

fn home_dir(request: &AckRequest) -> &std::path::Path {
    request.home_dir.as_path()
}

fn finalize_ack_outcome<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    runtime: &R,
    observability: &dyn ObservabilityPort,
    post_send_emitter: Option<&dyn PostSendHookEmitter>,
    owned: FinalizeAckContextOwned,
) -> Result<AckOutcome, AtmError> {
    let context = FinalizeAckContext {
        actor: &owned.actor,
        team: &owned.team,
        request_message_id: owned.request_message_id,
        persisted: &owned.persisted,
        post_send_config: owned.post_send_config,
        warnings: owned.warnings,
    };
    match context.persisted {
        PersistedAckReply::SuppressedSelfAck {
            reply_text,
            task_id,
        } => Ok(finalize_suppressed_self_ack_outcome(
            &context,
            observability,
            reply_text,
            task_id,
        )),
        PersistedAckReply::Sent(reply) => {
            finalize_sent_ack_outcome(runtime, observability, post_send_emitter, &context, reply)
        }
        PersistedAckReply::SentRemote(reply) => Ok(finalize_sent_remote_ack_outcome(
            &context,
            observability,
            reply,
        )),
    }
}

fn finalize_suppressed_self_ack_outcome(
    context: &FinalizeAckContext<'_>,
    observability: &dyn ObservabilityPort,
    reply_text: &str,
    task_id: &Option<TaskId>,
) -> AckOutcome {
    let outcome = AckOutcome {
        action: CommandAction::Ack,
        team: context.team.clone(),
        agent: context.actor.clone(),
        message_id: context.request_message_id,
        task_id: task_id.clone(),
        reply_disposition: AckReplyDisposition::SuppressedSelfAck,
        reply_text: reply_text.to_string(),
        warnings: context.warnings.clone(),
    };
    record_ack_telemetry(
        observability,
        context.actor,
        context.team.clone(),
        context.request_message_id,
        task_id.clone(),
    );
    outcome
}

/// Finalize an ack outcome for a reply that was routed cross-host via
/// [`AckReplyDeliveryPort`]. Local post-send hook emission is intentionally
/// skipped here: the remote daemon's own send/dispatch path is responsible
/// for any nudge or post-send effects once it persists the reply.
fn finalize_sent_remote_ack_outcome(
    context: &FinalizeAckContext<'_>,
    observability: &dyn ObservabilityPort,
    reply: &SentRemoteAckReply,
) -> AckOutcome {
    let outcome = AckOutcome {
        action: CommandAction::Ack,
        team: context.team.clone(),
        agent: context.actor.clone(),
        message_id: context.request_message_id,
        task_id: reply.task_id.clone(),
        reply_disposition: AckReplyDisposition::Sent {
            reply_message_id: reply.reply_message_id,
            reply_target: reply.reply_target.clone(),
        },
        reply_text: reply.reply_text.clone(),
        warnings: context.warnings.clone(),
    };
    record_ack_telemetry(
        observability,
        context.actor,
        context.team.clone(),
        context.request_message_id,
        reply.task_id.clone(),
    );
    outcome
}

fn finalize_sent_ack_outcome<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    runtime: &R,
    observability: &dyn ObservabilityPort,
    post_send_emitter: Option<&dyn PostSendHookEmitter>,
    context: &FinalizeAckContext<'_>,
    reply: &SentAckReply,
) -> Result<AckOutcome, AtmError> {
    let post_send_messages = reply_post_send_messages(reply.persistence.as_ref())?;
    let plan = build_reply_delivery_plan(
        reply.persistence.as_ref(),
        &reply.reply_target,
        &reply.reply_snapshot,
        &reply.reply_inbox_path,
    )?;
    let execution = execute_reply_delivery_plan(runtime, context.post_send_config.as_ref(), &plan)?;
    let mut outcome = AckOutcome {
        action: CommandAction::Ack,
        team: context.team.clone(),
        agent: context.actor.clone(),
        message_id: context.request_message_id,
        task_id: reply.task_id.clone(),
        reply_disposition: AckReplyDisposition::Sent {
            reply_message_id: reply.reply_message_id,
            reply_target: reply.reply_target.clone(),
        },
        reply_text: reply.reply_text.clone(),
        warnings: context.warnings.clone(),
    };
    outcome.warnings.extend(plan.warnings.iter().cloned());
    emit_reply_delivery_plan_transitions(
        observability,
        DeliveryTransitionContext {
            family: DeliveryEventFamily::AckReply,
            team: &reply.reply_target.team,
            agent: &reply.reply_target.agent,
            sender: context.actor,
            message_id: reply.reply_message_id,
            task_id: reply.task_id.clone(),
        },
        &plan,
        &execution,
    )?;
    outcome.warnings.extend(execution.warnings);
    crate::send::hook::emit_post_send_effects(
        runtime,
        &mut outcome.warnings,
        context.post_send_config.as_ref(),
        post_send_emitter,
        &ResolvedRecipient {
            agent: reply.reply_target.agent.clone(),
            team: reply.reply_target.team.clone(),
        },
        &reply.reply_snapshot,
        &post_send_messages,
    );
    record_ack_telemetry(
        observability,
        context.actor,
        context.team.clone(),
        context.request_message_id,
        reply.task_id.clone(),
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
            messages,
            warnings,
        )
    }
}

fn build_reply_delivery_plan(
    persistence: &crate::send::DeliveryPersistenceResult,
    reply_target: &ReplyTarget,
    reply_snapshot: &crate::delivery_policy::DeliveryRecipientSnapshot,
    reply_inbox_path: &std::path::Path,
) -> Result<DeliveryPlan, AtmError> {
    Ok(
        AckReplyStateMachine::from_persistence(persistence)?.into_reply_delivery_plan(
            reply_target,
            reply_snapshot,
            reply_inbox_path,
        ),
    )
}

fn reply_post_send_messages(
    persistence: &crate::send::DeliveryPersistenceResult,
) -> Result<Vec<LogicalMessage>, AtmError> {
    logical_messages_from_persistence(persistence, false, true).map_err(|error| {
        AtmError::mailbox_write(error.to_string()).with_recovery(
            "Repair the persisted reply-delivery record shape before retrying post-send emission.",
        )
    })
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

fn resolve_reply_target(
    message: &InboxMessage,
    current_team: &TeamName,
) -> Result<(AgentName, TeamName, Option<RemoteTargetHost>), AtmError> {
    let identity = canonical_sender_identity(message);
    let team = message
        .source_team
        .clone()
        .unwrap_or_else(|| current_team.clone());
    let host = message
        .origin_host
        .as_deref()
        .map(RemoteTargetHost::parse)
        .transpose()?;
    Ok((identity, team, host))
}

fn canonical_sender_identity(message: &InboxMessage) -> AgentName {
    crate::threading::canonical_sender_identity(message)
}

fn is_self_ack_reply_target(
    actor: &AgentName,
    team: &TeamName,
    reply_target: &ReplyTarget,
) -> bool {
    actor
        .as_str()
        .eq_ignore_ascii_case(reply_target.agent.as_str())
        && team
            .as_str()
            .eq_ignore_ascii_case(reply_target.team.as_str())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use super::{
        AckReplyDisposition, AckReplyStateMachine, FinalizeAckContextOwned, PersistedAckReply,
        ReplyTarget, SentAckReply, SentRemoteAckReply, canonical_sender_identity,
        finalize_ack_outcome, resolve_reply_target,
    };
    use crate::boundary::{
        self, AckReplyDeliveryPort, BuiltInPostSendDispatch, MessageKey,
        NonClaudeOutboundDeliveryRequest, PostSendBuiltInTarget, PostSendEmissionPath,
        PostSendHookEmitter,
    };
    use crate::delivery_plan::{DeliveryPlanDisposition, DeliveryTarget};
    use crate::error::AtmErrorKind;
    use crate::error_codes::AtmErrorCode;
    use crate::observability::NullObservability;
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::schema::{AckIntentFields, AtmMessageId, InboxMessage};
    use crate::send::{
        DeliveryPersistenceDisposition, DeliveryPersistenceResult, RemoteTargetHost, SendRequest,
        WarningEntry,
    };
    use crate::service_runtime::RetainedServiceRuntime;
    use crate::service_runtime_store::RetainedMailboxRuntime;
    use crate::test_support::{EnvGuard, TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, IsoTimestamp, TeamName};
    use serde_json::Map;

    struct AckRuntime {
        outbound_deliveries: Mutex<Vec<NonClaudeOutboundDeliveryRequest>>,
    }

    #[derive(Default)]
    struct RecordingPostSendEmitter {
        fail_code: Option<AtmErrorCode>,
        emitted: Mutex<Vec<BuiltInPostSendDispatch>>,
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

    impl RecordingPostSendEmitter {
        fn succeed() -> Self {
            Self::default()
        }

        fn fail(code: AtmErrorCode) -> Self {
            Self {
                fail_code: Some(code),
                emitted: Mutex::new(Vec::new()),
            }
        }

        fn emitted(&self) -> Vec<BuiltInPostSendDispatch> {
            self.emitted.lock().expect("post-send emitter").clone()
        }
    }

    struct AckRosterRuntime {
        roster_members: Vec<(TeamName, AgentName)>,
        inbox_path: PathBuf,
        source_row: boundary::MailStoreMailboxMetadataRow,
        source_record: boundary::Message,
        outbound_deliveries: Mutex<Vec<NonClaudeOutboundDeliveryRequest>>,
        persisted_states: Mutex<Vec<boundary::MailMessageState>>,
    }

    struct AckReloadRuntime {
        roster_members: Vec<(TeamName, AgentName)>,
        source_row: boundary::MailStoreMailboxMetadataRow,
        source_records: Mutex<VecDeque<boundary::Message>>,
        persisted_states: Mutex<Vec<boundary::MailMessageState>>,
    }

    impl crate::boundary::sealed::Sealed for AckRuntime {}
    impl crate::boundary::sealed::Sealed for RecordingPostSendEmitter {}

    impl crate::boundary::sealed::Sealed for AckRosterRuntime {}
    impl crate::boundary::sealed::Sealed for AckReloadRuntime {}

    impl PostSendHookEmitter for RecordingPostSendEmitter {
        fn emit_post_send(
            &self,
            dispatch: &BuiltInPostSendDispatch,
        ) -> Result<PostSendEmissionPath, crate::error::AtmError> {
            self.emitted
                .lock()
                .expect("post-send emitter")
                .push(dispatch.clone());
            if let Some(code) = self.fail_code {
                return Err(crate::error::AtmError::new_with_code(
                    code,
                    AtmErrorKind::DaemonUnavailable,
                    "test ack post-send emitter failure",
                )
                .with_recovery("Repair the test ack post-send emitter and retry."));
            }
            Ok(match dispatch.target {
                PostSendBuiltInTarget::LocalTmux(_) => PostSendEmissionPath::LocalTmux,
                PostSendBuiltInTarget::Graft(_) => PostSendEmissionPath::GraftPort,
            })
        }
    }

    impl RetainedServiceRuntime for AckRuntime {
        fn load_config(
            &self,
            _current_dir: &Path,
        ) -> Result<Option<crate::config::AtmConfig>, crate::error::AtmError> {
            Ok(None)
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
            state: boundary::MailMessageState,
        ) -> Result<(), crate::error::AtmError> {
            self.persisted_states
                .lock()
                .expect("persisted states")
                .push(state);
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

    impl RetainedServiceRuntime for AckReloadRuntime {
        fn load_config(
            &self,
            _current_dir: &Path,
        ) -> Result<Option<crate::config::AtmConfig>, crate::error::AtmError> {
            unreachable!("ack reload test does not load config")
        }

        fn inbox_path(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<PathBuf, crate::error::AtmError> {
            unreachable!("ack reload test should stop before reply inbox resolution")
        }

        fn load_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<Option<IsoTimestamp>, crate::error::AtmError> {
            unreachable!("ack reload test does not load seen watermark")
        }

        fn save_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _timestamp: IsoTimestamp,
        ) -> Result<(), crate::error::AtmError> {
            unreachable!("ack reload test does not save seen watermark")
        }

        fn deliver_non_claude_payloads(
            &self,
            _recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
            _messages: &[InboxMessage],
        ) -> Result<(), crate::error::AtmError> {
            unreachable!("ack reload test does not deliver outbound payloads")
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
    }

    impl RetainedMailboxRuntime for AckReloadRuntime {
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
            if message_key != &self.source_row.message_key {
                return Ok(None);
            }
            Ok(self
                .source_records
                .lock()
                .expect("source records")
                .pop_front())
        }

        fn persist_message_record(
            &self,
            _record: boundary::Message,
        ) -> Result<(), crate::error::AtmError> {
            unreachable!("ack reload test does not persist reply records")
        }

        fn persist_message_state(
            &self,
            state: boundary::MailMessageState,
        ) -> Result<(), crate::error::AtmError> {
            self.persisted_states
                .lock()
                .expect("persisted states")
                .push(state);
            Ok(())
        }
    }

    fn message_with_from(from: &str) -> InboxMessage {
        let ack_intent = AckIntentFields::not_required();
        InboxMessage {
            from: from.parse::<AgentName>().expect("agent"),
            text: "hello".to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(TEST_TEAM.parse::<TeamName>().expect("team")),
            summary: None,
            message_id: None,
            requires_ack: ack_intent.requires_ack,
            pending_ack_at: ack_intent.pending_ack_at,
            acknowledged_at: ack_intent.acknowledged_at,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            origin_host: None,
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

        let target = resolve_reply_target(&message, &TeamName::from_validated(TEST_TEAM))
            .expect("resolve reply target");
        assert_eq!(
            target,
            (
                ROLE_TEAM_LEAD.parse().expect("agent"),
                TEST_TEAM.parse().expect("team"),
                None,
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
            local_tmux_post_send: false,
            graft_post_send: false,
            roster_backed: true,
        };
        let machine = AckReplyStateMachine::from_persistence(&persistence).expect("state machine");
        let plan = machine.into_reply_delivery_plan(
            &super::ReplyTarget::new(agent.clone(), team.clone(), None),
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
        assert_eq!(plan.warnings.len(), 1);
        assert!(matches!(
            plan.delivery_target,
            DeliveryTarget::NonClaude { .. }
        ));
    }

    #[test]
    fn ack_write_goes_through_non_claude_outbound_boundary() {
        let team = TEST_TEAM.parse::<TeamName>().expect("team");
        let agent = ROLE_TEAM_LEAD.parse::<AgentName>().expect("agent");
        let reply_message_id = AtmMessageId::new();
        let request_message_id = AtmMessageId::new();
        let reply_text = "ack reply".to_string();
        let ack_intent = AckIntentFields::not_required();
        let reply_message = InboxMessage {
            from: "sender".parse::<AgentName>().expect("agent"),
            text: reply_text.clone(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(team.clone()),
            summary: None,
            message_id: Some(reply_message_id),
            requires_ack: ack_intent.requires_ack,
            pending_ack_at: ack_intent.pending_ack_at,
            acknowledged_at: ack_intent.acknowledged_at,
            acknowledges_message_id: Some(request_message_id),
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            origin_host: None,
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
            local_tmux_post_send: false,
            graft_post_send: false,
            roster_backed: true,
        };
        let reply_target = ReplyTarget::new(agent.clone(), team.clone(), None);
        let persisted = PersistedAckReply::Sent(SentAckReply {
            reply_target: reply_target.clone(),
            reply_snapshot,
            reply_message_id,
            reply_text: reply_text.clone(),
            task_id: None,
            reply_inbox_path: PathBuf::from("reply.jsonl"),
            persistence: Box::new(persistence),
        });
        let runtime = AckRuntime {
            outbound_deliveries: Mutex::new(Vec::new()),
        };

        let outcome = finalize_ack_outcome(
            &runtime,
            &NullObservability,
            None,
            FinalizeAckContextOwned {
                actor: "sender".parse::<AgentName>().expect("agent"),
                team: team.clone(),
                request_message_id,
                persisted,
                post_send_config: None,
                warnings: Vec::new(),
            },
        )
        .expect("finalize ack outcome");

        let outbound_messages = runtime.outbound_messages();
        assert_eq!(outbound_messages.len(), 1);
        assert_eq!(outbound_messages[0], reply_message);
        assert!(matches!(
            outcome.reply_disposition,
            AckReplyDisposition::Sent {
                reply_message_id: sent_id,
                ..
            } if sent_id == reply_message_id
        ));
        assert_eq!(outcome.reply_text, reply_text);
    }

    #[test]
    fn finalize_ack_outcome_for_remote_reply_skips_local_outbound_delivery() {
        let team = TEST_TEAM.parse::<TeamName>().expect("team");
        let agent = ROLE_TEAM_LEAD.parse::<AgentName>().expect("agent");
        let reply_message_id = AtmMessageId::new();
        let request_message_id = AtmMessageId::new();
        let reply_text = "ack reply".to_string();
        let host = RemoteTargetHost::parse("other-mac.local").expect("host");
        let reply_target = ReplyTarget::new(agent.clone(), team.clone(), Some(host));
        let persisted = PersistedAckReply::SentRemote(SentRemoteAckReply {
            reply_target: reply_target.clone(),
            reply_message_id,
            reply_text: reply_text.clone(),
            task_id: None,
        });
        let runtime = AckRuntime {
            outbound_deliveries: Mutex::new(Vec::new()),
        };

        let outcome = finalize_ack_outcome(
            &runtime,
            &NullObservability,
            None,
            FinalizeAckContextOwned {
                actor: "sender".parse::<AgentName>().expect("agent"),
                team: team.clone(),
                request_message_id,
                persisted,
                post_send_config: None,
                warnings: Vec::new(),
            },
        )
        .expect("finalize ack outcome");

        assert!(runtime.outbound_messages().is_empty());
        assert!(matches!(
            outcome.reply_disposition,
            AckReplyDisposition::Sent {
                reply_message_id: sent_id,
                ..
            } if sent_id == reply_message_id
        ));
        assert_eq!(outcome.reply_text, reply_text);
        assert_eq!(
            reply_target.to_string(),
            format!("{agent}@{team}.other-mac.local")
        );
    }

    #[test]
    fn finalize_ack_outcome_suppresses_self_ack_without_outbound_delivery() {
        let team = TEST_TEAM.parse::<TeamName>().expect("team");
        let request_message_id = AtmMessageId::new();
        let runtime = AckRuntime {
            outbound_deliveries: Mutex::new(Vec::new()),
        };

        let outcome = finalize_ack_outcome(
            &runtime,
            &NullObservability,
            None,
            FinalizeAckContextOwned {
                actor: TEST_SENDER.parse::<AgentName>().expect("agent"),
                team: team.clone(),
                request_message_id,
                persisted: PersistedAckReply::SuppressedSelfAck {
                    reply_text: "already on it".to_string(),
                    task_id: None,
                },
                post_send_config: None,
                warnings: Vec::new(),
            },
        )
        .expect("suppressed self-ack outcome");

        assert!(matches!(
            outcome.reply_disposition,
            AckReplyDisposition::SuppressedSelfAck
        ));
        assert_eq!(outcome.reply_text, "already on it");
        assert!(runtime.outbound_messages().is_empty());
    }

    #[test]
    #[serial_test::serial(env)]
    fn ack_reply_graft_post_send_dispatches_to_graft_port() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let home_dir = tempdir.path().join("home");
        let team = TEST_TEAM.parse::<TeamName>().expect("team");
        let agent = ROLE_TEAM_LEAD.parse::<AgentName>().expect("agent");
        let reply_message_id = AtmMessageId::new();
        let request_message_id = AtmMessageId::new();
        let reply_text = "ack reply".to_string();
        let ack_intent = AckIntentFields::not_required();
        let reply_message = InboxMessage {
            from: "sender".parse::<AgentName>().expect("agent"),
            text: reply_text.clone(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(team.clone()),
            summary: None,
            message_id: Some(reply_message_id),
            requires_ack: ack_intent.requires_ack,
            pending_ack_at: ack_intent.pending_ack_at,
            acknowledged_at: ack_intent.acknowledged_at,
            acknowledges_message_id: Some(request_message_id),
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            origin_host: None,
            extra: serde_json::Map::new(),
        };
        let persistence = DeliveryPersistenceResult {
            disposition: DeliveryPersistenceDisposition::Persisted,
            original_message: reply_message,
            companion_message: None,
            warnings: Vec::new(),
        };
        let reply_snapshot = crate::delivery_policy::DeliveryRecipientSnapshot {
            agent: agent.clone(),
            team: team.clone(),
            harness: crate::delivery_policy::DeliveryHarnessPath::NonClaude,
            recipient_pane_id: None,
            local_tmux_post_send: false,
            graft_post_send: true,
            roster_backed: true,
        };
        let reply_target = ReplyTarget::new(agent.clone(), team.clone(), None);
        let persisted = PersistedAckReply::Sent(SentAckReply {
            reply_target: reply_target.clone(),
            reply_snapshot,
            reply_message_id,
            reply_text: reply_text.clone(),
            task_id: None,
            reply_inbox_path: tempdir.path().join("reply.jsonl"),
            persistence: Box::new(persistence),
        });
        let runtime = AckRuntime {
            outbound_deliveries: Mutex::new(Vec::new()),
        };
        let post_send_emitter = RecordingPostSendEmitter::succeed();
        let _env = EnvGuard::set_many([
            ("HOME", Some(home_dir.to_str().expect("utf8 home"))),
            ("USERPROFILE", None),
            ("ATM_LOG_DIR", None),
        ]);

        let outcome = finalize_ack_outcome(
            &runtime,
            &NullObservability,
            Some(&post_send_emitter),
            FinalizeAckContextOwned {
                actor: "sender".parse::<AgentName>().expect("agent"),
                team,
                request_message_id,
                persisted,
                post_send_config: None,
                warnings: Vec::new(),
            },
        )
        .expect("finalize ack outcome");

        assert!(matches!(
            outcome.reply_disposition,
            AckReplyDisposition::Sent {
                reply_message_id: sent_id,
                ..
            } if sent_id == reply_message_id
        ));
        assert!(outcome.warnings.is_empty());
        let emitted = post_send_emitter.emitted();
        assert_eq!(emitted.len(), 1);
        assert!(emitted[0].event.is_ack);
        assert_eq!(emitted[0].event.recipient.as_str(), ROLE_TEAM_LEAD);
        assert!(matches!(emitted[0].target, PostSendBuiltInTarget::Graft(_)));
    }

    #[test]
    #[serial_test::serial(env)]
    fn finalize_ack_outcome_warns_when_graft_post_send_delivery_fails() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let home_dir = tempdir.path().join("home");
        let team = TEST_TEAM.parse::<TeamName>().expect("team");
        let agent = ROLE_TEAM_LEAD.parse::<AgentName>().expect("agent");
        let reply_message_id = AtmMessageId::new();
        let request_message_id = AtmMessageId::new();
        let reply_text = "ack reply".to_string();
        let ack_intent = AckIntentFields::not_required();
        let reply_message = InboxMessage {
            from: "sender".parse::<AgentName>().expect("agent"),
            text: reply_text.clone(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(team.clone()),
            summary: None,
            message_id: Some(reply_message_id),
            requires_ack: ack_intent.requires_ack,
            pending_ack_at: ack_intent.pending_ack_at,
            acknowledged_at: ack_intent.acknowledged_at,
            acknowledges_message_id: Some(request_message_id),
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            origin_host: None,
            extra: serde_json::Map::new(),
        };
        let persistence = DeliveryPersistenceResult {
            disposition: DeliveryPersistenceDisposition::Persisted,
            original_message: reply_message,
            companion_message: None,
            warnings: Vec::new(),
        };
        let reply_snapshot = crate::delivery_policy::DeliveryRecipientSnapshot {
            agent: agent.clone(),
            team: team.clone(),
            harness: crate::delivery_policy::DeliveryHarnessPath::NonClaude,
            recipient_pane_id: None,
            local_tmux_post_send: false,
            graft_post_send: true,
            roster_backed: true,
        };
        let reply_target = ReplyTarget::new(agent, team.clone(), None);
        let persisted = PersistedAckReply::Sent(SentAckReply {
            reply_target: reply_target.clone(),
            reply_snapshot,
            reply_message_id,
            reply_text: reply_text.clone(),
            task_id: None,
            reply_inbox_path: tempdir.path().join("reply.jsonl"),
            persistence: Box::new(persistence),
        });
        let runtime = AckRuntime {
            outbound_deliveries: Mutex::new(Vec::new()),
        };
        let post_send_emitter =
            RecordingPostSendEmitter::fail(AtmErrorCode::PostSendGraftUnavailable);
        let _env = EnvGuard::set_many([
            ("HOME", Some(home_dir.to_str().expect("utf8 home"))),
            ("USERPROFILE", None),
            ("ATM_LOG_DIR", None),
        ]);

        let outcome = finalize_ack_outcome(
            &runtime,
            &NullObservability,
            Some(&post_send_emitter),
            FinalizeAckContextOwned {
                actor: "sender".parse::<AgentName>().expect("agent"),
                team,
                request_message_id,
                persisted,
                post_send_config: None,
                warnings: Vec::new(),
            },
        )
        .expect("finalize ack outcome");

        assert!(matches!(
            outcome.reply_disposition,
            AckReplyDisposition::Sent {
                reply_message_id: sent_id,
                ..
            } if sent_id == reply_message_id
        ));
        assert_eq!(outcome.warnings.len(), 1);
        assert!(
            outcome.warnings[0]
                .message
                .contains("warning: post-send emission failed")
        );
    }

    #[test]
    fn ack_mail_rejects_actor_missing_from_atm_roster() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let runtime = AckRosterRuntime {
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
                requires_ack: true,
                pending_ack: true,
                acknowledged_at: None,
                expires_at: None,
                task_id: None,
            },
            source_record: boundary::Message {
                team: TEST_TEAM.parse().expect("team"),
                agent: TEST_SENDER.parse().expect("agent"),
                message_key: MessageKey::new("atm:source").expect("message key"),
                envelope: {
                    let ack_intent = AckIntentFields::required_pending(IsoTimestamp::now());
                    InboxMessage {
                        from: TEST_SENDER.parse().expect("agent"),
                        text: "source".to_string(),
                        timestamp: IsoTimestamp::now(),
                        read: false,
                        source_team: Some(TEST_TEAM.parse().expect("team")),
                        summary: Some("summary".to_string()),
                        message_id: Some(AtmMessageId::new()),
                        requires_ack: ack_intent.requires_ack,
                        pending_ack_at: ack_intent.pending_ack_at,
                        acknowledged_at: ack_intent.acknowledged_at,
                        acknowledges_message_id: None,
                        parent_message_id: None,
                        thread_mode: None,
                        expires_at: None,
                        task_id: None,
                        origin_host: None,
                        extra: Map::new(),
                    }
                },
            },
            outbound_deliveries: Mutex::new(Vec::new()),
            persisted_states: Mutex::new(Vec::new()),
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
            None,
            None,
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
        let source_key = MessageKey::from(source_message_id);
        let runtime = AckRosterRuntime {
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
                requires_ack: true,
                pending_ack: true,
                acknowledged_at: None,
                expires_at: None,
                task_id: None,
            },
            source_record: boundary::Message {
                team: team.clone(),
                agent: actor.clone(),
                message_key: source_key,
                envelope: {
                    let ack_intent = AckIntentFields::required_pending(IsoTimestamp::now());
                    InboxMessage {
                        from: actor.clone(),
                        text: "source".to_string(),
                        timestamp: IsoTimestamp::now(),
                        read: false,
                        source_team: Some(team.clone()),
                        summary: Some("summary".to_string()),
                        message_id: Some(source_message_id),
                        requires_ack: ack_intent.requires_ack,
                        pending_ack_at: ack_intent.pending_ack_at,
                        acknowledged_at: ack_intent.acknowledged_at,
                        acknowledges_message_id: None,
                        parent_message_id: None,
                        thread_mode: None,
                        expires_at: None,
                        task_id: None,
                        origin_host: None,
                        extra: Map::new(),
                    }
                },
            },
            outbound_deliveries: Mutex::new(Vec::new()),
            persisted_states: Mutex::new(Vec::new()),
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
            None,
            None,
        )
        .expect("valid ATM roster member should ack successfully");

        assert_eq!(outcome.team, team);
        assert_eq!(outcome.agent, actor);
        assert_eq!(outcome.message_id, source_message_id);
        assert!(matches!(
            outcome.reply_disposition,
            AckReplyDisposition::SuppressedSelfAck
        ));
        assert!(
            runtime
                .outbound_deliveries
                .lock()
                .expect("non-claude deliveries")
                .is_empty()
        );
    }

    struct RecordingAckReplyDeliveryPort {
        delivered: Mutex<Vec<SendRequest>>,
    }

    impl crate::boundary::sealed::Sealed for RecordingAckReplyDeliveryPort {}

    impl AckReplyDeliveryPort for RecordingAckReplyDeliveryPort {
        fn deliver_reply(&self, reply: SendRequest) -> Result<(), crate::error::AtmError> {
            self.delivered
                .lock()
                .expect("delivered replies")
                .push(reply);
            Ok(())
        }
    }

    #[test]
    fn ack_mail_with_cross_host_origin_routes_reply_through_delivery_port_and_marks_source_acknowledged()
     {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let team = TEST_TEAM.parse::<TeamName>().expect("team");
        let actor = TEST_SENDER.parse::<AgentName>().expect("agent");
        let remote_sender = ROLE_TEAM_LEAD.parse::<AgentName>().expect("agent");
        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        std::fs::create_dir_all(team_dir.join("inboxes")).expect("team inbox dir");
        let source_message_id = AtmMessageId::new();
        let source_key = MessageKey::from(source_message_id);
        let origin_host = "other-mac.local".to_string();
        let runtime = AckRosterRuntime {
            roster_members: vec![(team.clone(), actor.clone())],
            inbox_path: tempdir.path().join("reply.jsonl"),
            source_row: boundary::MailStoreMailboxMetadataRow {
                message_key: source_key.clone(),
                message_id: Some(source_message_id),
                parent_message_id: None,
                thread_mode: None,
                from_agent: remote_sender.clone(),
                summary: Some("summary".to_string()),
                message_at: IsoTimestamp::now(),
                read: false,
                requires_ack: true,
                pending_ack: true,
                acknowledged_at: None,
                expires_at: None,
                task_id: None,
            },
            source_record: boundary::Message {
                team: team.clone(),
                agent: actor.clone(),
                message_key: source_key,
                envelope: {
                    let ack_intent = AckIntentFields::required_pending(IsoTimestamp::now());
                    InboxMessage {
                        from: remote_sender.clone(),
                        text: "source".to_string(),
                        timestamp: IsoTimestamp::now(),
                        read: false,
                        source_team: Some(team.clone()),
                        summary: Some("summary".to_string()),
                        message_id: Some(source_message_id),
                        requires_ack: ack_intent.requires_ack,
                        pending_ack_at: ack_intent.pending_ack_at,
                        acknowledged_at: ack_intent.acknowledged_at,
                        acknowledges_message_id: None,
                        parent_message_id: None,
                        thread_mode: None,
                        expires_at: None,
                        task_id: None,
                        origin_host: Some(origin_host.clone()),
                        extra: Map::new(),
                    }
                },
            },
            outbound_deliveries: Mutex::new(Vec::new()),
            persisted_states: Mutex::new(Vec::new()),
        };
        let ack_reply_delivery = RecordingAckReplyDeliveryPort {
            delivered: Mutex::new(Vec::new()),
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
            None,
            Some(&ack_reply_delivery),
        )
        .expect("cross-host ack reply should route through the delivery port");

        assert_eq!(outcome.team, team);
        assert_eq!(outcome.agent, actor);
        assert_eq!(outcome.message_id, source_message_id);
        assert!(matches!(
            outcome.reply_disposition,
            AckReplyDisposition::Sent { .. }
        ));

        let delivered = ack_reply_delivery
            .delivered
            .lock()
            .expect("delivered replies");
        assert_eq!(delivered.len(), 1);
        let reply = &delivered[0];
        assert_eq!(
            reply.remote_host,
            Some(RemoteTargetHost::parse(&origin_host).expect("host"))
        );
        assert_eq!(reply.acknowledges_message_id, Some(source_message_id));
        assert_eq!(reply.caller_identity, actor);
        assert_eq!(
            reply.to,
            format!("{remote_sender}@{team}").parse().expect("address")
        );
        drop(delivered);

        let persisted_states = runtime.persisted_states.lock().expect("persisted states");
        assert_eq!(persisted_states.len(), 1);
        assert!(persisted_states[0].acknowledged_at.is_some());
        assert!(persisted_states[0].read);
        assert!(
            runtime
                .outbound_deliveries
                .lock()
                .expect("non-claude deliveries")
                .is_empty(),
            "remote ack reply must not persist into a local mailbox"
        );
    }

    #[test]
    fn ack_mail_reloads_source_inside_commit_before_persisting_state() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let team = TEST_TEAM.parse::<TeamName>().expect("team");
        let actor = TEST_SENDER.parse::<AgentName>().expect("agent");
        let team_dir = tempdir.path().join(".claude").join("teams").join(TEST_TEAM);
        std::fs::create_dir_all(&team_dir).expect("team dir");
        let source_message_id = AtmMessageId::new();
        let source_key = MessageKey::from(source_message_id);
        let pending_ack = AckIntentFields::required_pending(IsoTimestamp::now());
        let pending_record = boundary::Message {
            team: team.clone(),
            agent: actor.clone(),
            message_key: source_key.clone(),
            envelope: InboxMessage {
                from: ROLE_TEAM_LEAD.parse().expect("agent"),
                text: "source".to_string(),
                timestamp: IsoTimestamp::now(),
                read: false,
                source_team: Some(team.clone()),
                summary: Some("summary".to_string()),
                message_id: Some(source_message_id),
                requires_ack: pending_ack.requires_ack,
                pending_ack_at: pending_ack.pending_ack_at,
                acknowledged_at: pending_ack.acknowledged_at,
                acknowledges_message_id: None,
                parent_message_id: None,
                thread_mode: None,
                expires_at: None,
                task_id: None,
                origin_host: None,
                extra: Map::new(),
            },
        };
        let acknowledged_record = boundary::Message {
            team: team.clone(),
            agent: actor.clone(),
            message_key: source_key.clone(),
            envelope: InboxMessage {
                from: ROLE_TEAM_LEAD.parse().expect("agent"),
                text: "source".to_string(),
                timestamp: IsoTimestamp::now(),
                read: true,
                source_team: Some(team.clone()),
                summary: Some("summary".to_string()),
                message_id: Some(source_message_id),
                requires_ack: true,
                pending_ack_at: None,
                acknowledged_at: Some(IsoTimestamp::now()),
                acknowledges_message_id: None,
                parent_message_id: None,
                thread_mode: None,
                expires_at: None,
                task_id: None,
                origin_host: None,
                extra: Map::new(),
            },
        };
        let runtime = AckReloadRuntime {
            roster_members: vec![(team.clone(), actor.clone())],
            source_row: boundary::MailStoreMailboxMetadataRow {
                message_key: source_key,
                message_id: Some(source_message_id),
                parent_message_id: None,
                thread_mode: None,
                from_agent: ROLE_TEAM_LEAD.parse().expect("agent"),
                summary: Some("summary".to_string()),
                message_at: IsoTimestamp::now(),
                read: false,
                requires_ack: true,
                pending_ack: true,
                acknowledged_at: None,
                expires_at: None,
                task_id: None,
            },
            source_records: Mutex::new(VecDeque::from(vec![pending_record, acknowledged_record])),
            persisted_states: Mutex::new(Vec::new()),
        };

        let error = super::ack_mail_with_runtime_impl(
            crate::ack::AckRequest {
                home_dir: tempdir.path().to_path_buf(),
                current_dir: tempdir.path().to_path_buf(),
                caller_identity: actor,
                caller_team: team,
                message_id: source_message_id,
                reply_body: "ack".to_string(),
            },
            &NullObservability,
            &runtime,
            None,
            None,
        )
        .expect_err("stale pending metadata should be rejected after commit-time reload");

        assert!(error.message.contains("already acknowledged"), "{error:?}");
        assert!(
            runtime
                .persisted_states
                .lock()
                .expect("persisted states")
                .is_empty(),
            "ack state must not persist when the commit-time reload shows the message already changed"
        );
    }

    #[test]
    fn ack_mail_suppresses_self_ack_when_reply_target_case_differs() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let actor = AgentName::from_validated("Sender-A");
        let team = TeamName::from_validated("Test-Team");
        let team_dir = tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join("Test-Team");
        std::fs::create_dir_all(team_dir.join("inboxes")).expect("team inbox dir");
        let source_message_id = AtmMessageId::new();
        let source_key = MessageKey::from(source_message_id);
        let ack_intent = AckIntentFields::required_pending(IsoTimestamp::now());
        let runtime = AckRosterRuntime {
            roster_members: vec![
                (team.clone(), actor.clone()),
                (
                    TeamName::from_validated(TEST_TEAM),
                    AgentName::from_validated(TEST_SENDER),
                ),
            ],
            inbox_path: tempdir.path().join("reply.jsonl"),
            source_row: boundary::MailStoreMailboxMetadataRow {
                message_key: source_key.clone(),
                message_id: Some(source_message_id),
                parent_message_id: None,
                thread_mode: None,
                from_agent: AgentName::from_validated(TEST_SENDER),
                summary: Some("summary".to_string()),
                message_at: IsoTimestamp::now(),
                read: false,
                requires_ack: true,
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
                    from: AgentName::from_validated(TEST_SENDER),
                    text: "source".to_string(),
                    timestamp: IsoTimestamp::now(),
                    read: false,
                    source_team: Some(TeamName::from_validated(TEST_TEAM)),
                    summary: Some("summary".to_string()),
                    message_id: Some(source_message_id),
                    requires_ack: ack_intent.requires_ack,
                    pending_ack_at: ack_intent.pending_ack_at,
                    acknowledged_at: ack_intent.acknowledged_at,
                    acknowledges_message_id: None,
                    parent_message_id: None,
                    thread_mode: None,
                    expires_at: None,
                    task_id: None,
                    origin_host: None,
                    extra: Map::new(),
                },
            },
            outbound_deliveries: Mutex::new(Vec::new()),
            persisted_states: Mutex::new(Vec::new()),
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
            None,
            None,
        )
        .expect("case-variant self target should suppress reply");

        assert!(matches!(
            outcome.reply_disposition,
            AckReplyDisposition::SuppressedSelfAck
        ));
        assert!(
            runtime
                .outbound_deliveries
                .lock()
                .expect("non-claude deliveries")
                .is_empty()
        );
    }
}
