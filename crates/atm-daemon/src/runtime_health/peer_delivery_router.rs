use std::sync::Arc;

use atm_core::RequestDeadline;
use atm_core::boundary;
use atm_core::error::AtmError;
use atm_core::protocol::next_request_id;

use crate::peer_delivery_observability::{PeerDeliveryEvent, PeerDeliveryEventKind};
use crate::post_send_emitter::DaemonPostSendHookEmitter;

use super::peer_authority::resolve_peer_authority;
use super::{DaemonGraftPostSendPort, DaemonRequestDispatcher, MessageRecord, PostWriteRouter};

impl PostWriteRouter for DaemonRequestDispatcher {
    fn dispatch(
        &self,
        message: &mut MessageRecord,
        deadline: RequestDeadline,
    ) -> Result<(), AtmError> {
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
        let peer = resolve_peer_authority(host, &self.peer_config_store.list_trusted_peers()?)?;
        let request_id = next_request_id();
        let message_id = message.outbound_request.origin_message_id;
        self.record_peer_delivery_event(PeerDeliveryEvent {
            kind: PeerDeliveryEventKind::WritePersisted,
            request_id,
            message_id,
            peer: peer.host.clone(),
            error_code: None,
            candidate_count: Some(1),
            next_attempt_at: None,
        });
        self.deliver_to_peer(message, deadline, peer.host, request_id, message_id)
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

    /// The canonical router's only peer-delivery handoff. AI.28 moves the
    /// actual bounded drain behind the coordinator without adding a route.
    fn deliver_to_peer(
        &self,
        message: &MessageRecord,
        deadline: RequestDeadline,
        peer: atm_core::types::HostName,
        request_id: atm_core::protocol::RequestId,
        message_id: Option<atm_core::schema::AtmMessageId>,
    ) -> Result<(), AtmError> {
        // The coordinator owns retry eligibility and backoff. This router only
        // classifies the foreground result: an unconfirmed delivery is
        // retryable through the coordinator, while a validation/configuration
        // error is returned without inventing a second retry policy.
        if let Err(error) = self
            .peer_delivery_coordinator
            .deliver_after_persist(&message.outbound_request, deadline)
        {
            // Retry classification and scheduling belong to the coordinator;
            // every foreground failure has the same retained projection event.
            return self.record_peer_delivery_failure(peer, request_id, message_id, error);
        }
        Ok(())
    }

    fn record_peer_delivery_failure(
        &self,
        peer: atm_core::types::HostName,
        request_id: atm_core::protocol::RequestId,
        message_id: Option<atm_core::schema::AtmMessageId>,
        error: AtmError,
    ) -> Result<(), AtmError> {
        self.record_peer_delivery_event(PeerDeliveryEvent {
            kind: PeerDeliveryEventKind::PeerDeliveryUnconfirmed,
            request_id,
            message_id,
            peer,
            error_code: Some(error.code()),
            candidate_count: Some(1),
            next_attempt_at: None,
        });
        Err(error)
    }
}
