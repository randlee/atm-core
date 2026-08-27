//! The canonical write pipeline: `PreparedWrite`, the `send_mail`/`write_mail`
//! entry family, and persisted-write preparation.

use super::*;
use crate::send::NudgeMode;

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
    ///
    /// This deliberately does not set the `NudgeMode::Deferred` queue marker:
    /// callers that route through an async boundary (e.g.
    /// `StorageAndNudgeRouter`) must schedule [`PreparedWrite::mark_pending_if_deferred`]
    /// on their own blocking task after this returns. Callers with no such
    /// boundary should use [`PreparedWrite::finish_and_mark`] instead.
    pub fn finish(
        &mut self,
        runtime: &LocalServiceRuntime,
        observability: &dyn ObservabilityPort,
    ) -> Result<WriteOutcome, AtmError> {
        self.finish_with_runtime(runtime, observability)
    }

    /// Completes the canonical write and, for a newly persisted
    /// `NudgeMode::Deferred` message, sets its durable queue marker in the
    /// same call.
    ///
    /// This is the entry point for the synchronous public write API
    /// (`write_mail_with_runtime`/`send_mail_with_runtime`/`ack_mail_with_runtime`):
    /// those callers are already blocking, so performing the marker's
    /// blocking SQLite transaction inline here never lands on an async
    /// runtime worker. Callers that route through an async boundary must
    /// instead call [`PreparedWrite::finish`] and schedule
    /// [`PreparedWrite::mark_pending_if_deferred`] on their own blocking task.
    pub fn finish_and_mark(
        &mut self,
        runtime: &LocalServiceRuntime,
        observability: &dyn ObservabilityPort,
    ) -> Result<WriteOutcome, AtmError> {
        let outcome = self.finish(runtime, observability)?;
        self.mark_pending_if_deferred(runtime);
        Ok(outcome)
    }

    /// Sets the durable at-most-once queue marker for a newly persisted
    /// `NudgeMode::Deferred` write.
    ///
    /// The caller must invoke this from a blocking task. The pending-store
    /// contract is synchronous because its concrete SQLite implementation
    /// owns a blocking transaction; the Tokio router supplies that task
    /// boundary after async message admission completes.
    pub fn mark_pending_if_deferred(&self, runtime: &LocalServiceRuntime) {
        if !self.is_newly_persisted() || self.outbound_request.nudge_mode != NudgeMode::Deferred {
            return;
        }
        let message_id = self.persisted_message_id();
        let post_write = match &self.received_hook {
            Ok(Some(post_write)) => post_write,
            Ok(None) => {
                tracing::warn!(
                    subsystem = "atm_core.queue",
                    action = "queue_marker_set",
                    outcome = "failed",
                    message_id = %message_id,
                    "deferred write has no retained recipient to resolve a queue marker for"
                );
                return;
            }
            Err(error) => {
                tracing::warn!(
                    subsystem = "atm_core.queue",
                    action = "queue_marker_set",
                    outcome = "failed",
                    message_id = %message_id,
                    %error,
                    "deferred write receiver-hook planning failed before queue marker resolution"
                );
                return;
            }
        };
        let member = boundary::MemberKey::new(
            post_write.recipient.team.clone(),
            post_write.recipient.agent.clone(),
        );
        let store = match runtime.pending_nudge_store() {
            Ok(store) => store,
            Err(error) => {
                tracing::warn!(
                    subsystem = "atm_core.queue",
                    action = "queue_marker_set",
                    outcome = "failed",
                    message_id = %message_id,
                    member = %member,
                    %error,
                    "deferred write has no pending-nudge store installed in this runtime"
                );
                return;
            }
        };
        if let Err(error) = store.mark_pending(&member, &message_id, self.persisted_timestamp) {
            tracing::warn!(
                subsystem = "atm_core.queue",
                action = "queue_marker_set",
                outcome = "failed",
                message_id = %message_id,
                member = %member,
                %error,
                "failed to set the deferred-nudge queue marker"
            );
        }
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
        if self.outbound_request.nudge_mode == NudgeMode::Deferred {
            tracing::info!(
                subsystem = "atm_core.queue",
                action = "steer_suppressed",
                outcome = "ok",
                message_id = %self.persisted_message_id(),
                "deferred write suppresses its immediate receiver steer"
            );
            return Ok(Vec::new());
        }
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
                self.outbound_request.nudge_mode,
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
    prepared.finish_and_mark(runtime, observability)
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
    .finish_and_mark(runtime, observability)?
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
