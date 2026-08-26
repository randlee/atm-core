//! The one canonical mail-write pipeline.
//!
//! This module owns durable write admission for both sends and
//! acknowledgements: entry points, the shared [`PreparedWrite`] hand-off, and
//! acknowledgement admission. `send` re-exports the public entry points and
//! `ack` re-exports the acknowledgement request/outcome types, so external
//! paths (including serde/persisted shapes) are unchanged.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[cfg(test)]
use crate::boundary::MessageReceivedHookEmitter;
use crate::boundary::{self, BuiltInPostSendDispatch};
use crate::caller_context::ActivityObservation;
use crate::delivery_policy::DeliveryPolicyCoordinator;
use crate::error::AtmError;
use crate::observability::{CommandEvent, ObservabilityPort, action_name, outcome_label};
use crate::provenance::{
    ValidatedWriteProvenance, WriteIngress, WriteProvenance, validate_write_provenance,
};
use crate::schema::{AtmMessageId, InboxMessage, authenticated_source_host, peer_delivery_target};
use crate::send::{
    DeliveryExecutionMode, DuplicateWriteDisposition, PreparedReceivedHook, ResolvedRecipient,
    SendCommandOutcome, SendMessageSource, SendOutcome, SendRequest, WriteRequest,
    annotate_path_only_body, emit_send_command_event, finalize_send_outcome, persist_send_message,
    prepare_received_hook, prepare_send_context, request_requires_ack, resolve_message_body,
};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::{RetainedMailboxRuntime, default_runtime};
use crate::types::{AgentName, ChatId, CommandAction, HostName, IsoTimestamp, TaskId, TeamName};
use atm_storage::contract::{
    AcknowledgementReplyBuilder, AcknowledgementSource, Message as StoredMessage, MessageKey,
};

/// Parameters for acknowledging one pending-ack mailbox message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckRequest {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub caller_identity: AgentName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller_chat_id: Option<ChatId>,
    pub caller_team: TeamName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_observation: Option<ActivityObservation>,
    pub message_id: AtmMessageId,
    pub reply_body: String,
}

impl AckRequest {
    pub fn into_write_request(self) -> SendRequest {
        SendRequest {
            home_dir: self.home_dir,
            current_dir: self.current_dir,
            caller_identity: self.caller_identity,
            caller_chat_id: self.caller_chat_id,
            caller_team: self.caller_team,
            activity_observation: self.activity_observation,
            authenticated_source_host: None,
            origin_message_id: None,
            origin_timestamp: None,
            to: None,
            message_source: SendMessageSource::Inline(self.reply_body),
            classification: crate::send::MessageClassification::default(),
            max_message_bytes: crate::send::input::default_message_max_bytes(),
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
        let reply_body = match request.message_source {
            SendMessageSource::Inline(reply_body) => reply_body,
            SendMessageSource::File { .. } | SendMessageSource::Template(_) => {
                return Err(AtmError::validation(
                    "acknowledgement reply body must be inline",
                ));
            }
        };
        Ok(Self {
            home_dir: request.home_dir,
            current_dir: request.current_dir,
            caller_identity: request.caller_identity,
            caller_chat_id: request.caller_chat_id,
            caller_team: request.caller_team,
            activity_observation: request.activity_observation,
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
    host: Option<HostName>,
}

impl ReplyTarget {
    pub(crate) fn new(agent: AgentName, team: TeamName, host: Option<HostName>) -> Self {
        Self { agent, team, host }
    }
}

impl std::fmt::Display for ReplyTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.agent, self.team)?;
        if let Some(host) = &self.host {
            write!(f, ".{host}")?;
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
        let address: crate::address::AgentAddress =
            value.parse().map_err(serde::de::Error::custom)?;
        let team = address
            .team()
            .cloned()
            .ok_or_else(|| serde::de::Error::custom("expected <agent>@<team> reply target"))?;
        Ok(Self::new(
            address.agent().clone(),
            team,
            address.host().cloned(),
        ))
    }
}

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
                AckReplyDisposition::Sent {
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
    received_hook: Result<Option<PreparedReceivedHook>, AtmError>,
    acknowledgement: Option<ResolvedAcknowledgement>,
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

    /// Whether this canonical write produced a new durable record that must
    /// enter the receiver-side post-persistence route.
    ///
    /// Idempotent duplicate delivery deliberately returns `false`: the
    /// already-recorded immutable message remains the successful result, but
    /// it must not emit a second received-message hook.
    #[must_use]
    pub fn is_newly_persisted(&self) -> bool {
        !self.outcome.dry_run && self.post_write_needed
    }

    /// Whether this write reused an existing immutable record after an
    /// authenticated receipt returned to the same store. This is an
    /// idempotent receiver-side duplicate: the explicit disposition is
    /// recorded but no second received-message hook is emitted.
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

    /// Builds receiver-hook dispatches from the write's retained in-memory
    /// planning data. This is valid only after durable admission has returned
    /// successfully; it deliberately never reloads the committed record.
    pub fn build_received_hook_dispatches(
        &self,
        runtime: &LocalServiceRuntime,
    ) -> Result<Vec<BuiltInPostSendDispatch>, AtmError> {
        let post_write = match &self.received_hook {
            Ok(Some(post_write)) => post_write,
            Ok(None) => return Ok(Vec::new()),
            Err(error) => return Err(error.clone()),
        };
        let mut dispatches = Vec::new();
        for message in &post_write.messages {
            let event = crate::send::hook::post_send_event_from_message(
                &post_write.recipient,
                message,
                post_write.delivery_snapshot.recipient_pane_id.as_ref(),
            )?;
            if let Some(dispatch) = crate::send::hook::build_built_in_dispatch(
                runtime,
                &post_write.delivery_snapshot,
                &event,
                &message.envelope.text,
            ) {
                dispatches.push(dispatch);
            }
        }
        Ok(dispatches)
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
    let mut prepared = write_mail_with_runtime_impl_with_mode(
        request,
        observability,
        runtime,
        DeliveryExecutionMode::Inline,
    )?;
    prepared.finish(runtime, observability)
}

pub fn send_mail_with_runtime(
    request: SendRequest,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
) -> Result<SendOutcome, AtmError> {
    match write_mail_with_runtime_impl_with_mode(
        request,
        observability,
        runtime,
        DeliveryExecutionMode::Inline,
    )?
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
/// `StorageAndNudgeRouter` owns those actions and calls [`PreparedWrite::finish`]
/// after its selected action succeeds.
pub fn prepare_write_with_runtime(
    request: WriteRequest,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
) -> Result<PreparedWrite, AtmError> {
    write_mail_with_runtime_impl_with_mode(
        request,
        observability,
        runtime,
        DeliveryExecutionMode::Deferred,
    )
}

/// Prepares one canonical write through the Tokio durable-admission boundary.
///
/// Core validation and response construction remain shared with the legacy
/// path. The immutable storage transition is the only await: it enqueues work
/// to the backend's bounded writer lane and receives that lane's durable
/// result without a blocking task in the HTTP runtime.
pub async fn prepare_write_with_async_runtime(
    request: WriteRequest,
    observability: &(dyn ObservabilityPort + Send + Sync),
    runtime: &LocalServiceRuntime,
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
        return prepare_persisted_write_async(request, observability, runtime, None).await;
    }
    if has_authenticated_peer_provenance(&request) {
        return prepare_persisted_write_async(request, observability, runtime, None).await;
    }
    let acknowledgement = admit_acknowledgement_write_async(request, runtime).await?;
    prepare_atomic_acknowledgement_write(acknowledgement, observability, runtime)
}

/// The sole write pipeline. `acknowledges_message_id` selects only an
/// acknowledgement-source normalization step; both variants persist through
/// the same canonical writer exactly once.
fn write_mail_with_runtime_impl_with_mode<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    request: WriteRequest,
    observability: &dyn ObservabilityPort,
    runtime: &R,
    delivery_mode: DeliveryExecutionMode,
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
        return prepare_persisted_write(request, observability, runtime, None, delivery_mode);
    }
    // A locally invoked `atm ack` resolves and transitions its durable source
    // atomically.  Its host-qualified reply is then received by the original
    // sender as an ordinary canonical peer write carrying causal metadata.
    // The direct-send model has no sender-side outbox record to mutate, so a
    // peer ACK receipt must never attempt a second acknowledgement-source
    // lookup or synthesize an acknowledgement-of-an-ack reply.
    if has_authenticated_peer_provenance(&request) {
        return prepare_persisted_write(request, observability, runtime, None, delivery_mode);
    }
    let acknowledgement = admit_acknowledgement_write(request, runtime)?;
    prepare_atomic_acknowledgement_write(acknowledgement, observability, runtime)
}

#[cfg(test)]
pub(crate) fn write_mail_with_runtime_impl<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    request: WriteRequest,
    observability: &dyn ObservabilityPort,
    runtime: &R,
) -> Result<PreparedWrite, AtmError> {
    write_mail_with_runtime_impl_with_mode(
        request,
        observability,
        runtime,
        DeliveryExecutionMode::Inline,
    )
}

fn prepare_atomic_acknowledgement_write<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    acknowledgement: AtomicAcknowledgementWrite,
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
    let delivery_snapshot = DeliveryPolicyCoordinator::new().resolve_recipient_snapshot(
        _runtime,
        &recipient.team,
        &recipient.agent,
    )?;
    let logical = crate::delivery_plan::LogicalMessage::new(reply.envelope.clone(), false, true)
        .map_err(|error| AtmError::mailbox_read(error.to_string()))?;
    let received_hook = Ok(Some(PreparedReceivedHook {
        recipient: recipient.clone(),
        delivery_snapshot: delivery_snapshot.clone(),
        messages: vec![logical.clone()],
    }));
    Ok(PreparedWrite {
        outcome,
        outbound_request: acknowledgement.canonical_request,
        persisted_timestamp: reply.envelope.timestamp,
        post_write_needed: true,
        same_store_peer_receipt: false,
        received_hook,
        acknowledgement: Some(acknowledgement.acknowledgement),
    })
}

fn prepare_persisted_write<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    mut request: SendRequest,
    observability: &dyn ObservabilityPort,
    runtime: &R,
    acknowledgement: Option<ResolvedAcknowledgement>,
    delivery_mode: DeliveryExecutionMode,
) -> Result<PreparedWrite, AtmError> {
    let mut context = prepare_send_context(runtime, &request)?;
    let task_id = request.task_id.clone();
    let requires_ack = request_requires_ack(&request, &task_id);
    let body = resolve_message_body(
        &request.message_source,
        &request.current_dir,
        &request.home_dir,
        &context.recipient.team,
        request.max_message_bytes,
    )?;
    annotate_path_only_body(&mut request, &mut context, &body);
    let summary = crate::send::summary::build_summary(&body, request.summary_override.clone());
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
    let received_hook = prepare_received_hook(
        runtime,
        &context,
        &persistence,
        requires_ack,
        acknowledgement.is_some(),
    );
    // A same-host HTTPS receipt deliberately reuses the origin ULID. Storage
    // skips its duplicate row and the receiver-only hook is likewise skipped.
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
        &persistence,
        delivery_mode,
    )?;
    Ok(PreparedWrite {
        outcome,
        outbound_request: request,
        persisted_timestamp: timestamp,
        post_write_needed: persistence.requires_post_write(),
        same_store_peer_receipt: persistence.duplicate_disposition
            == DuplicateWriteDisposition::SameStorePeerReceipt,
        received_hook,
        acknowledgement,
    })
}

/// Prepares the canonical write after its one asynchronous durable admission.
async fn prepare_persisted_write_async(
    mut request: SendRequest,
    observability: &(dyn ObservabilityPort + Send + Sync),
    runtime: &LocalServiceRuntime,
    acknowledgement: Option<ResolvedAcknowledgement>,
) -> Result<PreparedWrite, AtmError> {
    let mut context = prepare_send_context(runtime, &request)?;
    let task_id = request.task_id.clone();
    let requires_ack = request_requires_ack(&request, &task_id);
    let verified_template =
        crate::send::async_persistence::verify_template_request(runtime, &request)?;
    let body = crate::send::async_persistence::resolve_async_body(
        &request,
        &context,
        verified_template.as_ref(),
    )?;
    annotate_path_only_body(&mut request, &mut context, &body);
    let summary = crate::send::summary::build_summary(&body, request.summary_override.clone());
    let message_id = request.origin_message_id.unwrap_or_default();
    let timestamp = request.origin_timestamp.unwrap_or_else(IsoTimestamp::now);
    let persistence = crate::send::async_persistence::persist_send_message_async(
        runtime,
        &request,
        &context,
        &body,
        &summary,
        message_id,
        timestamp,
        requires_ack,
        task_id.clone(),
        verified_template.as_ref(),
    )
    .await?;
    let received_hook = prepare_received_hook(
        runtime,
        &context,
        &persistence,
        requires_ack,
        acknowledgement.is_some(),
    );
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
        &persistence,
        DeliveryExecutionMode::Deferred,
    )?;
    Ok(PreparedWrite {
        outcome,
        outbound_request: request,
        persisted_timestamp: timestamp,
        post_write_needed: persistence.requires_post_write(),
        same_store_peer_receipt: persistence.duplicate_disposition
            == DuplicateWriteDisposition::SameStorePeerReceipt,
        received_hook,
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
pub(crate) fn send_mail_with_runtime_impl<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    request: WriteRequest,
    observability: &dyn ObservabilityPort,
    runtime: &R,
    _post_send_emitter: Option<&dyn MessageReceivedHookEmitter>,
) -> Result<SendOutcome, AtmError> {
    let mut prepared = write_mail_with_runtime_impl(request, observability, runtime)?;
    match prepared.finish_with_runtime(runtime, observability)? {
        WriteOutcome::Sent(outcome) => Ok(outcome),
        WriteOutcome::Acknowledged(_) => Err(AtmError::validation(
            "test send helper cannot finish an acknowledgement write",
        )),
    }
}

/// Immutable acknowledgement outcome assembled by the sealed storage
/// transaction, then completed through the canonical post-write pipeline.
#[derive(Clone)]
pub(crate) struct ResolvedAcknowledgement {
    actor: AgentName,
    team: TeamName,
    reply_target: ReplyTarget,
    reply_text: String,
    acknowledged_message_id: AtmMessageId,
    source_task_id: Option<TaskId>,
}

/// Reply data retained only long enough to form the post-commit route and
/// acknowledgement response.  The source itself is loaded by the storage
/// writer, never by the application-layer admission path.
#[derive(Clone)]
pub(crate) struct AtomicAcknowledgementWrite {
    pub(crate) reply: StoredMessage,
    pub(crate) canonical_request: SendRequest,
    pub(crate) acknowledgement: ResolvedAcknowledgement,
}

#[derive(Clone)]
enum AtomicAcknowledgementKind {
    Local(Box<AckRequest>),
    Received(Box<SendRequest>),
}

struct AtomicAcknowledgementBuilder {
    kind: AtomicAcknowledgementKind,
    // The storage callback requires `&self`; this narrow mutex publishes one
    // immutable reply assembled exactly once across that callback boundary.
    built: Mutex<Option<AtomicAcknowledgementWrite>>,
}

impl AtomicAcknowledgementBuilder {
    fn new(kind: AtomicAcknowledgementKind) -> Self {
        Self {
            kind,
            built: Mutex::new(None),
        }
    }

    fn take(&self) -> Result<AtomicAcknowledgementWrite, AtmError> {
        self.built
            .lock()
            .map_err(|_| {
                AtmError::daemon_unavailable("acknowledgement builder state lock poisoned")
            })?
            .take()
            .ok_or_else(|| {
                AtmError::daemon_unavailable(
                    "acknowledgement transaction completed without a reply",
                )
            })
    }
}

impl AcknowledgementReplyBuilder for AtomicAcknowledgementBuilder {
    fn build_reply(&self, source: &StoredMessage) -> Result<StoredMessage, AtmError> {
        let built = match &self.kind {
            AtomicAcknowledgementKind::Local(request) => {
                let actor = request.caller_identity.clone();
                let team = request.caller_team.clone();
                let reply_target = reply_target_from_source(&source.envelope, &team)?;
                let canonical_request = canonical_ack_write_request(
                    request,
                    &actor,
                    &team,
                    &reply_target,
                    &boundary::Message {
                        team: source.team.clone(),
                        agent: source.agent.clone(),
                        message_key: source.message_key.clone(),
                        envelope: source.envelope.clone(),
                    },
                )?;
                build_atomic_acknowledgement(
                    canonical_request,
                    actor,
                    team,
                    reply_target,
                    request.reply_body.clone(),
                    request.message_id,
                    source.envelope.task_id.clone(),
                )?
            }
            AtomicAcknowledgementKind::Received(request) => {
                let target = request.to.clone().ok_or_else(|| {
                    AtmError::validation("received peer acknowledgement is missing a destination")
                })?;
                let actor = target.agent().clone();
                let team = target
                    .team()
                    .cloned()
                    .unwrap_or_else(|| request.caller_team.clone());
                let reply_target =
                    ReplyTarget::new(actor.clone(), team.clone(), target.host().cloned());
                let message_id = request.acknowledges_message_id.ok_or_else(|| {
                    AtmError::validation("acknowledgement write is missing acknowledges_message_id")
                })?;
                let reply_text = match &request.message_source {
                    SendMessageSource::Inline(value) => value.clone(),
                    SendMessageSource::File { .. } | SendMessageSource::Template(_) => {
                        return Err(AtmError::validation(
                            "acknowledgement reply body must be inline",
                        ));
                    }
                };
                build_atomic_acknowledgement(
                    *request.clone(),
                    actor,
                    team,
                    reply_target,
                    reply_text,
                    message_id,
                    source.envelope.task_id.clone(),
                )?
            }
        };
        let mut slot = self.built.lock().map_err(|_| {
            AtmError::daemon_unavailable("acknowledgement builder state lock poisoned")
        })?;
        *slot = Some(built.clone());
        Ok(built.reply)
    }
}

pub(crate) fn admit_acknowledgement_write<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    request: SendRequest,
    runtime: &R,
) -> Result<AtomicAcknowledgementWrite, AtmError> {
    let provenance = validate_write_provenance(
        if request.to.is_some() {
            WriteIngress::Peer
        } else {
            WriteIngress::Canonical
        },
        WriteProvenance {
            target_host: request.to.as_ref().and_then(|address| address.host()),
            authenticated_source_host: request.authenticated_source_host.as_ref(),
            origin_message_id: request.origin_message_id.is_some(),
            origin_timestamp: request.origin_timestamp.is_some(),
        },
    )?;
    let (source, builder) = if let Some(target) = request.to.as_ref() {
        if !provenance.is_authenticated_peer() {
            return Err(AtmError::validation(
                "acknowledgement write must not include a client-supplied destination",
            ));
        }
        let message_id = request.acknowledges_message_id.ok_or_else(|| {
            AtmError::validation("acknowledgement write is missing acknowledges_message_id")
        })?;
        let team = target
            .team()
            .cloned()
            .unwrap_or_else(|| request.caller_team.clone());
        (
            AcknowledgementSource {
                team,
                agent: target.agent().clone(),
                message_id,
            },
            Arc::new(AtomicAcknowledgementBuilder::new(
                AtomicAcknowledgementKind::Received(Box::new(request)),
            )),
        )
    } else {
        let request = AckRequest::from_unresolved_write(request)?;
        ensure_roster_member_exists(
            runtime,
            &request.caller_team,
            &request.caller_identity,
            "Repair or reload the ATM roster before retrying `atm ack`.",
        )?;
        (
            AcknowledgementSource {
                team: request.caller_team.clone(),
                agent: request.caller_identity.clone(),
                message_id: request.message_id,
            },
            Arc::new(AtomicAcknowledgementBuilder::new(
                AtomicAcknowledgementKind::Local(Box::new(request)),
            )),
        )
    };
    let _commit = runtime.acknowledge_message_atomically(&source, builder.clone())?;
    builder.take()
}

/// Async counterpart of [`admit_acknowledgement_write`] for the replacement
/// Tokio daemon. The roster check remains synchronous core validation; the
/// source lookup, reply creation, and atomic source transition are one await
/// on the storage-owned durable-admission lane.
pub(crate) async fn admit_acknowledgement_write_async(
    request: SendRequest,
    runtime: &LocalServiceRuntime,
) -> Result<AtomicAcknowledgementWrite, AtmError> {
    let provenance = validate_write_provenance(
        if request.to.is_some() {
            WriteIngress::Peer
        } else {
            WriteIngress::Canonical
        },
        WriteProvenance {
            target_host: request.to.as_ref().and_then(|address| address.host()),
            authenticated_source_host: request.authenticated_source_host.as_ref(),
            origin_message_id: request.origin_message_id.is_some(),
            origin_timestamp: request.origin_timestamp.is_some(),
        },
    )?;
    let (source, builder) = if let Some(target) = request.to.as_ref() {
        if !provenance.is_authenticated_peer() {
            return Err(AtmError::validation(
                "acknowledgement write must not include a client-supplied destination",
            ));
        }
        let message_id = request.acknowledges_message_id.ok_or_else(|| {
            AtmError::validation("acknowledgement write is missing acknowledges_message_id")
        })?;
        let team = target
            .team()
            .cloned()
            .unwrap_or_else(|| request.caller_team.clone());
        (
            AcknowledgementSource {
                team,
                agent: target.agent().clone(),
                message_id,
            },
            Arc::new(AtomicAcknowledgementBuilder::new(
                AtomicAcknowledgementKind::Received(Box::new(request)),
            )),
        )
    } else {
        let request = AckRequest::from_unresolved_write(request)?;
        ensure_roster_member_exists(
            runtime,
            &request.caller_team,
            &request.caller_identity,
            "Repair or reload the ATM roster before retrying `atm ack`.",
        )?;
        (
            AcknowledgementSource {
                team: request.caller_team.clone(),
                agent: request.caller_identity.clone(),
                message_id: request.message_id,
            },
            Arc::new(AtomicAcknowledgementBuilder::new(
                AtomicAcknowledgementKind::Local(Box::new(request)),
            )),
        )
    };
    let _commit = runtime
        .acknowledge_message_atomically_async(source, builder.clone())
        .await?;
    builder.take()
}

pub(crate) fn build_atomic_acknowledgement(
    canonical_request: SendRequest,
    actor: AgentName,
    team: TeamName,
    reply_target: ReplyTarget,
    reply_text: String,
    acknowledged_message_id: AtmMessageId,
    source_task_id: Option<TaskId>,
) -> Result<AtomicAcknowledgementWrite, AtmError> {
    if canonical_request.authenticated_source_host.is_none()
        && reply_target.host.is_none()
        && actor == reply_target.agent
        && team == reply_target.team
    {
        return Err(AtmError::self_addressed_send_invalid(format!(
            "self-addressed messages are invalid ATM input: '{actor}@{team}' may not send to itself"
        )));
    }
    let destination = canonical_request.to.as_ref().ok_or_else(|| {
        AtmError::validation("acknowledgement reply is missing a canonical destination")
    })?;
    let message_id = canonical_request.origin_message_id.unwrap_or_default();
    let timestamp = canonical_request
        .origin_timestamp
        .unwrap_or_else(IsoTimestamp::now);
    let summary = crate::send::summary::build_summary(&reply_text, None);
    let mut envelope = InboxMessage {
        from: actor.clone(),
        source_chat_id: canonical_request.caller_chat_id.clone(),
        text: reply_text.clone(),
        timestamp,
        read: false,
        source_team: Some(team.clone()),
        destination_chat_id: destination.chat_id().cloned(),
        summary: Some(summary.clone()),
        message_id: Some(message_id),
        requires_ack: false,
        pending_ack_at: None,
        acknowledged_at: None,
        acknowledges_message_id: Some(acknowledged_message_id),
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        task_id: None,
        extra: serde_json::Map::new(),
    };
    persist_direct_peer_target(&canonical_request, destination, &mut envelope);
    let reply = StoredMessage {
        team: destination.team().cloned().ok_or_else(|| {
            AtmError::validation("acknowledgement reply destination is missing a team")
        })?,
        agent: destination.agent().clone(),
        message_key: MessageKey::from(message_id),
        envelope,
    };
    let acknowledgement = ResolvedAcknowledgement {
        actor,
        team,
        reply_target,
        reply_text,
        acknowledged_message_id,
        source_task_id,
    };
    Ok(AtomicAcknowledgementWrite {
        reply,
        canonical_request,
        acknowledgement,
    })
}

fn persist_direct_peer_target(
    canonical_request: &SendRequest,
    destination: &crate::address::AgentAddress,
    envelope: &mut InboxMessage,
) {
    if let Some(host) = crate::send::direct_peer_destination(canonical_request, destination) {
        crate::schema::set_peer_delivery_target(envelope, &host);
    }
}

fn reply_target_from_source(
    source: &InboxMessage,
    current_team: &TeamName,
) -> Result<ReplyTarget, AtmError> {
    let team = source
        .source_team
        .clone()
        .unwrap_or_else(|| current_team.clone());
    let agent = crate::threading::canonical_sender_identity(source);
    Ok(ReplyTarget::new(agent, team, reply_target_host(source)?))
}

impl ResolvedAcknowledgement {
    pub(crate) fn source_task_id(&self) -> Option<TaskId> {
        self.source_task_id.clone()
    }

    /// Receiver-side acknowledgement mutation occurs only after the canonical
    /// write succeeded. A failed write leaves the source pending.
    pub(crate) fn finish<R: RetainedMailboxRuntime>(
        self,
        _runtime: &R,
        observability: &dyn ObservabilityPort,
        send_outcome: SendOutcome,
    ) -> Result<AckOutcome, AtmError> {
        let outcome = AckOutcome {
            action: CommandAction::Ack,
            team: self.team.clone(),
            agent: self.actor.clone(),
            message_id: self.acknowledged_message_id,
            task_id: self.source_task_id.clone(),
            reply_disposition: AckReplyDisposition::Sent {
                reply_message_id: send_outcome.message_id,
                reply_target: self.reply_target,
            },
            reply_text: self.reply_text,
            warnings: send_outcome.warnings,
        };
        record_ack_telemetry(
            observability,
            &self.actor,
            self.team,
            outcome.message_id,
            outcome.task_id.clone(),
        );
        Ok(outcome)
    }
}

pub(crate) fn canonical_ack_write_request(
    request: &AckRequest,
    actor: &AgentName,
    team: &TeamName,
    target: &ReplyTarget,
    source: &boundary::Message,
) -> Result<SendRequest, AtmError> {
    Ok(SendRequest {
        home_dir: request.home_dir.clone(),
        current_dir: request.current_dir.clone(),
        caller_identity: actor.clone(),
        caller_chat_id: request.caller_chat_id.clone(),
        caller_team: team.clone(),
        activity_observation: request.activity_observation.clone(),
        authenticated_source_host: None,
        origin_message_id: None,
        origin_timestamp: None,
        to: Some(crate::address::AgentAddress::new(
            target.agent.clone(),
            source.envelope.source_chat_id.clone(),
            Some(target.team.clone()),
            target.host.clone(),
        )?),
        message_source: SendMessageSource::Inline(request.reply_body.clone()),
        classification: crate::send::MessageClassification::default(),
        max_message_bytes: crate::send::input::default_message_max_bytes(),
        summary_override: None,
        requires_ack: false,
        task_id: None,
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        acknowledges_message_id: Some(request.message_id),
        dry_run: false,
    })
}

fn ensure_roster_member_exists<R: RetainedServiceRuntime>(
    runtime: &R,
    team: &TeamName,
    agent: &AgentName,
    recovery: &str,
) -> Result<(), AtmError> {
    if runtime.load_roster_member(team, agent)?.is_none() {
        return Err(AtmError::new(
            crate::error_codes::AtmErrorCode::AgentNotFound,
            format!("agent '{agent}' was not found in team '{team}'\n  Recovery: {recovery}"),
        ));
    }
    Ok(())
}

pub(crate) fn reply_target_host(
    source: &InboxMessage,
) -> Result<Option<crate::types::HostName>, AtmError> {
    let authenticated = authenticated_source_host(source)?;
    let outbound = peer_delivery_target(source)?;
    let validated = validate_write_provenance(
        WriteIngress::Canonical,
        WriteProvenance {
            target_host: outbound.as_ref(),
            authenticated_source_host: authenticated.as_ref(),
            origin_message_id: authenticated.is_some(),
            origin_timestamp: authenticated.is_some(),
        },
    )?;
    Ok(validated
        .is_authenticated_peer()
        .then_some(authenticated)
        .flatten()
        .or(outbound))
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
