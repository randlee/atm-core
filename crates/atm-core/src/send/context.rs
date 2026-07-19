use serde_json::Map;

use super::*;
use crate::schema::{AckIntentFields, InboxMessage, set_remote_host};

#[expect(
    clippy::too_many_arguments,
    reason = "Canonical outbound envelope construction needs the full persisted message contract in one place."
)]
pub(crate) fn build_outbound_envelope(
    sender: AgentName,
    source_team: TeamName,
    body: String,
    summary: String,
    message_id: AtmMessageId,
    timestamp: IsoTimestamp,
    ack_intent: AckIntentFields,
    acknowledges_message_id: Option<AtmMessageId>,
    parent_message_id: Option<AtmMessageId>,
    thread_mode: Option<ThreadMode>,
    expires_at: Option<IsoTimestamp>,
    task_id: Option<TaskId>,
    remote_host: Option<&str>,
) -> InboxMessage {
    let mut envelope = InboxMessage {
        from: sender,
        text: body,
        timestamp,
        read: false,
        source_team: Some(source_team),
        summary: Some(summary),
        message_id: Some(message_id),
        requires_ack: ack_intent.requires_ack,
        pending_ack_at: ack_intent.pending_ack_at,
        acknowledged_at: ack_intent.acknowledged_at,
        acknowledges_message_id,
        parent_message_id,
        thread_mode,
        expires_at,
        task_id,
        extra: Map::new(),
    };
    if let Some(remote_host) = remote_host {
        set_remote_host(&mut envelope, remote_host);
    }
    envelope
}

pub(crate) struct SendExecutionContext {
    pub(super) command_config: Option<config::AtmConfig>,
    pub(super) post_send_config: Option<config::AtmConfig>,
    pub(super) recipient: ResolvedRecipient,
    pub(super) canonical_sender: AgentName,
    pub(crate) inbox_path: PathBuf,
    pub(crate) delivery_snapshot: DeliveryRecipientSnapshot,
    pub(super) delivery_family: DeliveryEventFamily,
    pub(super) warnings: Vec<WarningEntry>,
}

fn effective_remote_host(request: &SendRequest) -> Option<&str> {
    request
        .source_remote_host
        .as_deref()
        .or(request.remote_host.as_ref().map(RemoteTargetHost::as_str))
}

pub(crate) fn prepare_send_context<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    runtime: &R,
    request: &SendRequest,
) -> Result<SendExecutionContext, AtmError> {
    let command_config = runtime.load_config(&request.current_dir)?;
    let (post_send_config, warnings) = match hook::load_post_send_config_for_sender(
        runtime,
        &request.caller_team,
        &request.caller_identity,
    ) {
        Ok(config) => (config, Vec::new()),
        Err(error) => (
            None,
            vec![WarningEntry::with_code(
                error.code,
                format!(
                    "warning: post-send hook config lookup failed for {}@{}: {}.",
                    request.caller_identity, request.caller_team, error.message
                ),
                error.primary_recovery().map(str::to_owned),
            )],
        ),
    };
    let canonical_sender = request.caller_identity.clone();
    let recipient = resolve_recipient(&request.to, &request.caller_team, command_config.as_ref())?;
    if request.remote_host.is_none() && request.source_remote_host.is_none() {
        validate_non_self_recipient(&canonical_sender, &request.caller_team, &recipient)?;
    }
    let inbox_path = runtime.inbox_path(&request.home_dir, &recipient.team, &recipient.agent)?;
    let delivery_snapshot = if let Some(remote_host) = request.remote_host.as_ref() {
        DeliveryRecipientSnapshot::remote_non_claude(
            recipient.team.clone(),
            recipient.agent.clone(),
            remote_host.as_str().to_string(),
        )
    } else {
        let delivery_policy = DeliveryPolicyCoordinator::new();
        delivery_policy.resolve_recipient_snapshot(runtime, &recipient.team, &recipient.agent)?
    };
    let delivery_family = DeliveryPolicyCoordinator::resolve_send_family(
        request.parent_message_id,
        request.thread_mode,
    );
    Ok(SendExecutionContext {
        command_config,
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
// AG.20 migration: return the persisted canonical envelope directly; remove DeliveryPersistenceResult.
pub(crate) fn persist_send_message<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    request: &SendRequest,
    context: &SendExecutionContext,
    body: &str,
    summary: &str,
    message_id: AtmMessageId,
    timestamp: IsoTimestamp,
    requires_ack: bool,
    task_id: Option<TaskId>,
) -> Result<DeliveryPersistenceResult, AtmError> {
    let ack_intent = AckIntentFields::from_requires_ack(requires_ack, timestamp);
    let envelope = build_outbound_envelope(
        context.canonical_sender.clone(),
        request.caller_team.clone(),
        body.to_string(),
        summary.to_string(),
        message_id,
        timestamp,
        ack_intent,
        request.acknowledges_message_id,
        request.parent_message_id,
        request.thread_mode,
        request.expires_at,
        task_id.clone(),
        effective_remote_host(request),
    );
    if request.dry_run {
        return Ok(DeliveryPersistenceResult::persisted(envelope));
    }
    persist_message(
        runtime,
        &request.home_dir,
        &context.delivery_snapshot,
        &context.inbox_path,
        &envelope,
        false,
    )
}
