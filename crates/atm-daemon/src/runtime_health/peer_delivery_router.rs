use std::sync::Arc;

use atm_core::boundary;
use atm_core::error::AtmError;
use atm_core::protocol::next_request_id;

use crate::peer_delivery_observability::{PeerDeliveryEvent, PeerDeliveryEventKind};
use crate::post_send_emitter::DaemonPostSendHookEmitter;

use super::{DaemonGraftPostSendPort, DaemonRequestDispatcher, MessageRecord, PostWriteRouter};

impl PostWriteRouter for DaemonRequestDispatcher {
    fn dispatch(&self, message: &mut MessageRecord) -> Result<(), AtmError> {
        if message.prepared.is_peer_receipt() {
            tracing::info!(
                subsystem = "runtime_health",
                action = "post_write",
                outcome = "peer_ingress_local_post_write",
                message_id = ?message.outbound_request.origin_message_id,
                "authenticated peer receipt uses the canonical local post-write route"
            );
            self.emit_local_post_write(message);
            return Ok(());
        }
        let Some(host) = message
            .outbound_request
            .to
            .as_ref()
            .and_then(|address| address.host())
        else {
            self.emit_local_post_write(message);
            return Ok(());
        };
        let request_id = next_request_id();
        let message_id = message.outbound_request.origin_message_id;
        self.record_peer_delivery_event(PeerDeliveryEvent {
            kind: PeerDeliveryEventKind::WritePersisted,
            request_id,
            message_id,
            peer: host.clone(),
            error_code: None,
            candidate_count: Some(1),
            next_attempt_at: None,
        });
        // The immutable write is already committed.  The coordinator keeps
        // only a bounded wake-up by host and performs its own storage/DNS/TLS
        // work after this IPC response has been written.
        self.peer_delivery_coordinator
            .signal_after_persist(host.clone());
        Ok(())
    }
}

impl DaemonRequestDispatcher {
    fn emit_local_post_write(&self, message: &mut MessageRecord) {
        if message.prepared.is_peer_receipt() && message.prepared.is_same_store_peer_receipt() {
            let mut event = self.runtime_health_observability.event(
                "peer_duplicate_write_skipped",
                "ok",
                "peer duplicate write skipped; continuing the ordinary local post-write action",
            );
            event.message_id = Some(message.prepared.persisted_message_id());
            self.runtime_health_observability.emit_event_or_warn(event);
        }
        let graft_port: Arc<dyn boundary::GraftPostSendPort + Send + Sync> =
            Arc::new(DaemonGraftPostSendPort::new(self.service_runtime.clone()));
        let emitter = DaemonPostSendHookEmitter::new(Arc::clone(&graft_port));
        message
            .prepared
            .emit_local_post_write(&self.service_runtime, &emitter);
    }
}
