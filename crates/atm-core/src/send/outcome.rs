use super::*;

pub(super) struct SendExecutionContext {
    #[cfg(test)]
    pub(super) post_send_config: Option<config::AtmConfig>,
    pub(super) recipient: ResolvedRecipient,
    pub(super) canonical_sender: AgentName,
    pub(super) inbox_path: PathBuf,
    pub(super) delivery_snapshot: DeliveryRecipientSnapshot,
    pub(super) delivery_family: DeliveryEventFamily,
    pub(super) warnings: Vec<WarningEntry>,
}

pub(super) fn prepare_send_context<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    runtime: &R,
    request: &SendRequest,
    provenance: ValidatedWriteProvenance,
) -> Result<SendExecutionContext, AtmError> {
    // This is the durable-admission half of the pipeline.  A daemon must not
    // inspect a caller workspace or hook configuration before replying to a
    // committed write: those are post-commit worker concerns.  The daemon
    // runtime already rejects workspace config, but avoiding this call here
    // makes the no-filesystem-read admission contract structural rather than
    // dependent on the runtime implementation.
    #[cfg(test)]
    let post_send_config = None;
    let warnings = Vec::new();
    let canonical_sender = request.caller_identity.clone();
    let target = request.to.as_ref().ok_or_else(|| {
        AtmError::validation("write request destination must be resolved before persistence")
    })?;
    let recipient = resolve_recipient(target, &request.caller_team, None)?;
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
pub(super) fn persist_send_message<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
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
        set_peer_reply_host(&mut envelope, host);
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
pub(super) fn build_send_envelope(
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

#[expect(
    clippy::too_many_arguments,
    reason = "Y.6 closeout keeps the explicit send outcome pieces visible at the sprint seam."
)]
pub(super) fn finalize_send_outcome<
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
    persistence: &DeliveryPersistenceResult,
    delivery_mode: DeliveryExecutionMode,
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
        persistence,
    );
    if !request.dry_run && delivery_mode == DeliveryExecutionMode::Inline {
        let plan = build_send_delivery_plan(context, requires_ack, false, persistence)?;
        let execution = execute_delivery_plan(runtime, None, &plan)?;
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

pub(super) fn build_send_delivery_plan(
    context: &SendExecutionContext,
    requires_ack: bool,
    is_ack: bool,
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
        logical_messages_from_persistence(persistence, requires_ack, is_ack)
            .map_err(|error| AtmError::mailbox_write(error.to_string()))?,
        persistence.warnings.clone(),
    ))
}

#[cfg(test)]
pub(super) fn post_send_messages_from_persistence(
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
