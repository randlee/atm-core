//! Send command service implementation and post-send hook handling.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Map;
use tracing::warn;

use crate::ack::AckOutcome;
use crate::address::AgentAddress;
use crate::boundary;
use crate::boundary::PostSendHookEmitter;
use crate::config;
use crate::delivery_execution::{
    DeliveryTransitionContext, emit_delivery_plan_transitions, execute_delivery_plan,
};
use crate::delivery_plan::{
    DeliveryPlan, delivery_plan_disposition, logical_messages_from_persistence,
};
use crate::delivery_policy::{
    DeliveryEventFamily, DeliveryPolicyCoordinator, DeliveryRecipientSnapshot,
};
use crate::error::AtmError;
use crate::error_codes::AtmErrorCode;
use crate::observability::{CommandEvent, ObservabilityPort, action_name, outcome_label};
use crate::provenance::{
    ValidatedWriteProvenance, WriteIngress, WriteProvenance, validate_write_provenance,
};
use crate::schema::{
    AckIntentFields, AtmMessageId, InboxMessage, ThreadMode, set_authenticated_source_host,
    set_peer_outbound_write,
};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::{RetainedMailboxRuntime, default_runtime};
use crate::threading::{ThreadIndex, canonical_sender_identity, is_ephemeral};
use crate::types::{AgentName, ChatId, CommandAction, HostName, IsoTimestamp, TaskId, TeamName};

mod delivery_persistence;
pub(crate) mod file_policy;
pub(crate) mod hook;
pub mod input;
#[doc(hidden)]
pub(crate) mod nudge_template;
mod persistence;
pub(crate) mod summary;

pub(crate) use delivery_persistence::{
    DeliveryPersistenceDisposition, DeliveryPersistenceResult, DuplicateWriteDisposition,
};
#[doc(hidden)]
pub use nudge_template::{
    default_template, qualified_sender_identity as qualified_nudge_sender_identity,
    render_resolved_built_in_nudge,
};
#[cfg(test)]
pub(crate) use persistence::persist_message;

pub(super) const POST_SEND_HOOK_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SendMessageSource {
    Inline(String),
    File {
        path: PathBuf,
        message: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteRequest {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub caller_identity: AgentName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller_chat_id: Option<ChatId>,
    pub caller_team: TeamName,
    /// Set only by the authenticated HTTPS ingress before the shared writer
    /// persists an inbound record. It is not trusted from wire JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authenticated_source_host: Option<HostName>,
    /// The immutable identity assigned by the origin canonical writer.
    /// Authenticated peer ingress preserves it so both hosts store one ULID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_message_id: Option<AtmMessageId>,
    /// The immutable origin timestamp carried with a peer write.  It is set
    /// alongside `origin_message_id` by the canonical origin writer so a
    /// repeated peer delivery compares equal at the receiving store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_timestamp: Option<IsoTimestamp>,
    /// Destination is omitted only by an `atm ack` command.  The daemon
    /// resolves that destination from the acknowledged source before calling
    /// the canonical writer.
    pub to: Option<AgentAddress>,
    pub message_source: SendMessageSource,
    pub summary_override: Option<String>,
    pub requires_ack: bool,
    pub task_id: Option<TaskId>,
    pub parent_message_id: Option<AtmMessageId>,
    pub thread_mode: Option<ThreadMode>,
    pub expires_at: Option<crate::types::IsoTimestamp>,
    /// When present this write is an acknowledgement reply.  It otherwise
    /// follows the exact same persistence and post-write path as a send.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledges_message_id: Option<AtmMessageId>,
    pub dry_run: bool,
}

impl WriteRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        home_dir: PathBuf,
        current_dir: PathBuf,
        caller_identity: AgentName,
        to: &str,
        caller_team: TeamName,
        message_source: SendMessageSource,
        summary_override: Option<String>,
        requires_ack: bool,
        task_id: Option<TaskId>,
        dry_run: bool,
    ) -> Result<Self, AtmError> {
        Ok(Self {
            home_dir,
            current_dir,
            caller_identity,
            caller_chat_id: None,
            caller_team,
            authenticated_source_host: None,
            origin_message_id: None,
            origin_timestamp: None,
            to: Some(to.parse()?),
            message_source,
            summary_override,
            requires_ack,
            task_id,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            acknowledges_message_id: None,
            dry_run,
        })
    }

    #[must_use]
    pub fn with_caller_chat_id(mut self, caller_chat_id: Option<ChatId>) -> Self {
        self.caller_chat_id = caller_chat_id;
        self
    }

    #[must_use]
    pub fn with_origin_message_id(mut self, message_id: AtmMessageId) -> Self {
        self.origin_message_id = Some(message_id);
        self
    }

    #[must_use]
    pub fn with_origin_metadata(
        mut self,
        message_id: AtmMessageId,
        timestamp: IsoTimestamp,
    ) -> Self {
        self.origin_message_id = Some(message_id);
        self.origin_timestamp = Some(timestamp);
        self
    }
}

/// Compatibility name for existing callers.  There is one write payload;
/// acknowledgement is represented by `acknowledges_message_id` on it.
pub type SendRequest = WriteRequest;

/// Result of the one canonical write operation.
///
/// An acknowledgement is not a second transport operation: it is a write
/// whose request carries `acknowledges_message_id`.  The distinct outcome only
/// preserves the CLI/API response shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WriteOutcome {
    Sent(SendOutcome),
    Acknowledged(AckOutcome),
}

impl WriteOutcome {
    /// Returns the immutable identity persisted by this canonical write.
    #[must_use]
    pub fn persisted_message_id(&self) -> AtmMessageId {
        match self {
            Self::Sent(outcome) => outcome.message_id,
            Self::Acknowledged(outcome) => match outcome.reply_disposition {
                crate::ack::AckReplyDisposition::Sent {
                    reply_message_id, ..
                } => reply_message_id,
            },
        }
    }
}

/// A durable write awaiting post-commit notification scheduling.
///
/// Acknowledgement state completes before any post-commit notification or peer
/// work.  Delivery is never a prerequisite for an admitted local write.
pub struct PreparedWrite {
    outcome: SendOutcome,
    outbound_request: WriteRequest,
    persisted_timestamp: IsoTimestamp,
    post_write_needed: bool,
    same_store_peer_receipt: bool,
    #[cfg(test)]
    post_write: LocalPostWrite,
    acknowledgement: Option<crate::ack::ResolvedAcknowledgement>,
}

#[cfg(test)]
struct LocalPostWrite {
    post_send_config: Option<config::AtmConfig>,
    recipient: ResolvedRecipient,
    delivery_snapshot: DeliveryRecipientSnapshot,
    messages: Vec<crate::delivery_plan::LogicalMessage>,
}

impl PreparedWrite {
    /// Returns the immutable identifier persisted by the canonical writer.
    #[must_use]
    pub fn persisted_message_id(&self) -> AtmMessageId {
        self.outcome.message_id
    }

    #[must_use]
    pub fn persisted_timestamp(&self) -> IsoTimestamp {
        self.persisted_timestamp
    }

    /// Returns the canonical, resolved write payload for the post-write
    /// router.  For an acknowledgement this includes the reply destination
    /// resolved from the durable source record; it is not a second path.
    #[must_use]
    pub fn outbound_request(&self) -> WriteRequest {
        self.outbound_request.clone()
    }

    #[cfg(test)]
    pub(crate) fn emit_post_write_for_test<
        R: RetainedServiceRuntime + crate::boundary::sealed::Sealed + ?Sized,
    >(
        &mut self,
        runtime: &R,
        post_send_emitter: &dyn PostSendHookEmitter,
    ) {
        hook::emit_post_send_effects(
            runtime,
            &mut self.outcome.warnings,
            self.post_write.post_send_config.as_ref(),
            Some(post_send_emitter),
            &self.post_write.recipient,
            &self.post_write.delivery_snapshot,
            &self.post_write.messages,
        );
    }

    /// Completes the canonical write before post-commit work is scheduled.
    /// For acknowledgements this records the source transition before the
    /// caller receives its local admission response.
    pub fn finish(
        &mut self,
        runtime: &LocalServiceRuntime,
        observability: &dyn ObservabilityPort,
    ) -> Result<WriteOutcome, AtmError> {
        self.finish_with_runtime(runtime, observability)
    }

    fn finish_with_runtime<R>(
        &mut self,
        runtime: &R,
        observability: &dyn ObservabilityPort,
    ) -> Result<WriteOutcome, AtmError>
    where
        R: RetainedMailboxRuntime,
    {
        match self.acknowledgement.take() {
            Some(acknowledgement) => acknowledgement
                .finish(runtime, observability, self.outcome.clone())
                .map(WriteOutcome::Acknowledged),
            None => Ok(WriteOutcome::Sent(self.outcome.clone())),
        }
    }

    #[must_use]
    pub fn requires_post_write_route(&self) -> bool {
        !self.outcome.dry_run && self.post_write_needed
    }

    /// Whether this write reused an existing immutable record after an
    /// authenticated receipt returned to the same store. The daemon still
    /// performs the ordinary local post-write action exactly once for that
    /// receipt and records the explicit duplicate disposition.
    #[must_use]
    pub fn is_same_store_peer_receipt(&self) -> bool {
        self.same_store_peer_receipt
    }

    /// Whether this canonical write arrived from a peer transport.
    ///
    /// The canonical address deliberately preserves its host qualifier.  The
    /// post-write router therefore must use ingress provenance—not the
    /// address—to choose the one local-vs-peer action.
    #[must_use]
    pub fn is_peer_receipt(&self) -> bool {
        has_authenticated_peer_provenance(&self.outbound_request)
    }
}

/// Executes the local post-write effects from a committed immutable record.
///
/// The daemon calls this only from its post-commit worker.  Admission keeps
/// no prepared payload and never waits for hook, tmux, or graft I/O.
pub fn emit_persisted_local_post_write(
    runtime: &LocalServiceRuntime,
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
    message_id: AtmMessageId,
    post_send_emitter: &dyn PostSendHookEmitter,
) -> Result<(), AtmError> {
    let key = boundary::MessageKey::from(message_id);
    let Some(record) = runtime.load_message_record(home_dir, team, agent, &key)? else {
        return Ok(());
    };
    let recipient = ResolvedRecipient {
        agent: agent.clone(),
        team: team.clone(),
    };
    let delivery_snapshot =
        DeliveryPolicyCoordinator::new().resolve_recipient_snapshot(runtime, team, agent)?;
    let logical = crate::delivery_plan::LogicalMessage::new(
        record.envelope.clone(),
        record.envelope.requires_ack,
        record.envelope.acknowledges_message_id.is_some(),
    )
    .map_err(|error| AtmError::mailbox_read(error.to_string()))?;
    let mut warnings = Vec::new();
    hook::emit_post_send_effects(
        runtime,
        &mut warnings,
        None,
        Some(post_send_emitter),
        &recipient,
        &delivery_snapshot,
        &[logical],
    );
    for warning in warnings {
        tracing::warn!(
            code = ?warning.code,
            message_id = %message_id,
            "post-commit local post-write effect completed with warning: {}",
            warning.message
        );
    }
    Ok(())
}

/// Result of sending one ATM mailbox message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendOutcome {
    pub action: CommandAction,
    pub team: TeamName,
    pub agent: AgentName,
    pub sender: AgentName,
    pub outcome: SendCommandOutcome,
    pub message_id: AtmMessageId,
    pub requires_ack: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<WarningEntry>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SendCommandOutcome {
    Sent,
    DryRun,
}

impl SendCommandOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sent => "sent",
            Self::DryRun => "dry_run",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WarningEntry {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<AtmErrorCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<String>,
}

impl WarningEntry {
    pub fn new(message: impl Into<String>, recovery: Option<impl Into<String>>) -> Self {
        Self {
            message: message.into(),
            code: None,
            recovery: recovery.map(Into::into),
        }
    }

    pub fn with_code(
        code: AtmErrorCode,
        message: impl Into<String>,
        recovery: Option<impl Into<String>>,
    ) -> Self {
        Self {
            message: message.into(),
            code: Some(code),
            recovery: recovery.map(Into::into),
        }
    }

    pub fn render(&self) -> String {
        let message = match self.code {
            Some(code) if !self.message.contains(code.as_str()) => {
                format!("{} [{}]", self.message, code.as_str())
            }
            _ => self.message.clone(),
        };
        match &self.recovery {
            Some(recovery) if !message.contains("Recovery:") => {
                format!("{message} Recovery: {recovery}")
            }
            None => message,
            Some(_) => message,
        }
    }
}

/// Send one mailbox message to a team member.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::IdentityUnavailable`],
/// [`crate::error_codes::AtmErrorCode::TeamUnavailable`],
/// [`crate::error_codes::AtmErrorCode::TeamNotFound`],
/// [`crate::error_codes::AtmErrorCode::AgentNotFound`],
/// [`crate::error_codes::AtmErrorCode::AddressParseFailed`],
/// [`crate::error_codes::AtmErrorCode::SelfAddressedSendInvalid`],
/// [`crate::error_codes::AtmErrorCode::FilePolicyRejected`],
/// [`crate::error_codes::AtmErrorCode::MailboxReadFailed`], or
/// [`crate::error_codes::AtmErrorCode::MailboxWriteFailed`] when sender
/// identity cannot be resolved, recipient or team validation fails,
/// message/file-policy validation fails, or mailbox persistence fails.
pub fn send_mail(
    request: SendRequest,
    observability: &dyn ObservabilityPort,
) -> Result<SendOutcome, AtmError> {
    let runtime = default_runtime()?;
    send_mail_with_runtime(request, observability, &runtime)
}

/// Execute one canonical write without daemon-owned post-write delivery.
pub fn write_mail(
    request: WriteRequest,
    observability: &dyn ObservabilityPort,
) -> Result<WriteOutcome, AtmError> {
    let runtime = default_runtime()?;
    write_mail_with_runtime(request, observability, &runtime)
}

/// Execute one canonical write with an explicit local runtime.
pub fn write_mail_with_runtime(
    request: WriteRequest,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
) -> Result<WriteOutcome, AtmError> {
    let mut prepared = write_mail_with_runtime_impl(request, observability, runtime)?;
    prepared.finish(runtime, observability)
}

pub fn send_mail_with_runtime(
    request: SendRequest,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
) -> Result<SendOutcome, AtmError> {
    match write_mail_with_runtime_impl(request, observability, runtime)?
        .finish(runtime, observability)?
    {
        WriteOutcome::Sent(outcome) => Ok(outcome),
        WriteOutcome::Acknowledged(_) => Err(AtmError::validation(
            "send helper cannot finish an acknowledgement write",
        )),
    }
}

/// Prepares the shared durable write for daemon-owned post-write routing.
///
/// This performs canonical validation and persistence, but deliberately does
/// not emit a nudge, send to a peer, or mutate an acknowledgement source.
/// `PostWriteRouter` owns those actions and calls [`PreparedWrite::finish`]
/// after its selected action succeeds.
pub fn prepare_write_with_runtime(
    request: WriteRequest,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
) -> Result<PreparedWrite, AtmError> {
    write_mail_with_runtime_impl(request, observability, runtime)
}

/// The sole write pipeline. `acknowledges_message_id` selects only an
/// acknowledgement-source normalization step; both variants persist through
/// the same canonical writer exactly once.
fn write_mail_with_runtime_impl<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    request: WriteRequest,
    observability: &dyn ObservabilityPort,
    runtime: &R,
) -> Result<PreparedWrite, AtmError> {
    validate_write_provenance(
        WriteIngress::Canonical,
        WriteProvenance {
            target_host: request.to.as_ref().and_then(|address| address.host()),
            authenticated_source_host: request.authenticated_source_host.as_ref(),
            origin_message_id: request.origin_message_id.is_some(),
            origin_timestamp: request.origin_timestamp.is_some(),
        },
    )?;
    if request.acknowledges_message_id.is_none() {
        if request.to.is_none() {
            return Err(AtmError::validation(
                "message write is missing a destination",
            ));
        }
        return prepare_persisted_write(request, observability, runtime, None);
    }
    let acknowledgement = crate::ack::admit_acknowledgement_write(request, runtime)?;
    prepare_atomic_acknowledgement_write(acknowledgement, observability, runtime)
}

fn prepare_atomic_acknowledgement_write<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    acknowledgement: crate::ack::AtomicAcknowledgementWrite,
    observability: &dyn ObservabilityPort,
    _runtime: &R,
) -> Result<PreparedWrite, AtmError> {
    let source_task_id = acknowledgement.acknowledgement.source_task_id();
    let reply = acknowledgement.reply;
    let recipient = ResolvedRecipient {
        team: reply.team.clone(),
        agent: reply.agent.clone(),
    };
    let outcome = SendOutcome {
        action: CommandAction::Send,
        team: recipient.team.clone(),
        agent: recipient.agent.clone(),
        sender: reply.envelope.from.clone(),
        outcome: SendCommandOutcome::Sent,
        message_id: reply.envelope.message_id.ok_or_else(|| {
            AtmError::mailbox_write("atomic acknowledgement reply is missing its message ID")
        })?,
        requires_ack: false,
        task_id: source_task_id.clone(),
        summary: reply.envelope.summary.clone(),
        message: Some(reply.envelope.text.clone()),
        warnings: Vec::new(),
        dry_run: false,
    };
    emit_send_command_event(
        observability,
        SendCommandOutcome::Sent.as_str(),
        &outcome,
        source_task_id,
        &reply.envelope.from,
    );
    #[cfg(test)]
    let post_write = {
        let delivery_snapshot = DeliveryPolicyCoordinator::new().resolve_recipient_snapshot(
            _runtime,
            &recipient.team,
            &recipient.agent,
        )?;
        let logical =
            crate::delivery_plan::LogicalMessage::new(reply.envelope.clone(), false, true)
                .map_err(|error| AtmError::mailbox_read(error.to_string()))?;
        LocalPostWrite {
            post_send_config: None,
            recipient,
            delivery_snapshot,
            messages: vec![logical],
        }
    };
    Ok(PreparedWrite {
        outcome,
        outbound_request: acknowledgement.canonical_request,
        persisted_timestamp: reply.envelope.timestamp,
        post_write_needed: true,
        same_store_peer_receipt: false,
        #[cfg(test)]
        post_write,
        acknowledgement: Some(acknowledgement.acknowledgement),
    })
}

fn prepare_persisted_write<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    request: SendRequest,
    observability: &dyn ObservabilityPort,
    runtime: &R,
    acknowledgement: Option<crate::ack::ResolvedAcknowledgement>,
) -> Result<PreparedWrite, AtmError> {
    let context = prepare_send_context(runtime, &request)?;
    let task_id = request.task_id.clone();
    let requires_ack = request.requires_ack || task_id.is_some();
    let body = resolve_message_body(
        &request.message_source,
        &request.current_dir,
        &request.home_dir,
        &context.recipient.team,
    )?;
    let summary = summary::build_summary(&body, request.summary_override.clone());
    let message_id = request.origin_message_id.unwrap_or_default();
    let timestamp = request.origin_timestamp.unwrap_or_else(IsoTimestamp::now);
    let acknowledgement_source_update = None;
    let persistence = persist_send_message(
        runtime,
        &request,
        &context,
        &body,
        &summary,
        message_id,
        timestamp,
        requires_ack,
        task_id.clone(),
        acknowledgement_source_update,
    )?;
    // A same-host HTTPS receipt deliberately reuses the origin ULID. Storage
    // skips its duplicate row, but the ordinary post-write route still emits
    // the visible local nudge once the peer receipt has completed.
    let post_write_needed = persistence.requires_post_write();
    let same_store_peer_receipt =
        persistence.duplicate_disposition == DuplicateWriteDisposition::SameStorePeerReceipt;
    #[cfg(test)]
    let messages =
        post_send_messages_from_persistence(&persistence, requires_ack, acknowledgement.is_some())?;
    let outcome = finalize_send_outcome(
        runtime,
        observability,
        &request,
        &context,
        &body,
        &summary,
        message_id,
        requires_ack,
        task_id,
        persistence,
    )?;
    Ok(PreparedWrite {
        outcome,
        outbound_request: request,
        persisted_timestamp: timestamp,
        post_write_needed,
        same_store_peer_receipt,
        #[cfg(test)]
        post_write: LocalPostWrite {
            post_send_config: context.post_send_config,
            recipient: context.recipient,
            delivery_snapshot: context.delivery_snapshot,
            messages,
        },
        acknowledgement,
    })
}

fn has_authenticated_peer_provenance(request: &WriteRequest) -> bool {
    validate_write_provenance(
        WriteIngress::Canonical,
        WriteProvenance {
            target_host: request.to.as_ref().and_then(|address| address.host()),
            authenticated_source_host: request.authenticated_source_host.as_ref(),
            origin_message_id: request.origin_message_id.is_some(),
            origin_timestamp: request.origin_timestamp.is_some(),
        },
    )
    .is_ok_and(ValidatedWriteProvenance::is_authenticated_peer)
}

#[cfg(test)]
fn send_mail_with_runtime_impl<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    request: WriteRequest,
    observability: &dyn ObservabilityPort,
    runtime: &R,
    post_send_emitter: Option<&dyn PostSendHookEmitter>,
) -> Result<SendOutcome, AtmError> {
    let mut prepared = write_mail_with_runtime_impl(request, observability, runtime)?;
    if prepared.requires_post_write_route()
        && let Some(post_send_emitter) = post_send_emitter
    {
        // Test-only harness: production notification is owned by
        // `PostWriteRouter::dispatch` in atm-daemon.
        prepared.emit_post_write_for_test(runtime, post_send_emitter);
    }
    match prepared.finish_with_runtime(runtime, observability)? {
        WriteOutcome::Sent(outcome) => Ok(outcome),
        WriteOutcome::Acknowledged(_) => Err(AtmError::validation(
            "test send helper cannot finish an acknowledgement write",
        )),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "Y.6 closeout keeps the explicit send outcome pieces visible at the sprint seam."
)]
fn finalize_send_outcome<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    runtime: &R,
    observability: &dyn ObservabilityPort,
    request: &SendRequest,
    context: &SendExecutionContext,
    body: &str,
    summary: &str,
    message_id: AtmMessageId,
    requires_ack: bool,
    task_id: Option<TaskId>,
    persistence: DeliveryPersistenceResult,
) -> Result<SendOutcome, AtmError> {
    let command_outcome = if request.dry_run {
        SendCommandOutcome::DryRun
    } else {
        SendCommandOutcome::Sent
    };
    let mut outcome = build_send_outcome(
        request,
        context,
        body,
        summary,
        message_id,
        requires_ack,
        task_id.clone(),
        command_outcome,
        &persistence,
    );
    if !request.dry_run {
        let plan = build_send_delivery_plan(context, requires_ack, &persistence)?;
        let execution = execute_delivery_plan(runtime, context.command_config.as_ref(), &plan)?;
        emit_delivery_plan_transitions(
            observability,
            DeliveryTransitionContext {
                family: context.delivery_family,
                team: &context.recipient.team,
                agent: &context.recipient.agent,
                sender: &context.canonical_sender,
                message_id,
                task_id: task_id.clone(),
            },
            &plan,
            &execution,
        )?;
        outcome.warnings.extend(execution.warnings);
    }
    emit_send_command_event(
        observability,
        command_outcome.as_str(),
        &outcome,
        task_id,
        &context.canonical_sender,
    );
    Ok(outcome)
}

#[expect(
    clippy::too_many_arguments,
    reason = "Y.6 closeout keeps the explicit send outcome fields aligned with the command contract."
)]
fn build_send_outcome(
    request: &SendRequest,
    context: &SendExecutionContext,
    body: &str,
    summary: &str,
    message_id: AtmMessageId,
    requires_ack: bool,
    task_id: Option<TaskId>,
    command_outcome: SendCommandOutcome,
    persistence: &DeliveryPersistenceResult,
) -> SendOutcome {
    let mut outcome = SendOutcome {
        action: CommandAction::Send,
        team: context.recipient.team.clone(),
        agent: context.recipient.agent.clone(),
        sender: context.canonical_sender.clone(),
        outcome: command_outcome,
        message_id,
        requires_ack,
        task_id,
        summary: Some(summary.to_string()),
        message: request.dry_run.then_some(body.to_string()),
        warnings: context.warnings.clone(),
        dry_run: request.dry_run,
    };
    outcome
        .warnings
        .extend(persistence.warnings.iter().cloned());
    outcome
}

fn build_send_delivery_plan(
    context: &SendExecutionContext,
    requires_ack: bool,
    persistence: &DeliveryPersistenceResult,
) -> Result<DeliveryPlan, AtmError> {
    Ok(DeliveryPlan::new(
        crate::delivery_plan::DeliveryPlanKind::Send,
        delivery_plan_disposition(persistence.disposition),
        crate::delivery_plan::delivery_target_for_snapshot(
            &context.inbox_path,
            &context.delivery_snapshot,
        ),
        context.recipient.clone(),
        logical_messages_from_persistence(persistence, requires_ack, false)
            .map_err(|error| AtmError::mailbox_write(error.to_string()))?,
        persistence.warnings.clone(),
    ))
}

#[cfg(test)]
fn post_send_messages_from_persistence(
    persistence: &DeliveryPersistenceResult,
    requires_ack: bool,
    is_ack: bool,
) -> Result<Vec<crate::delivery_plan::LogicalMessage>, AtmError> {
    crate::delivery_plan::LogicalMessage::new(
        persistence.original_message.clone(),
        requires_ack,
        is_ack,
    )
    .map(|message| vec![message])
    .map_err(|error| AtmError::mailbox_write(error.to_string()))
}

struct SendExecutionContext {
    command_config: Option<config::AtmConfig>,
    #[cfg(test)]
    post_send_config: Option<config::AtmConfig>,
    recipient: ResolvedRecipient,
    canonical_sender: AgentName,
    inbox_path: PathBuf,
    delivery_snapshot: DeliveryRecipientSnapshot,
    delivery_family: DeliveryEventFamily,
    warnings: Vec<WarningEntry>,
}

fn prepare_send_context<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    runtime: &R,
    request: &SendRequest,
) -> Result<SendExecutionContext, AtmError> {
    // This is the durable-admission half of the pipeline.  A daemon must not
    // inspect a caller workspace or hook configuration before replying to a
    // committed write: those are post-commit worker concerns.  The daemon
    // runtime already rejects workspace config, but avoiding this call here
    // makes the no-filesystem-read admission contract structural rather than
    // dependent on the runtime implementation.
    let command_config = None;
    #[cfg(test)]
    let post_send_config = None;
    let warnings = Vec::new();
    let canonical_sender = request.caller_identity.clone();
    let target = request.to.as_ref().ok_or_else(|| {
        AtmError::validation("write request destination must be resolved before persistence")
    })?;
    let provenance = validate_write_provenance(
        WriteIngress::Canonical,
        WriteProvenance {
            target_host: target.host(),
            authenticated_source_host: request.authenticated_source_host.as_ref(),
            origin_message_id: request.origin_message_id.is_some(),
            origin_timestamp: request.origin_timestamp.is_some(),
        },
    )?;
    let recipient = resolve_recipient(target, &request.caller_team, command_config.as_ref())?;
    validate_non_self_recipient(
        &canonical_sender,
        &request.caller_team,
        &recipient,
        target,
        provenance,
    )?;
    let inbox_path = runtime.inbox_path(&request.home_dir, &recipient.team, &recipient.agent)?;
    let delivery_policy = DeliveryPolicyCoordinator::new();
    let delivery_snapshot =
        delivery_policy.resolve_write_recipient_snapshot(runtime, &recipient, provenance)?;
    let delivery_family = DeliveryPolicyCoordinator::resolve_send_family(
        request.parent_message_id,
        request.thread_mode,
    );
    Ok(SendExecutionContext {
        command_config,
        #[cfg(test)]
        post_send_config,
        recipient,
        canonical_sender,
        inbox_path,
        delivery_snapshot,
        delivery_family,
        warnings,
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "Send persistence needs the explicit request/body/message envelope fields documented in the Y.4 state-machine seam."
)]
fn persist_send_message<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    request: &SendRequest,
    context: &SendExecutionContext,
    body: &str,
    summary: &str,
    message_id: AtmMessageId,
    timestamp: IsoTimestamp,
    requires_ack: bool,
    task_id: Option<TaskId>,
    acknowledgement_source_update: Option<boundary::Message>,
) -> Result<DeliveryPersistenceResult, AtmError> {
    let mut envelope = build_send_envelope(
        request,
        context,
        body,
        summary,
        message_id,
        timestamp,
        requires_ack,
        task_id,
    );
    if request.dry_run {
        return Ok(DeliveryPersistenceResult::persisted(envelope));
    }
    // Origin metadata is assigned only by the canonical origin writer and is
    // required on every peer receipt. It prevents an inbound peer write from
    // becoming a second outbound peer delivery while preserving its original
    // host-qualified address for the shared writer and a later ACK.
    if request.authenticated_source_host.is_none()
        && request.origin_message_id.is_none()
        && let Some(host) = request.to.as_ref().and_then(|address| address.host())
    {
        let exact_request = request.clone().with_origin_metadata(message_id, timestamp);
        let request_json = serde_json::to_string(&exact_request).map_err(|_source| {
            AtmError::mailbox_write("failed to serialize immutable peer outbound write")
        })?;
        set_peer_outbound_write(&mut envelope, host, request_json);
    }
    persistence::persist_message_with_ack_update(
        runtime,
        &request.home_dir,
        &context.delivery_snapshot,
        &context.inbox_path,
        &envelope,
        false,
        request
            .authenticated_source_host
            .as_ref()
            .and_then(|source_host| {
                request
                    .to
                    .as_ref()
                    .and_then(|destination| destination.host())
                    .map(|destination_host| (source_host, destination_host))
            }),
        acknowledgement_source_update,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the immutable envelope is assembled from the canonical write fields"
)]
fn build_send_envelope(
    request: &SendRequest,
    context: &SendExecutionContext,
    body: &str,
    summary: &str,
    message_id: AtmMessageId,
    timestamp: IsoTimestamp,
    requires_ack: bool,
    task_id: Option<TaskId>,
) -> InboxMessage {
    let ack_intent = AckIntentFields::from_requires_ack(requires_ack, timestamp);
    let mut envelope = InboxMessage {
        from: context.canonical_sender.clone(),
        source_chat_id: request.caller_chat_id.clone(),
        text: body.to_string(),
        timestamp,
        read: false,
        source_team: Some(request.caller_team.clone()),
        destination_chat_id: request
            .to
            .as_ref()
            .and_then(|address| address.chat_id().cloned()),
        summary: Some(summary.to_string()),
        message_id: Some(message_id),
        requires_ack: ack_intent.requires_ack,
        pending_ack_at: ack_intent.pending_ack_at,
        acknowledged_at: ack_intent.acknowledged_at,
        acknowledges_message_id: request.acknowledges_message_id,
        parent_message_id: request.parent_message_id,
        thread_mode: request.thread_mode,
        expires_at: request.expires_at,
        task_id: task_id.clone(),
        extra: Map::new(),
    };
    set_authenticated_source_host(&mut envelope, request.authenticated_source_host.clone());
    envelope
}

fn emit_send_command_event(
    observability: &dyn ObservabilityPort,
    outcome_name: &'static str,
    outcome: &SendOutcome,
    task_id: Option<TaskId>,
    canonical_sender: &AgentName,
) {
    if let Err(error) = observability.emit(CommandEvent {
        command: "send",
        action: action_name("send"),
        outcome: outcome_label(outcome_name),
        team: outcome.team.clone(),
        agent: outcome.agent.clone(),
        sender: canonical_sender.clone(),
        message_id: Some(outcome.message_id),
        requires_ack: outcome.requires_ack,
        dry_run: outcome.dry_run,
        task_id,
        error_code: None,
        error_message: None,
    }) {
        warn!(%error, command = "send", action = "send", "failed to emit send command event");
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedRecipient {
    pub(crate) agent: AgentName,
    pub(crate) team: TeamName,
}

pub(crate) fn validate_non_self_recipient(
    sender: &AgentName,
    sender_team: &TeamName,
    recipient: &ResolvedRecipient,
    target: &AgentAddress,
    provenance: ValidatedWriteProvenance,
) -> Result<(), AtmError> {
    let same_identity = sender
        .as_str()
        .eq_ignore_ascii_case(recipient.agent.as_str())
        && sender_team
            .as_str()
            .eq_ignore_ascii_case(recipient.team.as_str());
    if same_identity && target.host().is_none() && !provenance.is_authenticated_peer() {
        return Err(AtmError::self_addressed_send_invalid(format!(
            "self-addressed messages are invalid ATM input: '{sender}@{sender_team}' may not send to itself"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod self_address_tests {
    use super::{ResolvedRecipient, validate_non_self_recipient};
    use crate::address::AgentAddress;
    use crate::error_codes::AtmErrorCode;
    use crate::provenance::{WriteIngress, WriteProvenance, validate_write_provenance};
    use crate::types::{AgentName, TeamName};

    #[test]
    fn validate_non_self_recipient_rejects_case_variant_self_target() {
        let provenance = validate_write_provenance(
            WriteIngress::Canonical,
            WriteProvenance {
                target_host: None,
                authenticated_source_host: None,
                origin_message_id: false,
                origin_timestamp: false,
            },
        )
        .expect("local provenance");
        let error = validate_non_self_recipient(
            &AgentName::from_validated("Sender-A"),
            &TeamName::from_validated("Test-Team"),
            &ResolvedRecipient {
                agent: AgentName::from_validated("sender-a"),
                team: TeamName::from_validated("test-team"),
            },
            &"sender-a@test-team"
                .parse::<AgentAddress>()
                .expect("target"),
            provenance,
        )
        .expect_err("case-variant self target must be rejected");

        assert_eq!(error.code(), AtmErrorCode::SelfAddressedSendInvalid);
    }

    #[test]
    fn validate_non_self_recipient_allows_host_qualified_self_target() {
        let target = "sender-a@test-team.127.0.0.1"
            .parse::<AgentAddress>()
            .expect("host-qualified target");
        let provenance = validate_write_provenance(
            WriteIngress::Canonical,
            WriteProvenance {
                target_host: target.host(),
                authenticated_source_host: None,
                origin_message_id: false,
                origin_timestamp: false,
            },
        )
        .expect("host-qualified origin provenance");
        validate_non_self_recipient(
            &AgentName::from_validated("sender-a"),
            &TeamName::from_validated("test-team"),
            &ResolvedRecipient {
                agent: AgentName::from_validated("sender-a"),
                team: TeamName::from_validated("test-team"),
            },
            &target,
            provenance,
        )
        .expect("host-qualified self target must use the ordinary peer route");
    }

    #[test]
    fn validate_non_self_recipient_allows_authenticated_peer_after_target_normalization() {
        let target = "sender-a@test-team"
            .parse::<AgentAddress>()
            .expect("normalized target");
        let peer_host = "peer.example.test".parse().expect("peer host");
        let provenance = validate_write_provenance(
            WriteIngress::Canonical,
            WriteProvenance {
                target_host: target.host(),
                authenticated_source_host: Some(&peer_host),
                origin_message_id: true,
                origin_timestamp: true,
            },
        )
        .expect("authenticated peer provenance");
        validate_non_self_recipient(
            &AgentName::from_validated("sender-a"),
            &TeamName::from_validated("test-team"),
            &ResolvedRecipient {
                agent: AgentName::from_validated("sender-a"),
                team: TeamName::from_validated("test-team"),
            },
            &target,
            provenance,
        )
        .expect("authenticated peer receipt must not become a local self-send");
    }
}

fn resolve_recipient(
    target_address: &AgentAddress,
    caller_team: &TeamName,
    config: Option<&config::AtmConfig>,
) -> Result<ResolvedRecipient, AtmError> {
    // `AgentAddress` has already validated the explicit team segment. Never
    // parse it again and silently substitute the caller team on failure.
    let team = target_address
        .team()
        .cloned()
        .unwrap_or_else(|| caller_team.clone());

    Ok(ResolvedRecipient {
        agent: config::aliases::resolve_agent_name(target_address.agent(), config)?,
        team,
    })
}

fn resolve_message_body(
    source: &SendMessageSource,
    current_dir: &Path,
    home_dir: &Path,
    team_name: &TeamName,
) -> Result<String, AtmError> {
    match source {
        SendMessageSource::Inline(message) => input::validate_message_text(message.clone()),
        SendMessageSource::File { path, message } => {
            input::validate_message_text(file_policy::process_file_reference(
                path,
                message.as_deref(),
                team_name,
                current_dir,
                home_dir,
            )?)
        }
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn prepare_threaded_message(
    envelope: &mut InboxMessage,
    inbox_messages: &[InboxMessage],
) -> Result<(), AtmError> {
    match (
        envelope.parent_message_id,
        envelope.thread_mode,
        envelope.expires_at,
    ) {
        (None, None, _) => Ok(()),
        (Some(_), Some(_), Some(_)) => Err(AtmError::validation(
            "ephemeral messages may not participate in a message thread",
        )),
        (Some(parent_id), Some(_), None) => {
            validate_thread_append(envelope, inbox_messages, parent_id)
        }
        (Some(_), None, _) | (None, Some(_), _) => Err(AtmError::validation(
            "thread updates must set both parent_message_id and thread_mode",
        )),
    }
}

fn validate_thread_append(
    envelope: &mut InboxMessage,
    inbox_messages: &[InboxMessage],
    parent_id: AtmMessageId,
) -> Result<(), AtmError> {
    let index = ThreadIndex::new(inbox_messages);
    let parent = index.message(parent_id).ok_or_else(|| {
        AtmError::validation(format!(
            "thread parent message {} was not found in the recipient inbox",
            parent_id
        ))
    })?;

    if is_ephemeral(parent) {
        return Err(AtmError::validation(
            "ephemeral messages may not be updated or superseded",
        ));
    }

    let Some(root_id) = index.root_id(parent_id) else {
        return Err(AtmError::validation(format!(
            "thread root could not be resolved for parent message {}",
            parent_id
        )));
    };
    let root = index.message(root_id).ok_or_else(|| {
        AtmError::validation(format!(
            "thread root message {} was not found in the recipient inbox",
            root_id
        ))
    })?;

    if canonical_sender_identity(root) != canonical_sender_identity(envelope) {
        return Err(AtmError::validation(
            "only the original sender may append details or supersede a message thread",
        ));
    }

    if index.has_successor(parent_id) {
        return Err(AtmError::validation(format!(
            "message {} already has a successor; ATM threads are strictly linear",
            parent_id
        )));
    }

    let thread_requires_ack = index.thread_requires_ack(parent_id);
    envelope.requires_ack = thread_requires_ack;
    envelope.pending_ack_at = thread_requires_ack.then_some(envelope.timestamp);
    envelope.acknowledged_at = None;
    Ok(())
}

#[allow(
    dead_code,
    reason = "Retained for the dormant direct post-send hook helper."
)]
pub(super) fn qualified_sender_identity(
    sender: &AgentName,
    sender_team: Option<&TeamName>,
) -> String {
    sender_team
        .map(|team| format!("{sender}@{team}"))
        .unwrap_or_else(|| sender.to_string())
}

#[cfg(test)]
mod graft_warning_tests;
#[cfg(test)]
mod post_write_tests;
#[cfg(test)]
mod tests;
