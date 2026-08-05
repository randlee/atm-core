use crate::peer_http_listener::send_peer_http_batch;

use super::{DaemonRequestDispatcher, MessageRecord, PostCommitWorkKey, PostWriteRouter};

impl PostWriteRouter for DaemonRequestDispatcher {
    fn dispatch(
        &self,
        message: &mut MessageRecord,
        deadline: atm_core::api::RequestDeadline,
    ) -> Result<(), atm_core::error::AtmError> {
        if message.prepared.is_peer_receipt() {
            tracing::info!(
                subsystem = "runtime_health",
                action = "post_write",
                outcome = "peer_ingress_local_post_write",
                message_id = ?message.outbound_request.origin_message_id,
                "authenticated peer receipt uses the canonical local post-write route"
            );
            self.signal_local_post_write(message);
            return Ok(());
        }
        if let Some(host) = message
            .outbound_request
            .to
            .as_ref()
            .and_then(|address| address.host())
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
            if let Some(scheduler) = self.peer_resend_scheduler.load_full() {
                scheduler.deliver_or_queue(
                    endpoint.clone(),
                    message.prepared.persisted_message_id(),
                    &message.outbound_request,
                    deadline,
                )?;
            } else {
                // Cache-disabled is intentionally AK.4's direct fast path:
                // no scheduler lock, deadline aggregation, durable scan, or retry.
                send_peer_http_batch(
                    &config,
                    &endpoint,
                    std::slice::from_ref(&message.outbound_request),
                    deadline,
                )?;
                self.message_store.confirm_peer_delivery_batch(
                    &endpoint.canonical_host,
                    &[message.prepared.persisted_message_id()],
                )?;
            }
            tracing::info!(
                subsystem = "runtime_health",
                action = "peer_delivery_confirmation",
                outcome = "confirmed",
                message_id = ?message.prepared.persisted_message_id(),
                "direct configured-peer HTTP delivery completed"
            );
            return Ok(());
        }
        self.signal_local_post_write(message);
        Ok(())
    }
}

impl DaemonRequestDispatcher {
    fn signal_local_post_write(&self, message: &mut MessageRecord) {
        if message.prepared.is_peer_receipt() && message.prepared.is_same_store_peer_receipt() {
            let mut event = self.runtime_health_observability.event(
                "peer_duplicate_write_skipped",
                "ok",
                "peer duplicate write skipped; continuing the ordinary local post-write action",
            );
            event.message_id = Some(message.prepared.persisted_message_id());
            self.runtime_health_observability.emit_event_or_warn(event);
        }
        let message_id = message.prepared.persisted_message_id();
        let Some(target) = message.outbound_request.to.as_ref() else {
            tracing::warn!(subsystem = "runtime_health", action = "post_commit_work_signal", %message_id, "local post-commit work had no canonical destination");
            return;
        };
        self.post_commit_signals.register_local_nudge(
            message_id,
            target
                .team()
                .cloned()
                .unwrap_or_else(|| message.outbound_request.caller_team.clone()),
            target.agent().clone(),
        );
        self.post_commit_work_queue
            .signal(PostCommitWorkKey::LocalNudge(message_id));
    }
}
