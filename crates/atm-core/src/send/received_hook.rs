//! In-memory received-hook planning retained across durable admission.

use crate::delivery_plan::LogicalMessage;
use crate::delivery_policy::{DeliveryPolicyCoordinator, DeliveryRecipientSnapshot};
use crate::error::AtmError;
use crate::service_runtime::RetainedServiceRuntime;

use super::{
    DeliveryPersistenceResult, ResolvedRecipient, SendExecutionContext, build_send_delivery_plan,
};

/// Post-commit data retained by the replacement runtime without reloading the
/// just-persisted SQLite record.
pub(crate) struct PreparedReceivedHook {
    pub(crate) recipient: ResolvedRecipient,
    pub(crate) delivery_snapshot: DeliveryRecipientSnapshot,
    pub(crate) messages: Vec<LogicalMessage>,
    pub(crate) reassign_notice: Option<PreparedReassignNotice>,
}

pub(crate) struct PreparedReassignNotice {
    pub(crate) recipient: ResolvedRecipient,
    pub(crate) delivery_snapshot: DeliveryRecipientSnapshot,
    pub(crate) message: LogicalMessage,
}

pub(crate) fn prepare_received_hook<R: RetainedServiceRuntime + ?Sized>(
    runtime: &R,
    context: &SendExecutionContext,
    persistence: &DeliveryPersistenceResult,
    requires_ack: bool,
    is_ack: bool,
) -> Result<Option<PreparedReceivedHook>, AtmError> {
    if !persistence.requires_post_write() {
        return Ok(None);
    }
    // An origin write to a host-qualified address keeps a remote snapshot for
    // admission, because its actual delivery belongs to the peer client. The
    // historical post-commit hook still resolves this local recipient if the
    // record was admitted here (notably the localhost compatibility path).
    let delivery_snapshot = if context.delivery_snapshot.roster_backed {
        context.delivery_snapshot.clone()
    } else {
        DeliveryPolicyCoordinator::new().resolve_recipient_snapshot(
            runtime,
            &context.recipient.team,
            &context.recipient.agent,
        )?
    };
    let hook_context = SendExecutionContext {
        recipient: context.recipient.clone(),
        canonical_sender: context.canonical_sender.clone(),
        inbox_path: context.inbox_path.clone(),
        delivery_snapshot: delivery_snapshot.clone(),
        delivery_family: context.delivery_family,
        warnings: Vec::new(),
    };
    let plan = build_send_delivery_plan(&hook_context, requires_ack, is_ack, persistence)?;
    let reassign_notice =
        persistence
            .reassign_notice
            .as_ref()
            .map(|notice| {
                let recipient = ResolvedRecipient {
                    team: notice.team.clone(),
                    agent: notice.agent.clone(),
                };
                let delivery_snapshot = DeliveryPolicyCoordinator::new()
                    .resolve_recipient_snapshot(runtime, &recipient.team, &recipient.agent)?;
                let message = LogicalMessage::new(notice.envelope.clone(), false, false, None)
                    .map_err(|error| AtmError::validation(error.to_string()))?
                    .with_task_transition(crate::boundary::TaskTransition::Closed {
                        outcome: crate::boundary::TaskClosedOutcome::Reassigned,
                    });
                Ok::<PreparedReassignNotice, AtmError>(PreparedReassignNotice {
                    recipient,
                    delivery_snapshot,
                    message,
                })
            })
            .transpose()?;
    Ok(Some(PreparedReceivedHook {
        recipient: context.recipient.clone(),
        delivery_snapshot,
        messages: plan.messages,
        reassign_notice,
    }))
}
