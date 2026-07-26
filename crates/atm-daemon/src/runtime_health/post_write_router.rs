use std::sync::Arc;

use atm_core::api::RequestDeadline;
use atm_core::boundary;
use atm_core::error::AtmError;
use atm_core::protocol::{ResponseEnvelope, next_request_id};

use crate::peer_delivery_observability::{PeerDeliveryEvent, PeerDeliveryEventKind};
use crate::post_send_emitter::DaemonPostSendHookEmitter;

use super::peer_authority::resolve_peer_authority;
use super::{DaemonGraftPostSendPort, DaemonRequestDispatcher, MessageRecord, PostWriteRouter};

impl PostWriteRouter for DaemonRequestDispatcher {
    fn dispatch(&self, message: &mut MessageRecord) -> Result<(), AtmError> {
        let host = message
            .outbound_request
            .to
            .as_ref()
            .and_then(|address| address.host());
        if message.prepared.is_peer_receipt() && message.prepared.is_same_store_peer_receipt() {
            let mut event = self.runtime_health_observability.event(
                "peer_duplicate_write_skipped",
                "ok",
                "peer duplicate write skipped; continuing the ordinary local post-write action",
            );
            event.message_id = Some(message.prepared.persisted_message_id());
            self.runtime_health_observability.emit_event_or_warn(event);
        }
        if message.prepared.is_peer_receipt() || host.is_none() {
            let post_send_emitter = self.local_post_write_emitter();
            message
                .prepared
                .emit_local_post_write(&self.service_runtime, &post_send_emitter);
            return Ok(());
        }
        let host = host.expect("host-qualified writes are routed above");
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
        let transport = self
            .https_transport
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("HTTPS peer transport slot lock poisoned"))?
            .clone()
            .ok_or_else(|| {
                AtmError::daemon_unavailable("HTTPS peer transport is not enabled in this daemon")
            })?;
        match transport.deliver(
            message.outbound_request.clone(),
            &peer,
            RequestDeadline::after(std::time::Duration::from_secs(5)),
        ) {
            Ok(ResponseEnvelope::Error(error)) => {
                self.record_unconfirmed_delivery(peer.host, request_id, message_id, error)
            }
            // Reconciliation is explicit: this just-delivered immutable write
            // must not be replayed by the ordinary post-write route.
            Ok(_) => {
                self.record_peer_delivery_event(PeerDeliveryEvent {
                    kind: PeerDeliveryEventKind::PeerDeliveryConfirmed,
                    request_id,
                    message_id,
                    peer: peer.host,
                    error_code: None,
                    candidate_count: Some(1),
                    next_attempt_at: None,
                });
                Ok(())
            }
            Err(error) => {
                let error = if error.is_daemon_unavailable() {
                    AtmError::remote_delivery_unconfirmed(format!(
                        "local persistence completed but peer delivery was not confirmed: {}",
                        error.message()
                    ))
                } else {
                    error
                };
                self.record_unconfirmed_delivery(peer.host, request_id, message_id, error)
            }
        }
    }
}

impl DaemonRequestDispatcher {
    fn local_post_write_emitter(&self) -> DaemonPostSendHookEmitter {
        let graft_port: Arc<dyn boundary::GraftPostSendPort + Send + Sync> =
            Arc::new(DaemonGraftPostSendPort::new(self.service_runtime.clone()));
        DaemonPostSendHookEmitter::new(graft_port)
    }

    fn record_unconfirmed_delivery(
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
