//! The canonical write pipeline: `PreparedWrite`, the `send_mail`/`write_mail`
//! entry family, and persisted-write preparation.

use super::pipeline_terminal_escalation::{
    TerminalEscalationMail, terminal_episode, terminal_escalation_details,
    write_terminal_escalations,
};
use super::*;
use crate::boundary::MemberKey;
use crate::send::async_persistence::validate_async_write_admission;
use crate::send::{NudgeMode, WriteSourcePreflight, send_mode_for_task_request};

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

/// A committed operation result and its exact task-ledger audit rows.
pub type WriteExecution = atm_storage::CommittedTaskWrite<PreparedWrite>;

fn retain_successful_execution(execution: WriteExecution) -> Result<PreparedWrite, AtmError> {
    let mut prepared = execution.operation?;
    prepared.task_events = execution.task_events;
    Ok(prepared)
}

/// A durable write awaiting post-commit notification scheduling.
///
/// Acknowledgement state completes before any post-commit notification or peer
/// work. Delivery is never a prerequisite for an admitted local write.
pub struct PreparedWrite {
    outcome: SendOutcome,
    outbound_request: WriteRequest,
    persisted_timestamp: IsoTimestamp,
    post_write_needed: bool,
    same_store_peer_receipt: bool,
    received_hook: Result<Option<PreparedReceivedHook>, AtmError>,
    acknowledgement: Option<ResolvedAcknowledgement>,
    task_rejection: Option<AtmError>,
    task_events: Vec<atm_storage::TaskEventRow>,
}

impl PreparedWrite {
    fn from_persistence(
        outcome: SendOutcome,
        request: SendRequest,
        timestamp: IsoTimestamp,
        persistence: &crate::send::DeliveryPersistenceResult,
        received_hook: Result<Option<PreparedReceivedHook>, AtmError>,
        acknowledgement: Option<ResolvedAcknowledgement>,
    ) -> Self {
        Self {
            outcome,
            outbound_request: request,
            persisted_timestamp: timestamp,
            post_write_needed: persistence.requires_post_write(),
            // Same-host receipts reuse the origin ULID; duplicate storage and
            // receiver-only hooks have already been skipped by admission.
            same_store_peer_receipt: persistence.duplicate_disposition
                == DuplicateWriteDisposition::SameStorePeerReceipt,
            received_hook,
            acknowledgement,
            task_rejection: persistence.task_rejection.clone(),
            task_events: Vec::new(),
        }
    }

    /// Takes the exact task-ledger rows committed by this successful write.
    pub fn take_task_events(&mut self) -> Vec<atm_storage::TaskEventRow> {
        std::mem::take(&mut self.task_events)
    }

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
    /// A task rejection whose report was retained as plain mail is likewise
    /// available through [`PreparedWrite::task_rejection`] only after the
    /// caller completes ordinary post-write delivery.
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
        let outcome = self.finish_with_runtime(runtime, observability)?;
        if let Err(error) = self.mark_pending_if_deferred(runtime) {
            tracing::warn!(
                message_id = %self.persisted_message_id(),
                %error,
                "synchronous deferred-write queue marker failed after durable write"
            );
        }
        self.reject_task_operation_if_needed()?;
        Ok(outcome)
    }

    /// Returns the task rejection that must be surfaced only after ordinary
    /// post-write delivery completes.
    #[must_use]
    pub fn task_rejection(&self) -> Option<AtmError> {
        self.task_rejection.clone()
    }

    fn reject_task_operation_if_needed(&self) -> Result<(), AtmError> {
        match &self.task_rejection {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    /// Sets the durable at-most-once queue marker for a newly persisted
    /// `NudgeMode::Deferred` write.
    ///
    /// The caller must invoke this from a blocking task. The pending-store
    /// contract is synchronous because its concrete SQLite implementation
    /// owns a blocking transaction; the Tokio router supplies that task
    /// boundary after async message admission completes.
    pub fn mark_pending_if_deferred(&self, runtime: &LocalServiceRuntime) -> Result<(), AtmError> {
        if !self.is_newly_persisted() || self.outbound_request.nudge_mode != NudgeMode::Deferred {
            return Ok(());
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
                return Err(AtmError::new(
                    crate::error_codes::AtmErrorCode::InternalError,
                    "deferred write has no retained recipient",
                ));
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
                return Err(error.clone());
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
                return Err(error.clone());
            }
        };
        store
            .mark_pending(&member, &message_id, self.persisted_timestamp)
            .map(|_| ())
            .map_err(|error| {
                tracing::warn!(
                    subsystem = "atm_core.queue",
                    action = "queue_marker_set",
                    outcome = "failed",
                    message_id = %message_id,
                    member = %member,
                    %error,
                    "failed to set the deferred-nudge queue marker"
                );
                error
            })
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
        // Queue is a deferred notification for local steer backends, but the
        // graft recipient's queue-shaped channel is itself the handoff. The
        // selector owns the channel-specific emitter; retain AQ1's
        // suppression for every other backend until its downstream trigger
        // sprint supplies that emitter.
        let suppress_primary = self.outbound_request.nudge_mode == NudgeMode::Deferred
            && !post_write.delivery_snapshot.graft_post_send
            && !post_write.delivery_snapshot.bare_cli_post_send;
        if suppress_primary {
            tracing::info!(
                subsystem = "atm_core.queue",
                action = "steer_suppressed",
                outcome = "ok",
                message_id = %self.persisted_message_id(),
                "deferred write suppresses its immediate receiver steer"
            );
        }
        let mut dispatches = Vec::new();
        for message in post_write.messages.iter().filter(|_| !suppress_primary) {
            let event = crate::send::hook::post_send_event_from_message(
                &post_write.recipient,
                message,
                post_write.delivery_snapshot.recipient_pane_id.as_ref(),
            )?;
            let dispatch = crate::send::hook::build_built_in_dispatch(
                runtime,
                &post_write.delivery_snapshot,
                &event,
                self.outbound_request.nudge_mode,
            )?;
            if let Some(dispatch) = dispatch {
                dispatches.push(dispatch);
            }
        }
        if let Some(notice) = &post_write.reassign_notice {
            let event = crate::send::hook::post_send_event_from_message(
                &notice.recipient,
                &notice.message,
                notice.delivery_snapshot.recipient_pane_id.as_ref(),
            )?;
            if let Some(dispatch) = crate::send::hook::build_built_in_dispatch(
                runtime,
                &notice.delivery_snapshot,
                &event,
                NudgeMode::Immediate,
            )? {
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
    )?
    .operation?;
    prepared.finish_and_mark(runtime, observability)
}

/// Outcome of a core-owned terminal Herdr escalation admission.
///
/// The caller receives durable message identifiers for audit only. It never
/// supplies a task, runtime episode, recipient, body, summary, or provenance
/// marker; those are derived from the authoritative runtime while its roster
/// admission guard is held.
#[derive(Debug, Clone, Default)]
pub struct TerminalHerdrEscalationOutcome {
    pub deliveries: Vec<(AgentName, AtmMessageId)>,
    pub recipients_failed: u32,
}

impl TerminalHerdrEscalationOutcome {
    #[must_use]
    pub fn recipients_written(&self) -> u32 {
        self.deliveries.len().try_into().unwrap_or(u32::MAX)
    }
}

/// Emits the current member's terminal Blocked or Offline escalation.
///
/// This is deliberately the only public terminal-provenance entry point. Its
/// member key selects authoritative state; core derives the episode, open
/// tasks, escalation recipients, rendered body and summary before it stamps
/// the internal marker. Ordinary [`WriteRequest`] values cannot enter this
/// path and always retain normal queue-marker behaviour.
pub fn write_current_herdr_terminal_escalation_with_runtime(
    daemon_home: std::path::PathBuf,
    source_member: &MemberKey,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
) -> Result<TerminalHerdrEscalationOutcome, AtmError> {
    let team = source_member.team().clone();
    let task_store = runtime.task_store()?;
    runtime.with_roster_member_runtime_admission(
        source_member.team(),
        source_member.agent(),
        |current| {
            let current = current.ok_or_else(|| {
                AtmError::validation(
                    "Herdr escalation admission member is absent from the runtime roster",
                )
            })?;
            let (state_name, started_at) = terminal_episode(&current.runtime)?;
            let (recipients, summary, body) = terminal_escalation_details(
                runtime,
                task_store.as_ref(),
                source_member,
                &team,
                state_name,
                started_at,
            )?;
            write_terminal_escalations(
                daemon_home,
                recipients,
                team,
                TerminalEscalationMail {
                    body,
                    summary,
                    started_at,
                },
                observability,
                runtime,
            )
        },
    )
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
    .operation?
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
    .and_then(retain_successful_execution)
}

/// Prepares one canonical write through the Tokio durable-admission boundary.
///
/// Core validation and response construction remain shared with blocking
/// library callers. The immutable storage transition enqueues work
/// to the backend's bounded writer lane and receives that lane's durable
/// result without a blocking task in the HTTP runtime.
pub async fn prepare_write_with_preflight_async_runtime(
    request: WriteRequest,
    observability: &(dyn ObservabilityPort + Send + Sync),
    runtime: &LocalServiceRuntime,
    source_preflight: WriteSourcePreflight,
) -> Result<WriteExecution, AtmError> {
    prepare_write_with_admission_async_runtime(
        request,
        observability,
        runtime,
        source_preflight,
        None,
        None,
        None,
    )
    .await
}

/// Persists a write whose virtual sender and stable identity were admitted by
/// a signed local runtime proof or authenticated peer attestation. The proof
/// itself remains in the transport request; only this non-serializable
/// capability authorizes canonical sender/id projection.
pub async fn prepare_write_with_tool_capability_async_runtime(
    request: WriteRequest,
    capability: crate::boundary::ToolSenderCapability,
    observability: &(dyn ObservabilityPort + Send + Sync),
    runtime: &LocalServiceRuntime,
    source_preflight: WriteSourcePreflight,
) -> Result<WriteExecution, AtmError> {
    prepare_write_with_admission_async_runtime(
        request,
        observability,
        runtime,
        source_preflight,
        Some(capability),
        None,
        None,
    )
    .await
}

/// Builds the internal storage provenance only from the scheduler-owned
/// capability. Callers cannot turn caller-supplied schedule ids into a fence.
#[must_use]
pub fn cron_run_message_write_origin(
    permit: &crate::boundary::CronRunPermit,
) -> atm_storage::MessageWriteOrigin {
    atm_storage::MessageWriteOrigin::CronRun {
        schedule_id: permit.schedule_id(),
        generation: permit.generation(),
    }
}

/// Persists a cron action through canonical admission with the opaque
/// scheduler permit retained until the storage provenance is constructed.
pub async fn prepare_write_with_cron_run_permit_async_runtime(
    request: WriteRequest,
    permit: &crate::boundary::CronRunPermit,
    observability: &(dyn ObservabilityPort + Send + Sync),
    runtime: &LocalServiceRuntime,
    source_preflight: WriteSourcePreflight,
) -> Result<WriteExecution, AtmError> {
    prepare_write_with_admission_async_runtime(
        request,
        observability,
        runtime,
        source_preflight,
        None,
        Some((permit.schedule_id(), permit.generation())),
        None,
    )
    .await
}

/// Persists cron module output as ordinary creator-addressed mail, retaining
/// both the scheduler capability and the id allocated by the transition
/// store. The stable id never enters the serializable write request.
pub async fn prepare_cron_module_mail_async_runtime(
    request: WriteRequest,
    permit: &crate::boundary::CronRunPermit,
    message_id: AtmMessageId,
    observability: &(dyn ObservabilityPort + Send + Sync),
    runtime: &LocalServiceRuntime,
    source_preflight: WriteSourcePreflight,
) -> Result<WriteExecution, AtmError> {
    if request.acknowledges_message_id.is_some()
        || request.task_id.is_some()
        || request.task_op.is_some()
        || request.task_complete.is_some()
    {
        return Err(AtmError::validation(
            "cron module output cannot be an acknowledgement or task operation",
        ));
    }
    prepare_write_with_admission_async_runtime(
        request,
        observability,
        runtime,
        source_preflight,
        None,
        Some((permit.schedule_id(), permit.generation())),
        Some(message_id),
    )
    .await
}

async fn prepare_write_with_admission_async_runtime(
    request: WriteRequest,
    observability: &(dyn ObservabilityPort + Send + Sync),
    runtime: &LocalServiceRuntime,
    source_preflight: WriteSourcePreflight,
    tool_capability: Option<crate::boundary::ToolSenderCapability>,
    cron_run_fence: Option<(atm_storage::CronScheduleId, atm_storage::CronGeneration)>,
    stable_message_id: Option<AtmMessageId>,
) -> Result<WriteExecution, AtmError> {
    validate_async_write_admission(
        &request,
        tool_capability.is_some(),
        cron_run_fence,
        stable_message_id,
    )?;
    if request.acknowledges_message_id.is_none() || has_authenticated_peer_provenance(&request) {
        return prepare_persisted_write_async(
            request,
            observability,
            runtime,
            PersistedWritePreparation {
                acknowledgement: None,
                source_preflight,
                tool_capability,
                cron_run_fence,
                stable_message_id,
            },
        )
        .await;
    }
    let origin = cron_run_fence.map_or(
        atm_storage::MessageWriteOrigin::Local,
        |(schedule_id, generation)| atm_storage::MessageWriteOrigin::CronRun {
            schedule_id,
            generation,
        },
    );
    let admitted = admit_acknowledgement_write_async(request, runtime, origin).await?;
    let task_events = admitted.task_events;
    match admitted.operation {
        Ok(acknowledgement) => {
            prepare_atomic_acknowledgement_write(acknowledgement, observability, runtime).map(
                |prepared| WriteExecution {
                    operation: Ok(prepared),
                    task_events,
                },
            )
        }
        Err(error) => Ok(WriteExecution {
            operation: Err(error),
            task_events,
        }),
    }
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
) -> Result<WriteExecution, AtmError> {
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
    prepare_atomic_acknowledgement_write(acknowledgement, observability, runtime).map(|prepared| {
        WriteExecution {
            operation: Ok(prepared),
            task_events: Vec::new(),
        }
    })
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
    .and_then(retain_successful_execution)
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
        task_complete: None,
        already_closed: None,
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
    let logical =
        crate::delivery_plan::LogicalMessage::new(reply.envelope.clone(), false, true, None)
            .map_err(|error| AtmError::mailbox_read(error.to_string()))?;
    let received_hook = Ok(Some(PreparedReceivedHook {
        recipient: recipient.clone(),
        delivery_snapshot: delivery_snapshot.clone(),
        messages: vec![logical.clone()],
        reassign_notice: None,
    }));
    Ok(PreparedWrite {
        outcome,
        outbound_request: acknowledgement.canonical_request,
        persisted_timestamp: reply.envelope.timestamp,
        post_write_needed: true,
        same_store_peer_receipt: false,
        received_hook,
        acknowledgement: Some(acknowledgement.acknowledgement),
        task_rejection: None,
        task_events: Vec::new(),
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
) -> Result<WriteExecution, AtmError> {
    let mut context = prepare_send_context(runtime, &mut request)?;
    crate::send::validate_task_request(&mut request)?;
    let task_id = request.task_id.clone();
    request.nudge_mode = send_mode_for_task_request(&request, &task_id);
    let self_task = mark_self_task(&mut request, &context);
    let is_ack = acknowledgement.is_some();
    let delivery_mode = delivery_mode_for(self_task, delivery_mode);
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
    let mut persistence = persist_send_message(
        runtime,
        &request,
        &context,
        &body,
        &summary,
        message_id,
        timestamp,
        requires_ack,
        task_id.clone(),
        None,
    )?;
    let task_events = std::mem::take(&mut persistence.task_events);
    if let Err(error) = &persistence.operation {
        return Ok(WriteExecution {
            operation: Err(error.clone()),
            task_events,
        });
    }
    let received_hook = if self_task {
        Ok(None)
    } else {
        prepare_received_hook(runtime, &context, &persistence, requires_ack, is_ack)
    };
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
    );
    Ok(WriteExecution {
        operation: outcome.map(|outcome| {
            PreparedWrite::from_persistence(
                outcome,
                request,
                timestamp,
                &persistence,
                received_hook,
                acknowledgement,
            )
        }),
        task_events,
    })
}

/// A task-linked write from a member to itself records the task and nothing
/// else: the nudge is never built and no delivery is planned for the caller.
fn mark_self_task(request: &mut SendRequest, context: &crate::send::SendExecutionContext) -> bool {
    let self_task = crate::send::is_task_write(request)
        && crate::send::is_same_member(
            &context.canonical_sender,
            &request.caller_team,
            &context.recipient,
        );
    if self_task {
        request.nudge_mode = NudgeMode::Immediate;
    }
    self_task
}

fn delivery_mode_for(self_task: bool, mode: DeliveryExecutionMode) -> DeliveryExecutionMode {
    if self_task {
        DeliveryExecutionMode::Deferred
    } else {
        mode
    }
}

/// Verifies that an opaque admission still matches the exact proof and canonical
/// body carried into persistence, then selects its immutable message id.
fn verified_tool_message_id(
    request: &WriteRequest,
    capability: &crate::boundary::ToolSenderCapability,
    body: &str,
) -> Result<AtmMessageId, AtmError> {
    let actual_digest = atm_storage::MessageDigest::for_message_body(body);
    if capability.message_digest() != &actual_digest {
        return Err(AtmError::new(
            crate::error_codes::AtmErrorCode::ToolSignatureInvalid,
            "canonical message body does not match the admitted tool proof digest",
        ));
    }

    match (
        capability.is_peer_admission(),
        request.tool_runtime_proof.as_ref(),
        request.tool_sender_attestation.as_ref(),
    ) {
        (false, Some(proof), None) if capability.matches_runtime_proof(proof) => {
            if request.origin_message_id.is_some()
                || request.origin_timestamp.is_some()
                || request.authenticated_source_host.is_some()
            {
                return Err(AtmError::validation(
                    "local tool proof cannot override peer origin identity or timestamp",
                ));
            }
        }
        (true, None, Some(attestation)) if capability.matches_peer_attestation(attestation) => {
            if request.origin_message_id != Some(capability.message_id()) {
                return Err(AtmError::validation(
                    "peer tool proof identity does not match authenticated origin metadata",
                ));
            }
            if request.authenticated_source_host.as_ref() != capability.authenticated_source_host()
            {
                return Err(atm_storage::ToolSenderFailure::HostMismatch.into_error(
                    "peer tool proof host does not match authenticated origin metadata",
                ));
            }
        }
        _ => {
            return Err(AtmError::new(
                crate::error_codes::AtmErrorCode::ToolSignatureInvalid,
                "tool admission capability does not match the exact proof in the write request",
            ));
        }
    }
    Ok(capability.message_id())
}

/// Prepares the canonical write after its one asynchronous durable admission.
struct PersistedWritePreparation {
    acknowledgement: Option<ResolvedAcknowledgement>,
    source_preflight: WriteSourcePreflight,
    tool_capability: Option<crate::boundary::ToolSenderCapability>,
    cron_run_fence: Option<(atm_storage::CronScheduleId, atm_storage::CronGeneration)>,
    stable_message_id: Option<AtmMessageId>,
}

async fn prepare_persisted_write_async(
    mut request: SendRequest,
    observability: &(dyn ObservabilityPort + Send + Sync),
    runtime: &LocalServiceRuntime,
    preparation: PersistedWritePreparation,
) -> Result<WriteExecution, AtmError> {
    let PersistedWritePreparation {
        acknowledgement,
        source_preflight,
        tool_capability,
        cron_run_fence,
        stable_message_id,
    } = preparation;
    let mut context = crate::send::prepare_send_context_with_tool_capability(
        runtime,
        &mut request,
        tool_capability.as_ref(),
    )?;
    crate::send::validate_task_request(&mut request)?;
    let task_id = request.task_id.clone();
    request.nudge_mode = send_mode_for_task_request(&request, &task_id);
    let self_task = mark_self_task(&mut request, &context);
    let is_ack = acknowledgement.is_some();
    let requires_ack = request_requires_ack(&request, &task_id);
    let (body, verified_template) =
        crate::send::async_persistence::resolve_async_body(&request, source_preflight)?;
    let message_id = match tool_capability.as_ref() {
        Some(capability) => verified_tool_message_id(&request, capability, &body)?,
        None => stable_message_id
            .or(request.origin_message_id)
            .unwrap_or_default(),
    };
    annotate_path_only_body(&mut request, &mut context, &body);
    let summary = crate::send::summary::build_summary(&body, request.summary_override.clone());
    let timestamp = if tool_capability
        .as_ref()
        .is_some_and(|capability| !capability.is_peer_admission())
    {
        IsoTimestamp::now()
    } else {
        stable_message_id
            .map(|id| id.timestamp())
            .unwrap_or_else(|| request.origin_timestamp.unwrap_or_else(IsoTimestamp::now))
    };
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
        cron_run_fence,
    )
    .await?;
    Ok(finish_async_persisted_write(
        request,
        observability,
        runtime,
        acknowledgement,
        AsyncPersistedSendCompletion {
            context,
            task_id,
            self_task,
            is_ack,
            requires_ack,
            body,
            message_id,
            summary,
            timestamp,
        },
        persistence,
    ))
}

fn finish_async_persisted_write(
    request: SendRequest,
    observability: &(dyn ObservabilityPort + Send + Sync),
    runtime: &LocalServiceRuntime,
    acknowledgement: Option<ResolvedAcknowledgement>,
    prepared: AsyncPersistedSendCompletion,
    mut persistence: crate::send::DeliveryPersistenceResult,
) -> WriteExecution {
    let AsyncPersistedSendCompletion {
        context,
        task_id,
        self_task,
        is_ack,
        requires_ack,
        body,
        message_id,
        summary,
        timestamp,
        ..
    } = prepared;
    let task_events = std::mem::take(&mut persistence.task_events);
    if let Err(error) = &persistence.operation {
        return WriteExecution {
            operation: Err(error.clone()),
            task_events,
        };
    }
    let received_hook = if self_task {
        Ok(None)
    } else {
        prepare_received_hook(runtime, &context, &persistence, requires_ack, is_ack)
    };
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
    );
    WriteExecution {
        operation: outcome.map(|outcome| {
            PreparedWrite::from_persistence(
                outcome,
                request,
                timestamp,
                &persistence,
                received_hook,
                acknowledgement,
            )
        }),
        task_events,
    }
}

struct AsyncPersistedSendCompletion {
    context: crate::send::SendExecutionContext,
    task_id: Option<TaskId>,
    self_task: bool,
    is_ack: bool,
    requires_ack: bool,
    body: String,
    message_id: AtmMessageId,
    summary: String,
    timestamp: IsoTimestamp,
}

pub(crate) fn has_authenticated_peer_provenance(request: &WriteRequest) -> bool {
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

#[cfg(test)]
mod tool_sender_admission_tests {
    use std::str::FromStr;

    use super::verified_tool_message_id;
    use crate::boundary::{ToolSenderCapability, claim_verified_tool_runtime};
    use crate::send::{SendMessageSource, WriteRequest};
    use crate::types::{AgentName, TeamName};
    use atm_storage::testing::InMemoryToolSenderRegistry;
    use atm_storage::{
        AtmMessageId, HostName, InstallerId, MessageDigest, PluginSenderId, ToolInstallerOperation,
        ToolInstallerRequest, ToolKeyId, ToolNonce, ToolRuntimeProof, ToolSenderRegistry,
    };

    const MESSAGE_ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    const BODY: &str = "deterministic tool mail";

    fn admitted() -> (ToolSenderCapability, ToolRuntimeProof) {
        let sender = PluginSenderId::parse("test-plugin").expect("sender");
        let key_id = ToolKeyId::parse("runtime-key").expect("runtime key");
        let host = HostName::from_str("host.example").expect("host");
        let now = crate::types::IsoTimestamp::now();
        let registry = InMemoryToolSenderRegistry::default();
        registry
            .apply_signed(
                ToolInstallerRequest {
                    schema_version: "1.0.0".into(),
                    operation: ToolInstallerOperation::Register,
                    installer_id: InstallerId::parse("installer").expect("installer"),
                    installer_key_id: ToolKeyId::parse("installer-key").expect("installer key"),
                    sender: sender.clone(),
                    package_digest: MessageDigest::parse("package-digest").expect("package digest"),
                    registration_generation: 1,
                    host_id: host.clone(),
                    runtime_key_id: key_id.clone(),
                    runtime_public_key: "test-key".into(),
                    previous_runtime_key_id: None,
                    nonce: ToolNonce::parse("install-nonce").expect("installer nonce"),
                    issued_at: now,
                    expires_at: now,
                    signature: "test-signature".into(),
                },
                now,
            )
            .expect("register deterministic fixture");

        let message_id = AtmMessageId::from_str(MESSAGE_ID).expect("message id");
        let digest = MessageDigest::for_message_body(BODY);
        let proof = ToolRuntimeProof {
            runtime_key_id: key_id,
            sender,
            registration_generation: 1,
            host_id: host,
            message_id,
            message_digest: digest.clone(),
            nonce: ToolNonce::parse("runtime-nonce").expect("runtime nonce"),
            signature: "runtime-signature".into(),
        };
        let admission = registry
            .admit_signed_message(proof.clone(), message_id, digest, now)
            .expect("admit deterministic fixture");
        let claim = claim_verified_tool_runtime(admission);
        let crate::boundary::SenderClaim::DaemonTool {
            proof: capability, ..
        } = claim
        else {
            panic!("admission yields a daemon tool claim");
        };
        (capability, proof)
    }

    fn request(proof: ToolRuntimeProof) -> WriteRequest {
        let mut request = WriteRequest::new(
            std::path::PathBuf::from("/tmp"),
            std::path::PathBuf::from("/tmp"),
            AgentName::from_validated("daemon"),
            "recipient@team",
            TeamName::from_validated("team"),
            SendMessageSource::Inline(BODY.into()),
            None,
            false,
            None,
            false,
        )
        .expect("valid fixed request");
        request.tool_runtime_proof = Some(proof);
        request
    }

    #[test]
    fn exact_admitted_tool_proof_keeps_the_bound_message_id_on_retry() {
        let (capability, proof) = admitted();
        let request = request(proof);
        let first = verified_tool_message_id(&request, &capability, BODY).expect("first write");
        let retry = verified_tool_message_id(&request, &capability, BODY).expect("exact retry");
        assert_eq!(first, retry);
        assert_eq!(first.to_string(), MESSAGE_ID);
    }

    #[test]
    fn tool_admission_rejects_changed_body_and_substituted_proof() {
        let (capability, proof) = admitted();
        let original_request = request(proof.clone());
        assert!(verified_tool_message_id(&original_request, &capability, "changed body").is_err());

        let mut changed_id = proof.clone();
        changed_id.message_id =
            AtmMessageId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FAW").expect("alternate id");
        assert!(verified_tool_message_id(&request(changed_id), &capability, BODY).is_err());

        let mut changed_sender = proof;
        changed_sender.sender = PluginSenderId::parse("other-plugin").expect("other sender");
        assert!(verified_tool_message_id(&request(changed_sender), &capability, BODY).is_err());
    }

    #[test]
    fn local_tool_admission_rejects_origin_identity_overrides() {
        let (capability, proof) = admitted();
        let mut request = request(proof);
        request.origin_message_id = Some(capability.message_id());
        assert!(verified_tool_message_id(&request, &capability, BODY).is_err());
    }
}
