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
    BuiltInPostSendDispatch, MemberKey, MessageKey, NudgeKind, PostSendHookEvent,
};
use crate::delivery_policy::DeliveryPolicyCoordinator;
use crate::error::AtmError;
use crate::schema::{AtmMessageId, authenticated_source_host};
use crate::send::NudgeMode;
use crate::send::hook::build_built_in_dispatch;
use crate::service_runtime::LocalServiceRuntime;
use atm_storage::TaskRow;

/// Clears the exact durable queue marker after a successful handoff.
///
/// Marker cleanup is deliberately best-effort: the handoff has already
/// succeeded, so cleanup must never turn that success into a failed delivery.
/// A failed clear is logged and retried once. `record_failure` is invoked for
/// each failed attempt so the composition layer can project the failure into
/// its runtime-health counters without making core depend on that layer.
pub fn clear_queue_marker_after_handoff(
    service_runtime: &LocalServiceRuntime,
    member: &MemberKey,
    message_id: &AtmMessageId,
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
    if let Err(error) = store.clear_pending_on_handoff(member, message_id) {
        record_failure();
        tracing::warn!(
            subsystem = "atm_core.queue",
            action = "handoff_marker_clear",
            outcome = "failed",
            %error,
            msg_id = %message_id,
            "queue delivery succeeded but pending marker clear failed; retrying"
        );
        if let Err(retry_error) = store.clear_pending_on_handoff(member, message_id) {
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

/// Rebuilds the receiver-hook dispatch for one already-persisted message.
///
/// `kind` selects the rebuilt dispatch's [`NudgeKind`] (a queue claim always
/// rebuilds `Queue`; a diagnostic replay may request `Steer`). Returns
/// `Ok(None)` when the message does not exist, is not addressed to `member`,
/// or resolves to no first-party delivery capability for the recipient —
/// the same conditions under which the write-time planner omits a dispatch.
///
/// # Errors
///
/// Returns [`AtmError`] if the message store or roster lookups fail, or if
/// the recipient is no longer present in the roster.
pub fn rebuild_received_hook_dispatch(
    runtime: &LocalServiceRuntime,
    member: &MemberKey,
    message_id: AtmMessageId,
    kind: NudgeKind,
) -> Result<Option<BuiltInPostSendDispatch>, AtmError> {
    let key = MessageKey::from(message_id);
    let Some(message) = runtime
        .message_store
        .load_message(&key)?
        .filter(|message| &message.team == member.team() && &message.agent == member.agent())
    else {
        return Ok(None);
    };

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
        recipient_pane_id: delivery_snapshot.recipient_pane_id.clone(),
    };

    let nudge_mode = match kind {
        NudgeKind::Steer => NudgeMode::Immediate,
        NudgeKind::Queue => NudgeMode::Deferred,
    };

    Ok(build_built_in_dispatch(
        runtime,
        &delivery_snapshot,
        &event,
        &message.envelope.text,
        nudge_mode,
    ))
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
        recipient_pane_id: delivery_snapshot.recipient_pane_id.clone(),
    };
    Ok(build_built_in_dispatch(
        runtime,
        &delivery_snapshot,
        &event,
        &row.description,
        NudgeMode::Deferred,
    ))
}
