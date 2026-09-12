//! Rebuilds a receiver-hook dispatch from durable message-store state.
//!
//! [`PreparedWrite::build_received_hook_dispatches`](crate::send::PreparedWrite::build_received_hook_dispatches)
//! is the write-time planner: it deliberately never reloads the just-persisted
//! record. This module is the one, explicitly separate, reload path used to
//! replay a durable at-most-once queue claim (`atm queue`, AQ2/AQ3) into the
//! same public [`BuiltInPostSendDispatch`] shape a write-time dispatch would
//! have produced. Living outside `send` keeps that invariant checkable by
//! construction: the write-time planner module never imports the message
//! store reload helper this module wraps.

use crate::boundary::{
    BuiltInPostSendDispatch, MemberKey, Message, MessageKey, NudgeKind, PostSendHookEvent,
    TaskTransition,
};
use crate::delivery_policy::DeliveryPolicyCoordinator;
use crate::error::AtmError;
use crate::schema::{AtmMessageId, authenticated_source_host};
use crate::send::NudgeMode;
use crate::send::hook::build_built_in_dispatch;
use crate::service_runtime::LocalServiceRuntime;
use atm_storage::TaskRow;

const fn claim_task_transition() -> Option<TaskTransition> {
    None
}

const fn task_pass_transition(reminder_count: u32) -> TaskTransition {
    if reminder_count == 0 {
        TaskTransition::Ready
    } else {
        TaskTransition::Reminder {
            attempt: reminder_count,
        }
    }
}

/// Re-arms the exact durable queue marker after a successful handoff.
///
/// Marker cleanup is deliberately best-effort: the handoff has already
/// succeeded, so cleanup must never turn that success into a failed delivery.
/// A failed clear is logged and retried once. `record_failure` is invoked for
/// each failed attempt so the composition layer can project the failure into
/// its runtime-health counters without making core depend on that layer.
pub fn rearm_queue_marker_after_handoff(
    service_runtime: &LocalServiceRuntime,
    member: &MemberKey,
    message_id: &AtmMessageId,
    next_due: atm_storage::types::IsoTimestamp,
    mut record_failure: impl FnMut(),
) {
    let store = match service_runtime.pending_nudge_store() {
        Ok(store) => store,
        Err(error) => {
            record_failure();
            tracing::warn!(
                subsystem = "atm_core.queue",
                action = "handoff_marker_clear",
                outcome = "failed",
                %error,
                msg_id = %message_id,
                "queue delivery succeeded but pending marker store was unavailable"
            );
            return;
        }
    };
    if let Err(error) = store.rearm_pending_after_handoff(member, message_id, next_due) {
        record_failure();
        tracing::warn!(
            subsystem = "atm_core.queue",
            action = "handoff_marker_clear",
            outcome = "failed",
            %error,
            msg_id = %message_id,
            "queue delivery succeeded but pending marker clear failed; retrying"
        );
        if let Err(retry_error) = store.rearm_pending_after_handoff(member, message_id, next_due) {
            record_failure();
            tracing::warn!(
                subsystem = "atm_core.queue",
                action = "handoff_marker_clear",
                outcome = "failed",
                %retry_error,
                msg_id = %message_id,
                "pending marker clear retry failed after successful queue delivery"
            );
        }
    }
}

/// Loads one queued message for receiver-hook dispatch reconstruction.
///
/// Composition owns this durable read and must place it on the reader path
/// before passing the returned message to [`rebuild_received_hook_dispatch`].
pub fn load_received_hook_dispatch_message(
    runtime: &LocalServiceRuntime,
    member: &MemberKey,
    message_id: AtmMessageId,
) -> Result<Option<Message>, AtmError> {
    let key = MessageKey::from(message_id);
    Ok(runtime
        .message_store
        .load_message(&key)?
        .filter(|message| &message.team == member.team() && &message.agent == member.agent()))
}

/// Rebuilds the receiver-hook dispatch for one already-loaded message.
///
/// `kind` selects the rebuilt dispatch's [`NudgeKind`] (a queue claim always
/// rebuilds `Queue`; a diagnostic replay may request `Steer`). Returns
/// `Ok(None)` when the loaded message resolves to no first-party delivery
/// capability for the recipient — the same condition under which the
/// write-time planner omits a dispatch.
///
/// # Errors
///
/// Returns [`AtmError`] if the recipient is no longer present in the roster.
pub fn rebuild_received_hook_dispatch(
    runtime: &LocalServiceRuntime,
    member: &MemberKey,
    message_id: AtmMessageId,
    kind: NudgeKind,
    message: &Message,
) -> Result<Option<BuiltInPostSendDispatch>, AtmError> {
    if &message.team != member.team() || &message.agent != member.agent() {
        return Ok(None);
    }
    let delivery_snapshot = DeliveryPolicyCoordinator::new().resolve_recipient_snapshot(
        runtime,
        member.team(),
        member.agent(),
    )?;
    // Mapping mirrors `send::hook::post_send_event_from_message`
    // (write-time), adapted to the persisted `Message` shape returned by a
    // reload instead of the in-memory `LogicalMessage` retained across a
    // single write.
    let event = PostSendHookEvent {
        sender: message.envelope.from.clone(),
        sender_chat_id: message.envelope.source_chat_id.clone(),
        sender_team: message
            .envelope
            .source_team
            .clone()
            .unwrap_or_else(|| member.team().clone()),
        sender_host: crate::schema::authenticated_source_host(&message.envelope)?,
        recipient: member.agent().clone(),
        recipient_team: member.team().clone(),
        message_id,
        description: message
            .envelope
            .summary
            .clone()
            .filter(|summary| !summary.trim().is_empty())
            .unwrap_or_else(|| message.envelope.text.clone()),
        requires_ack: message.envelope.requires_ack,
        is_ack: message.envelope.acknowledges_message_id.is_some(),
        task_id: message.envelope.task_id.clone(),
        task_transition: claim_task_transition(),
        recipient_pane_id: delivery_snapshot.recipient_pane_id.clone(),
    };

    let nudge_mode = match kind {
        NudgeKind::Steer => NudgeMode::Immediate,
        NudgeKind::Queue => NudgeMode::Deferred,
    };

    build_built_in_dispatch(runtime, &delivery_snapshot, &event, nudge_mode)
}

/// Builds a deferred Task reminder without requiring the assignment message
/// to remain in the mailbox. A missing assignment record only removes the
/// optional source-host attribution; the durable task row remains sufficient
/// to render the reminder body.
pub fn build_task_reminder_dispatch(
    runtime: &LocalServiceRuntime,
    member: &MemberKey,
    row: &TaskRow,
) -> Result<Option<BuiltInPostSendDispatch>, AtmError> {
    let delivery_snapshot = DeliveryPolicyCoordinator::new().resolve_recipient_snapshot(
        runtime,
        member.team(),
        member.agent(),
    )?;
    let sender_host = runtime
        .message_store
        .load_message(&MessageKey::from(row.assignment_message_id))?
        .map(|message| authenticated_source_host(&message.envelope))
        .transpose()?
        .flatten();
    let event = PostSendHookEvent {
        sender: row.assigner.clone(),
        sender_chat_id: None,
        sender_team: row.team.clone(),
        sender_host,
        recipient: row.assignee.clone(),
        recipient_team: row.team.clone(),
        message_id: row.assignment_message_id,
        description: row.description.clone(),
        requires_ack: true,
        is_ack: false,
        task_id: Some(row.task_id.clone()),
        task_transition: Some(task_pass_transition(row.reminder_count)),
        recipient_pane_id: delivery_snapshot.recipient_pane_id.clone(),
    };
    build_built_in_dispatch(runtime, &delivery_snapshot, &event, NudgeMode::Deferred)
}

#[cfg(test)]
mod tests {
    use super::{claim_task_transition, task_pass_transition};
    use crate::boundary::{
        BuiltInNudgeTemplateKind, NudgeKind, PostSendHookEvent, TaskTransition,
        built_in_nudge_template_kind_from_post_send_event,
    };
    use crate::schema::AtmMessageId;
    use crate::types::{AgentName, TeamName};

    #[test]
    fn task_pass_builder_sets_ready_at_zero_reminders_then_reminder_with_count() {
        assert_eq!(task_pass_transition(0), TaskTransition::Ready);
        assert_eq!(
            task_pass_transition(3),
            TaskTransition::Reminder { attempt: 3 }
        );
    }

    #[test]
    fn claim_builder_never_sets_task_transition() {
        for task_id in [None, Some("BB.1".parse().expect("task id"))] {
            let event = PostSendHookEvent {
                sender: AgentName::from_validated("sender"),
                sender_chat_id: None,
                sender_team: TeamName::from_validated("team"),
                sender_host: None,
                recipient: AgentName::from_validated("recipient"),
                recipient_team: TeamName::from_validated("team"),
                message_id: AtmMessageId::new(),
                description: "test".to_owned(),
                requires_ack: false,
                is_ack: false,
                task_id,
                task_transition: claim_task_transition(),
                recipient_pane_id: None,
            };
            assert_eq!(event.task_transition, None);
            assert_eq!(
                built_in_nudge_template_kind_from_post_send_event(&event, NudgeKind::Steer),
                Ok(BuiltInNudgeTemplateKind::Delivery)
            );
        }
    }
}
