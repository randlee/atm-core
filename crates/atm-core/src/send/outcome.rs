use tracing::warn;

use super::*;
use crate::delivery_execution::{
    DeliveryTransitionContext, emit_delivery_plan_transitions, execute_delivery_plan,
};
use crate::delivery_plan::{DeliveryPlan, logical_messages_from_persistence};
use crate::observability::{CommandEvent, action_name, outcome_label};

#[expect(
    clippy::too_many_arguments,
    reason = "Y.6 closeout keeps the explicit send outcome pieces visible at the sprint seam."
)]
pub(super) fn finalize_send_outcome<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    runtime: &R,
    observability: &dyn ObservabilityPort,
    post_send_emitter: Option<&dyn PostSendHookEmitter>,
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
    let effective_message_id = persistence
        .original_message
        .message_id
        .unwrap_or(message_id);
    let mut outcome = build_send_outcome(
        context,
        body,
        summary,
        effective_message_id,
        requires_ack,
        task_id.clone(),
        command_outcome,
        &persistence,
    );
    if !request.dry_run
        && !matches!(
            persistence.disposition,
            DeliveryPersistenceDisposition::AlreadyPersisted
        )
    {
        let is_ack = request.acknowledges_message_id.is_some();
        let post_send_messages =
            post_send_messages_from_persistence(&persistence, requires_ack, is_ack)?;
        hook::emit_post_send_effects(
            runtime,
            &mut outcome.warnings,
            context.post_send_config.as_ref(),
            post_send_emitter,
            &context.recipient,
            &context.delivery_snapshot,
            &post_send_messages,
        );
        let plan = build_send_delivery_plan(context, requires_ack, is_ack, &persistence)?;
        let execution = execute_delivery_plan(runtime, context.command_config.as_ref(), &plan)?;
        emit_delivery_plan_transitions(
            observability,
            DeliveryTransitionContext {
                family: context.delivery_family,
                team: &context.recipient.team,
                agent: &context.recipient.agent,
                sender: &context.canonical_sender,
                message_id: effective_message_id,
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
    reason = "AG.10 keeps the persisted-send outcome ingredients explicit at the seam while the module split lands."
)]
fn build_send_outcome(
    context: &SendExecutionContext,
    body: &str,
    summary: &str,
    message_id: AtmMessageId,
    requires_ack: bool,
    task_id: Option<TaskId>,
    command_outcome: SendCommandOutcome,
    persistence: &DeliveryPersistenceResult,
) -> SendOutcome {
    SendOutcome {
        action: CommandAction::Send,
        team: context.recipient.team.clone(),
        agent: context.recipient.agent.clone(),
        sender: context.canonical_sender.clone(),
        outcome: command_outcome,
        message_id,
        receipt_message_id: None,
        requires_ack,
        task_id,
        summary: Some(summary.to_string()),
        message: Some(body.to_string()),
        warnings: context
            .warnings
            .iter()
            .cloned()
            .chain(persistence.warnings.clone())
            .collect(),
        dry_run: matches!(command_outcome, SendCommandOutcome::DryRun),
    }
}

pub(super) fn build_send_delivery_plan(
    context: &SendExecutionContext,
    requires_ack: bool,
    is_ack: bool,
    persistence: &DeliveryPersistenceResult,
) -> Result<DeliveryPlan, AtmError> {
    Ok(DeliveryPlan::new(
        crate::delivery_plan::DeliveryPlanKind::Send,
        persistence.disposition,
        crate::delivery_plan::delivery_target_for_snapshot(
            &context.inbox_path,
            &context.delivery_snapshot,
        ),
        context.recipient.clone(),
        logical_messages_from_persistence(persistence, requires_ack, is_ack).map_err(|error| {
            AtmError::mailbox_write(error.to_string()).with_recovery(
                "Repair the persisted delivery record shape before retrying delivery-plan execution.",
            )
        })?,
        persistence.warnings.clone(),
    ))
}

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
    .map_err(|error| {
        AtmError::mailbox_write(error.to_string()).with_recovery(
            "Repair the persisted delivery record shape before retrying post-send emission.",
        )
    })
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
