use serde_json::Map;

use super::*;
use crate::schema::{AckIntentFields, InboxMessage, set_remote_host};

pub(super) struct SendExecutionContext {
    pub(super) command_config: Option<config::AtmConfig>,
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
    let delivery_policy = DeliveryPolicyCoordinator::new();
    let delivery_snapshot =
        delivery_policy.resolve_recipient_snapshot(runtime, &recipient.team, &recipient.agent)?;
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
) -> Result<DeliveryPersistenceResult, AtmError> {
    let ack_intent = AckIntentFields::from_requires_ack(requires_ack, timestamp);
    if request.dry_run {
        let mut envelope = InboxMessage {
            from: context.canonical_sender.clone(),
            text: body.to_string(),
            timestamp,
            read: false,
            source_team: Some(request.caller_team.clone()),
            summary: Some(summary.to_string()),
            message_id: Some(message_id),
            requires_ack: ack_intent.requires_ack,
            pending_ack_at: ack_intent.pending_ack_at,
            acknowledged_at: ack_intent.acknowledged_at,
            acknowledges_message_id: None,
            parent_message_id: request.parent_message_id,
            thread_mode: request.thread_mode,
            expires_at: request.expires_at,
            task_id: task_id.clone(),
            extra: Map::new(),
        };
        if let Some(remote_host) = request.source_remote_host.as_deref() {
            set_remote_host(&mut envelope, remote_host);
        }
        return Ok(DeliveryPersistenceResult::persisted(envelope));
    }
    let mut envelope = InboxMessage {
        from: context.canonical_sender.clone(),
        text: body.to_string(),
        timestamp,
        read: false,
        source_team: Some(request.caller_team.clone()),
        summary: Some(summary.to_string()),
        message_id: Some(message_id),
        requires_ack: ack_intent.requires_ack,
        pending_ack_at: ack_intent.pending_ack_at,
        acknowledged_at: ack_intent.acknowledged_at,
        acknowledges_message_id: None,
        parent_message_id: request.parent_message_id,
        thread_mode: request.thread_mode,
        expires_at: request.expires_at,
        task_id: task_id.clone(),
        extra: Map::new(),
    };
    if let Some(remote_host) = request.source_remote_host.as_deref() {
        set_remote_host(&mut envelope, remote_host);
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
