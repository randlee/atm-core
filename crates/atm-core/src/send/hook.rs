use tracing::warn;

use super::{ResolvedRecipient, nudge_template};
use crate::boundary::{
    BuiltInPostSendDispatch, GraftNudgeTarget, HerdrNudgeTarget, LocalSteerTarget,
    LocalTmuxNudgeTarget, NudgeKind, PostSendBuiltInTarget, PostSendHookEvent, QueuePullTarget,
    built_in_nudge_template_kind_from_post_send_event,
};
use crate::delivery_policy::DeliveryRecipientSnapshot;
use crate::error::AtmError;
use crate::send::NudgeMode;
use crate::service_runtime::RetainedServiceRuntime;

/// Builds the built-in receiver dispatch planned after a durable write.
///
/// Delivery execution belongs to `atm-http-runtime`; this core helper only
/// transforms the persisted message and resolved recipient policy into the
/// public dispatch value consumed by that runtime.
pub(crate) fn build_built_in_dispatch<R>(
    runtime: &R,
    delivery_snapshot: &DeliveryRecipientSnapshot,
    event: &PostSendHookEvent,
    nudge_mode: NudgeMode,
) -> Result<Option<BuiltInPostSendDispatch>, AtmError>
where
    R: RetainedServiceRuntime + ?Sized,
{
    let kind = nudge_kind_for_mode(nudge_mode);
    if delivery_snapshot.local_tmux_post_send {
        let pane_id = tmux_pane_id(event, delivery_snapshot);
        let Some(pane_id) = pane_id else {
            return Ok(None);
        };
        let Some(rendered_nudge) = render_built_in_nudge_for_dispatch(runtime, event, kind)? else {
            return Ok(None);
        };
        return Ok(Some(BuiltInPostSendDispatch {
            event: event.clone(),
            target: PostSendBuiltInTarget::LocalSteer(LocalSteerTarget::Tmux(
                LocalTmuxNudgeTarget {
                    pane_id,
                    rendered_nudge,
                },
            )),
            kind,
        }));
    }
    if delivery_snapshot.local_herdr_post_send {
        return build_herdr_dispatch(runtime, delivery_snapshot, event, kind);
    }
    if delivery_snapshot.graft_post_send {
        let Some(rendered_nudge) = render_built_in_nudge_for_dispatch(runtime, event, kind)? else {
            return Ok(None);
        };
        return Ok(Some(BuiltInPostSendDispatch {
            event: event.clone(),
            target: PostSendBuiltInTarget::Graft(GraftNudgeTarget {
                recipient: event.recipient.clone(),
                recipient_team: event.recipient_team.clone(),
                rendered_nudge,
            }),
            kind,
        }));
    }
    if delivery_snapshot.bare_cli_post_send {
        let Some(rendered_nudge) = render_built_in_nudge_for_dispatch(runtime, event, kind)? else {
            return Ok(None);
        };
        return Ok(Some(BuiltInPostSendDispatch {
            event: event.clone(),
            target: PostSendBuiltInTarget::QueuePull(QueuePullTarget {
                team: event.recipient_team.clone(),
                agent: event.recipient.clone(),
                kind,
                msg_id: event.message_id,
                body: rendered_nudge,
            }),
            kind,
        }));
    }
    Ok(None)
}

fn build_herdr_dispatch<R>(
    runtime: &R,
    delivery_snapshot: &DeliveryRecipientSnapshot,
    event: &PostSendHookEvent,
    kind: NudgeKind,
) -> Result<Option<BuiltInPostSendDispatch>, AtmError>
where
    R: RetainedServiceRuntime + ?Sized,
{
    let Some(agent) = crate::delivery_channel::resolve_herdr_agent_target(
        &event.recipient,
        delivery_snapshot.herdr_agent.clone(),
        "post_send_herdr_dispatch",
    ) else {
        return Ok(None);
    };
    let Some(rendered_nudge) = render_built_in_nudge_for_dispatch(runtime, event, kind)? else {
        return Ok(None);
    };
    Ok(Some(BuiltInPostSendDispatch {
        event: event.clone(),
        target: PostSendBuiltInTarget::LocalSteer(LocalSteerTarget::Herdr(HerdrNudgeTarget {
            agent,
            session: delivery_snapshot.herdr_session.clone(),
            rendered_nudge,
        })),
        kind,
    }))
}

fn tmux_pane_id(
    event: &PostSendHookEvent,
    delivery_snapshot: &DeliveryRecipientSnapshot,
) -> Option<crate::types::PaneId> {
    event
        .recipient_pane_id
        .clone()
        .or_else(|| delivery_snapshot.recipient_pane_id.as_ref().cloned())
}

/// Maps the write-time delivery mode to the dispatch's `NudgeKind`.
///
/// `Immediate` writes always build a `Steer` dispatch; `Deferred` writes are
/// suppressed before reaching this helper except when the caller (queue
/// rebuild, L2.4) explicitly rebuilds a `Queue` dispatch for durable replay.
const fn nudge_kind_for_mode(nudge_mode: NudgeMode) -> NudgeKind {
    match nudge_mode {
        NudgeMode::Immediate => NudgeKind::Steer,
        NudgeMode::Deferred => NudgeKind::Queue,
    }
}

/// Render the database-resolved built-in nudge once for every first-party
/// delivery sink. Tmux, Herdr, and graft therefore receive identical XML text.
fn render_built_in_nudge_for_dispatch<R>(
    runtime: &R,
    event: &PostSendHookEvent,
    delivery_kind: NudgeKind,
) -> Result<Option<String>, AtmError>
where
    R: RetainedServiceRuntime + ?Sized,
{
    let kind = built_in_nudge_template_kind_from_post_send_event(event, delivery_kind);
    let override_row = match runtime.load_nudge_template_override(&event.recipient_team, kind) {
        Ok(row) => row,
        Err(error) => {
            warn!(
                code = %error.code(),
                recipient = %event.recipient,
                recipient_team = %event.recipient_team,
                message_id = %event.message_id,
                %error,
                "failed to load built-in nudge template override; falling back to default"
            );
            None
        }
    };
    let template = nudge_template::resolve_template(override_row, kind);
    let Some(template_body) = template.body.as_deref() else {
        return Ok(None);
    };
    nudge_template::render_built_in_nudge(event, template_body).map(Some)
}

pub(crate) fn post_send_event_from_message(
    recipient: &ResolvedRecipient,
    message: &crate::delivery_plan::LogicalMessage,
    recipient_pane_id: Option<&crate::types::PaneId>,
) -> Result<PostSendHookEvent, AtmError> {
    Ok(PostSendHookEvent {
        sender: message.envelope.from.clone(),
        sender_chat_id: message.envelope.source_chat_id.clone(),
        sender_team: message
            .envelope
            .source_team
            .clone()
            .unwrap_or_else(|| recipient.team.clone()),
        sender_host: crate::schema::authenticated_source_host(&message.envelope)?,
        recipient: recipient.agent.clone(),
        recipient_team: recipient.team.clone(),
        message_id: message.message_id(),
        description: message
            .envelope
            .summary
            .clone()
            .filter(|summary| !summary.trim().is_empty())
            .unwrap_or_else(|| message.envelope.text.clone()),
        requires_ack: message.requires_ack,
        is_ack: message.is_ack,
        task_id: message.envelope.task_id.clone(),
        recipient_pane_id: recipient_pane_id.cloned(),
    })
}
