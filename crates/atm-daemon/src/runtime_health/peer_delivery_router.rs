use std::sync::Arc;

use atm_core::RequestDeadline;
use atm_core::boundary;
use atm_core::error::AtmError;
use atm_core::protocol::{ResponseEnvelope, next_request_id};

use crate::https_transport::resolve_peer_authority;
use crate::peer_delivery_observability::{PeerDeliveryEvent, PeerDeliveryEventKind};
use crate::post_send_emitter::DaemonPostSendHookEmitter;

use super::{DaemonGraftPostSendPort, DaemonRequestDispatcher, MessageRecord, PostWriteRouter};

impl PostWriteRouter for DaemonRequestDispatcher {
    fn dispatch(
        &self,
        message: &mut MessageRecord,
        deadline: RequestDeadline,
    ) -> Result<(), AtmError> {
        if message.prepared.is_peer_receipt() {
            if message.prepared.is_same_store_peer_receipt() {
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
            return Ok(());
        }
        let Some(host) = message
            .outbound_request
            .to
            .as_ref()
            .and_then(|address| address.host())
        else {
            let graft_port: Arc<dyn boundary::GraftPostSendPort + Send + Sync> =
                Arc::new(DaemonGraftPostSendPort::new(self.service_runtime.clone()));
            let emitter = DaemonPostSendHookEmitter::new(Arc::clone(&graft_port));
            message
                .prepared
                .emit_local_post_write(&self.service_runtime, &emitter);
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
        let transport = self
            .https_transport
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("HTTPS peer transport slot lock poisoned"))?
            .clone();
        match transport {
            Some(transport) => {
                self.deliver_to_peer(message, deadline, peer, request_id, message_id, transport)
            }
            None => self.peer_delivery_unconfirmed(
                peer.host,
                request_id,
                message_id,
                AtmError::daemon_unavailable("HTTPS peer transport is not enabled in this daemon"),
            ),
        }
    }
}

impl DaemonRequestDispatcher {
    fn deliver_to_peer(
        &self,
        message: &MessageRecord,
        deadline: RequestDeadline,
        peer: atm_storage::TrustedPeer,
        request_id: atm_core::protocol::RequestId,
        message_id: Option<atm_core::schema::AtmMessageId>,
        transport: Arc<dyn crate::https_transport::HttpsMessageTransport>,
    ) -> Result<(), AtmError> {
        match transport.deliver(message.outbound_request.clone(), &peer, deadline) {
            Ok(ResponseEnvelope::Error(error)) => {
                self.peer_delivery_terminal_error(peer.host, request_id, message_id, error)
            }
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
            Err(error) if error.is_daemon_unavailable() => {
                self.peer_delivery_unconfirmed(peer.host, request_id, message_id, error)
            }
            Err(error) => {
                self.peer_delivery_terminal_error(peer.host, request_id, message_id, error)
            }
        }
    }

    fn peer_delivery_unconfirmed(
        &self,
        peer: atm_core::types::HostName,
        request_id: atm_core::protocol::RequestId,
        message_id: Option<atm_core::schema::AtmMessageId>,
        error: AtmError,
    ) -> Result<(), AtmError> {
        let unconfirmed = AtmError::remote_delivery_unconfirmed(format!(
            "local persistence completed but peer delivery was not confirmed: {}",
            error.message()
        ));
        self.peer_delivery_terminal_error(peer, request_id, message_id, unconfirmed)
    }

    fn peer_delivery_terminal_error(
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
