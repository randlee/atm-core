//! Shared durable-write context and outcome planning.

use std::path::PathBuf;

use crate::delivery_plan::{
    DeliveryPlan, delivery_plan_disposition, logical_messages_from_persistence,
};
use crate::delivery_policy::{
    DeliveryEventFamily, DeliveryPolicyCoordinator, DeliveryRecipientSnapshot,
};
use crate::error::AtmError;
use crate::schema::AtmMessageId;
use crate::service_runtime::RetainedServiceRuntime;
use crate::service_runtime_store::RetainedMailboxRuntime;
use crate::types::{AgentName, CommandAction, TaskId};

use super::{
    DeliveryPersistenceResult, ResolvedRecipient, SendCommandOutcome, SendOutcome, SendRequest,
    WarningEntry, resolve_recipient, validate_non_self_recipient,
};

#[expect(
    clippy::too_many_arguments,
    reason = "Y.6 closeout keeps the explicit send outcome fields aligned with the command contract."
)]
pub(super) fn build_send_outcome(
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

pub(crate) struct SendExecutionContext {
    pub(crate) recipient: ResolvedRecipient,
    pub(crate) canonical_sender: AgentName,
    pub(crate) inbox_path: PathBuf,
    pub(crate) delivery_snapshot: DeliveryRecipientSnapshot,
    pub(crate) delivery_family: DeliveryEventFamily,
    pub(crate) warnings: Vec<WarningEntry>,
}

pub(crate) fn prepare_send_context<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    runtime: &R,
    request: &SendRequest,
) -> Result<SendExecutionContext, AtmError> {
    // This is the durable-admission half of the pipeline. A daemon must not
    // inspect caller workspace or hook configuration before a durable reply.
    let warnings = Vec::new();
    let canonical_sender = request.caller_identity.clone();
    let target = request.to.as_ref().ok_or_else(|| {
        AtmError::validation("write request destination must be resolved before persistence")
    })?;
    let provenance = crate::provenance::validate_write_provenance(
        crate::provenance::WriteIngress::Canonical,
        crate::provenance::WriteProvenance {
            target_host: target.host(),
            authenticated_source_host: request.authenticated_source_host.as_ref(),
            origin_message_id: request.origin_message_id.is_some(),
            origin_timestamp: request.origin_timestamp.is_some(),
        },
    )?;
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
        recipient,
        canonical_sender,
        inbox_path,
        delivery_snapshot,
        delivery_family,
        warnings,
    })
}
