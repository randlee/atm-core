use super::*;

pub(super) fn build_built_in_dispatch<R>(
    runtime: &R,
    delivery_snapshot: &crate::delivery_policy::DeliveryRecipientSnapshot,
    event: &PostSendHookEvent,
) -> Option<BuiltInPostSendDispatch>
where
    R: RetainedServiceRuntime + ?Sized,
{
    if delivery_snapshot.local_tmux_post_send {
        let pane_id = event
            .recipient_pane_id
            .clone()
            .or_else(|| delivery_snapshot.recipient_pane_id.as_ref().cloned())?;
        let kind = built_in_nudge_template_kind_from_post_send_event(event);
        let override_row = match runtime.load_nudge_template_override(&event.recipient_team, kind) {
            Ok(row) => row,
            Err(error) => {
                warn!(
                    code = %error.code,
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
        let template_body = template.body.as_deref()?;
        let rendered_nudge = match nudge_template::render_built_in_nudge(event, template_body) {
            Ok(rendered) => rendered,
            Err(error) => {
                warn!(
                    code = %error.code,
                    recipient = %event.recipient,
                    recipient_team = %event.recipient_team,
                    message_id = %event.message_id,
                    %error,
                    "failed to render built-in tmux nudge"
                );
                return None;
            }
        };
        return Some(BuiltInPostSendDispatch {
            event: event.clone(),
            target: PostSendBuiltInTarget::LocalTmux(LocalTmuxNudgeTarget {
                pane_id,
                rendered_nudge,
            }),
        });
    }
    if delivery_snapshot.graft_post_send {
        return Some(BuiltInPostSendDispatch {
            event: event.clone(),
            target: PostSendBuiltInTarget::Graft(GraftNudgeTarget {
                recipient: event.recipient.clone(),
                recipient_team: event.recipient_team.clone(),
            }),
        });
    }
    None
}
