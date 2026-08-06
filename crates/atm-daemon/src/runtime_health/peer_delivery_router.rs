use crate::peer_delivery_client::send_configured_peer_write;

use super::{DaemonRequestDispatcher, MessageRecord, PostWriteRouter};
use atm_core::send::WarningEntry;

impl PostWriteRouter for DaemonRequestDispatcher {
    fn dispatch(
        &self,
        message: &mut MessageRecord,
        deadline: atm_core::api::RequestDeadline,
    ) -> Result<Vec<WarningEntry>, atm_core::error::AtmError> {
        // A host-qualified local write is the one sender-side action. Peer
        // provenance, not `--host`, distinguishes an inbound receipt from
        // that local outbound write.
        let destination_host = message
            .outbound_request
            .to
            .as_ref()
            .and_then(|address| address.host());
        if !message.prepared.is_peer_receipt()
            && let Some(host) = destination_host
        {
            let endpoint = self
                .admission_runtime_view
                .endpoint_for_canonical_host(host)
                .ok_or_else(|| {
                    atm_core::error::AtmError::remote_delivery_unconfirmed(format!(
                        "local persistence succeeded but canonical peer `{host}` is no longer enabled"
                    ))
                })?;
            let config = self.peer_http_runtime_config.load_full().ok_or_else(|| {
                atm_core::error::AtmError::remote_delivery_unconfirmed(
                    "local persistence succeeded but no enabled local peer interface advertises a source host",
                )
            })?;
            let response = send_configured_peer_write(
                &config,
                &endpoint,
                &message.outbound_request,
                deadline,
            )?;
            self.message_store.confirm_peer_delivery_batch(
                &endpoint.canonical_host,
                &[message.prepared.persisted_message_id()],
            )?;
            tracing::info!(
                subsystem = "runtime_health",
                action = "peer_delivery_confirmation",
                outcome = "confirmed",
                message_id = ?message.prepared.persisted_message_id(),
                "direct configured-peer HTTP delivery completed"
            );
            return Ok(send_response_warnings(response));
        }

        // This is the sole receiver-side hook path. It is reached only after
        // the immutable message write has committed. A hook error is carried
        // home as a warning, not turned into a receive failure.
        tracing::info!(
            subsystem = "runtime_health",
            action = "message_received_hook",
            outcome = "emit_after_persist",
            message_id = ?message.outbound_request.origin_message_id,
            "accepted receive invokes the canonical message-received hook"
        );
        Ok(self.emit_message_received_hook(message))
    }
}

impl DaemonRequestDispatcher {
    fn emit_message_received_hook(&self, message: &MessageRecord) -> Vec<WarningEntry> {
        let message_id = message.prepared.persisted_message_id();
        let Some(target) = message.outbound_request.to.as_ref() else {
            return vec![hook_warning(atm_core::error::AtmError::validation(
                "durably received message had no canonical destination for receiver hook",
            ))];
        };
        let team = target
            .team()
            .cloned()
            .unwrap_or_else(|| message.outbound_request.caller_team.clone());
        let agent = target.agent().clone();
        let emitter = match self.service_runtime.load_roster_member(&team, &agent) {
            Ok(Some(member)) => {
                crate::post_send_emitter::message_received_emitter_for_harness(&member)
            }
            Ok(None) => None,
            Err(error) => return vec![hook_warning(error)],
        };
        match atm_core::send::emit_persisted_local_post_write(
            &self.service_runtime,
            self.observability.as_ref(),
            self.home_dir.as_path(),
            &team,
            &agent,
            message_id,
            emitter.as_deref(),
        ) {
            Ok(warnings) => {
                for warning in &warnings {
                    tracing::warn!(
                        subsystem = "runtime_health",
                        action = "message_received_hook",
                        outcome = "warning",
                        message_id = %message_id,
                        code = ?warning.code,
                        "received-message hook completed with warning: {}",
                        warning.message
                    );
                }
                warnings
            }
            Err(error) => vec![hook_warning(error)],
        }
    }
}

fn hook_warning(error: atm_core::error::AtmError) -> WarningEntry {
    let diagnostic = error
        .cause()
        .map(|cause| format!(" Detail: {cause}"))
        .unwrap_or_default();
    WarningEntry::with_code(
        error.code(),
        format!(
            "message received successfully, but its receiver hook did not run: {error}.{diagnostic}"
        ),
        Some("inspect the receiver hook endpoint or harness, then continue normally"),
    )
}

fn send_response_warnings(response: atm_core::protocol::SendResponseEnvelope) -> Vec<WarningEntry> {
    match response {
        atm_core::protocol::SendResponseEnvelope::Sent(outcome) => outcome.warnings,
        atm_core::protocol::SendResponseEnvelope::Acknowledged(outcome) => outcome.warnings,
    }
}

#[cfg(test)]
mod tests {
    use atm_core::error::{AtmError, AtmErrorCode};

    use super::hook_warning;

    const ROUTER_SOURCE: &str = include_str!("peer_delivery_router.rs");

    #[test]
    fn source_guard_has_one_receiver_hook_path_and_no_sender_nudge() {
        let source = ROUTER_SOURCE.replace("\r\n", "\n");
        let receiver_hook_call = concat!("self.emit_message_received_", "hook(message)");
        assert_eq!(
            source.matches(receiver_hook_call).count(),
            1,
            "the receive hook has one daemon emission point"
        );
        let outbound = source
            .split("if !message.prepared.is_peer_receipt()")
            .nth(1)
            .expect("outbound guard")
            .split("// This is the sole receiver-side hook path.")
            .next()
            .expect("outbound branch");
        assert_eq!(outbound.matches("send_configured_peer_write(").count(), 1);
        assert!(!outbound.contains("emit_message_received_hook"));
    }

    #[test]
    fn hook_warning_preserves_adapter_diagnostics_after_successful_receive() {
        let warning = hook_warning(
            AtmError::for_code(AtmErrorCode::PostSendTmuxSendFailed)
                .with_cause("tmux exited unsuccessfully: pane is gone"),
        );

        assert_eq!(warning.code, Some(AtmErrorCode::PostSendTmuxSendFailed));
        assert!(warning.message.starts_with("message received successfully"));
        assert!(
            warning
                .message
                .contains("tmux exited unsuccessfully: pane is gone")
        );
    }
}
