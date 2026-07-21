use std::path::PathBuf;

use crate::boundary;
use crate::boundary::PostSendHookEmitter;
use crate::error::AtmError;
use crate::observability::{CommandEvent, ObservabilityPort, action_name, outcome_label};
use crate::schema::{AtmMessageId, InboxMessage};
use crate::send::{SendMessageSource, SendRequest};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::{RetainedMailboxRuntime, default_runtime};
use crate::types::{AgentName, CommandAction, IsoTimestamp, TaskId, TeamName};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

impl AckRequest {
    pub fn into_write_request(self) -> SendRequest {
        SendRequest {
            home_dir: self.home_dir,
            current_dir: self.current_dir,
            caller_identity: self.caller_identity,
            caller_team: self.caller_team,
            to: None,
            message_source: SendMessageSource::Inline(self.reply_body),
            summary_override: None,
            requires_ack: false,
            task_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            acknowledges_message_id: Some(self.message_id),
            dry_run: false,
        }
    }

    pub fn from_unresolved_write(request: SendRequest) -> Result<Self, AtmError> {
        let message_id = request.acknowledges_message_id.ok_or_else(|| {
            AtmError::validation("acknowledgement write is missing acknowledges_message_id")
        })?;
        let SendMessageSource::Inline(reply_body) = request.message_source else {
            return Err(AtmError::validation(
                "acknowledgement reply body must be inline",
            ));
        };
        Ok(Self {
            home_dir: request.home_dir,
            current_dir: request.current_dir,
            caller_identity: request.caller_identity,
            caller_team: request.caller_team,
            message_id,
            reply_body,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AckReplyDisposition {
    Sent {
        reply_message_id: AtmMessageId,
        reply_target: ReplyTarget,
    },
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
    ack_mail_with_runtime_impl(request, observability, runtime, None)
}

pub(crate) fn ack_mail_with_runtime_and_post_send_emitter<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    request: AckRequest,
    observability: &dyn ObservabilityPort,
    runtime: &R,
    post_send_emitter: &dyn PostSendHookEmitter,
) -> Result<AckOutcome, AtmError> {
    ack_mail_with_runtime_impl(request, observability, runtime, Some(post_send_emitter))
}

fn ack_mail_with_runtime_impl<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    request: AckRequest,
    observability: &dyn ObservabilityPort,
    runtime: &R,
    post_send_emitter: Option<&dyn PostSendHookEmitter>,
) -> Result<AckOutcome, AtmError> {
    let actor = request.caller_identity.clone();
    let team = request.caller_team.clone();
    ensure_roster_member_exists(
        runtime,
        &team,
        &actor,
        "Repair or reload the ATM roster before retrying `atm ack`.",
    )?;
    acknowledge_via_canonical_write(
        request,
        observability,
        runtime,
        post_send_emitter,
        actor,
        team,
    )
}

/// Normalize an `atm ack` command into the same outbound write used by
/// `atm send`.  The source message is consulted only to recover the reply
/// address; the reply itself is persisted and emitted by `send_mail`.
///
/// The source acknowledgement state is deliberately updated *after* the
/// canonical write returns successfully.  A failed write therefore cannot
/// consume the pending acknowledgement.
fn acknowledge_via_canonical_write<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    request: AckRequest,
    observability: &dyn ObservabilityPort,
    runtime: &R,
    post_send_emitter: Option<&dyn PostSendHookEmitter>,
    actor: AgentName,
    team: TeamName,
) -> Result<AckOutcome, AtmError> {
    let source = load_ack_source(
        runtime,
        &request.home_dir,
        &team,
        &actor,
        request.message_id,
    )?;
    let source_record =
        load_ack_source_record(runtime, &request.home_dir, &team, &actor, &source.row)?;
    ensure_ack_is_pending(request.message_id, &source_record.envelope)?;
    let reply_target = validate_reply_target(runtime, &source_record, &team)?;
    let canonical_request =
        canonical_ack_write_request(&request, &actor, &team, &reply_target, &source_record);
    let send_outcome = crate::send::write_mail_persisted_with_runtime(
        canonical_request,
        observability,
        runtime,
        post_send_emitter,
    )?;

    mark_source_acknowledged(
        runtime,
        &team,
        &actor,
        source.row.message_key,
        source_record.envelope.expires_at,
    )?;

    let outcome = AckOutcome {
        action: CommandAction::Ack,
        team: team.clone(),
        agent: actor.clone(),
        message_id: request.message_id,
        task_id: source_record.envelope.task_id.clone(),
        reply_disposition: AckReplyDisposition::Sent {
            reply_message_id: send_outcome.message_id,
            reply_target,
        },
        reply_text: request.reply_body,
        warnings: send_outcome.warnings,
    };
    record_ack_telemetry(
        observability,
        &actor,
        team,
        request.message_id,
        outcome.task_id.clone(),
    );
    Ok(outcome)
}

fn canonical_ack_write_request(
    request: &AckRequest,
    actor: &AgentName,
    team: &TeamName,
    target: &ReplyTarget,
    source: &boundary::Message,
) -> SendRequest {
    SendRequest {
        home_dir: request.home_dir.clone(),
        current_dir: request.current_dir.clone(),
        caller_identity: actor.clone(),
        caller_team: team.clone(),
        to: Some(crate::address::AgentAddress {
            agent: target.agent.clone(),
            chat_id: source.envelope.source_chat_id.clone(),
            team: Some(target.team.clone()),
            host: None,
        }),
        message_source: SendMessageSource::Inline(request.reply_body.clone()),
        summary_override: None,
        requires_ack: false,
        task_id: None,
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        acknowledges_message_id: Some(request.message_id),
        dry_run: false,
    }
}

struct LoadedAckSource {
    row: boundary::MailStoreMailboxMetadataRow,
}

fn mark_source_acknowledged<R: RetainedMailboxRuntime>(
    runtime: &R,
    team: &TeamName,
    actor: &AgentName,
    message_key: boundary::MessageKey,
    expires_at: Option<IsoTimestamp>,
) -> Result<(), AtmError> {
    let timestamp = IsoTimestamp::now();
    runtime.persist_message_state(boundary::MailMessageState {
        team: team.clone(),
        agent: actor.clone(),
        actor: actor.clone(),
        message_key,
        read: true,
        pending_ack_at: None,
        acknowledged_at: Some(timestamp),
        expires_at,
        deleted_at: None,
        updated_at: Some(timestamp),
    })
}

fn ensure_roster_member_exists<R: RetainedServiceRuntime>(
    runtime: &R,
    team: &TeamName,
    agent: &AgentName,
    _recovery: &str,
) -> Result<(), AtmError> {
    if runtime.load_roster_member(team, agent)?.is_none() {
        return Err(AtmError::agent_not_found(agent, team));
    }
    Ok(())
}

fn load_ack_source<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &std::path::Path,
    team: &TeamName,
    actor: &AgentName,
    message_id: AtmMessageId,
) -> Result<LoadedAckSource, AtmError> {
    let rows = runtime.query_mailbox_metadata_rows(home_dir, team, actor, None)?;
    let row = rows
        .iter()
        .find(|row| row.message_id == Some(message_id))
        .ok_or_else(|| {
            AtmError::validation(format!(
                "message {message_id} was not found in {actor}@{team}"
            ))
        })?;
    if rows
        .iter()
        .any(|candidate| candidate.parent_message_id == Some(message_id))
    {
        return Err(AtmError::validation(format!(
            "message {message_id} has been updated; acknowledge the current terminal message instead"
        )));
    }
    Ok(LoadedAckSource { row: row.clone() })
}

fn load_ack_source_record<R: RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &std::path::Path,
    team: &TeamName,
    actor: &AgentName,
    row: &boundary::MailStoreMailboxMetadataRow,
) -> Result<boundary::Message, AtmError> {
    runtime
        .load_message_record(home_dir, team, actor, &row.message_key)?
        .ok_or_else(|| {
            AtmError::validation("acknowledgement source metadata could not be reloaded")
        })
}

fn ensure_ack_is_pending(message_id: AtmMessageId, source: &InboxMessage) -> Result<(), AtmError> {
    if source.pending_ack_at.is_some() {
        return Ok(());
    }
    let state = if source.acknowledged_at.is_some() {
        "already acknowledged"
    } else {
        "not pending acknowledgement"
    };
    Err(AtmError::validation(format!(
        "message {message_id} is {state}"
    )))
}

fn validate_reply_target<R: RetainedServiceRuntime>(
    runtime: &R,
    source: &boundary::Message,
    current_team: &TeamName,
) -> Result<ReplyTarget, AtmError> {
    let team = source
        .envelope
        .source_team
        .clone()
        .unwrap_or_else(|| current_team.clone());
    let agent = crate::threading::canonical_sender_identity(&source.envelope);
    ensure_roster_member_exists(
        runtime,
        &team,
        &agent,
        "reload the roster before retrying the acknowledgement",
    )?;
    Ok(ReplyTarget::new(agent, team))
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
        tracing::warn!(%error, command = "ack", "failed to emit acknowledgement telemetry");
    }
}
