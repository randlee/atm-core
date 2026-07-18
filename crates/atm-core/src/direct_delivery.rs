use crate::boundary::PostSendHookEmitter;
use crate::delivery_plan::logical_messages_from_persistence;
use crate::delivery_policy::DeliveryPolicyCoordinator;
use crate::error::AtmError;
use crate::protocol::{DirectDeliveryOutcome, DirectDeliveryRequest};
use crate::schema::set_remote_host;
use crate::send::{ResolvedRecipient, hook, persist_message};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};

pub fn deliver_direct_messages_with_runtime_and_post_send_emitter(
    home_dir: &std::path::Path,
    request: DirectDeliveryRequest,
    runtime: &LocalServiceRuntime,
    post_send_emitter: &dyn PostSendHookEmitter,
) -> Result<DirectDeliveryOutcome, AtmError> {
    if request.messages.is_empty() {
        return Err(AtmError::validation(
            "direct delivery request must contain at least one message",
        )
        .with_recovery(
            "Include at least one canonical inbox envelope before retrying the cross-host direct delivery.",
        ));
    }

    let recipient = ResolvedRecipient {
        agent: request.agent.clone(),
        team: request.team.clone(),
    };
    let inbox_path = runtime.inbox_path(home_dir, &request.team, &request.agent)?;
    let snapshot = DeliveryPolicyCoordinator::new().resolve_recipient_snapshot(
        runtime,
        &request.team,
        &request.agent,
    )?;
    let mut warnings = Vec::new();
    let mut logical_messages = Vec::new();

    for message in &request.messages {
        let mut message = message.clone();
        if let Some(remote_host) = request.remote_host.as_deref() {
            set_remote_host(&mut message, remote_host);
        }
        let persistence =
            persist_message(runtime, home_dir, &snapshot, &inbox_path, &message, false)?;
        warnings.extend(persistence.warnings.clone());
        logical_messages.extend(
            logical_messages_from_persistence(
                &persistence,
                message.requires_ack,
                message.acknowledges_message_id.is_some(),
            )
            .map_err(|error| {
                AtmError::mailbox_write(error.to_string()).with_recovery(
                    "Repair the persisted direct-delivery record shape before retrying cross-host delivery.",
                )
            })?,
        );
    }

    hook::emit_post_send_effects(
        runtime,
        &mut warnings,
        None,
        Some(post_send_emitter),
        &recipient,
        &snapshot,
        &logical_messages,
    );

    Ok(DirectDeliveryOutcome {
        delivered_messages: request.messages.len(),
    })
}
