use std::sync::Arc;

use atm_core::api::RequestDeadline;
use atm_core::boundary;
use atm_core::error::AtmError;
use atm_core::protocol::{ResponseEnvelope, next_request_id};
use atm_core::types::HostName;

use crate::peer_delivery_observability::{PeerDeliveryEvent, PeerDeliveryEventKind};
use crate::post_send_emitter::DaemonPostSendHookEmitter;

use super::peer_authority::resolve_peer_authority;
use super::{DaemonGraftPostSendPort, DaemonRequestDispatcher, MessageRecord, PostWriteRouter};

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
        let port: Arc<dyn boundary::GraftPostSendPort + Send + Sync> =
            Arc::new(DaemonGraftPostSendPort::new(self.service_runtime.clone()));
        let emitter = DaemonPostSendHookEmitter::new(Arc::clone(&port));
        message
            .prepared
            .emit_local_post_write(&self.service_runtime, &emitter);
    }

    fn record_peer_outcome(
        &self,
        kind: PeerDeliveryEventKind,
        request_id: atm_core::protocol::RequestId,
        message_id: Option<atm_core::schema::AtmMessageId>,
        peer: HostName,
        error_code: Option<atm_core::error_codes::AtmErrorCode>,
    ) {
        self.record_peer_delivery_event(PeerDeliveryEvent {
            kind,
            request_id,
            message_id,
            peer,
            error_code,
            candidate_count: Some(1),
            next_attempt_at: None,
        });
    }

    fn deliver_to_peer(
        &self,
        message: &MessageRecord,
        host: &HostName,
        deadline: RequestDeadline,
    ) -> Result<(), AtmError> {
        let peer = resolve_peer_authority(host, &self.peer_config_store.list_trusted_peers()?)?;
        let request_id = next_request_id();
        let message_id = message.outbound_request.origin_message_id;
        self.record_peer_outcome(
            PeerDeliveryEventKind::WritePersisted,
            request_id,
            message_id,
            peer.host.clone(),
            None,
        );
        let transport = self
            .https_transport
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("HTTPS peer transport slot lock poisoned"))?
            .clone()
            .ok_or_else(|| {
                AtmError::daemon_unavailable("HTTPS peer transport is not enabled in this daemon")
            })?;
        match transport.deliver(message.outbound_request.clone(), &peer, deadline) {
            Ok(ResponseEnvelope::Error(error)) => {
                self.record_peer_outcome(
                    PeerDeliveryEventKind::PeerDeliveryUnconfirmed,
                    request_id,
                    message_id,
                    peer.host,
                    Some(error.code()),
                );
                Err(error)
            }
            Ok(_) => {
                self.record_peer_outcome(
                    PeerDeliveryEventKind::PeerDeliveryConfirmed,
                    request_id,
                    message_id,
                    peer.host,
                    None,
                );
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
                self.record_peer_outcome(
                    PeerDeliveryEventKind::PeerDeliveryUnconfirmed,
                    request_id,
                    message_id,
                    peer.host,
                    Some(error.code()),
                );
                Err(error)
            }
        }
    }
}

impl PostWriteRouter for DaemonRequestDispatcher {
    fn dispatch(
        &self,
        message: &mut MessageRecord,
        deadline: RequestDeadline,
    ) -> Result<(), AtmError> {
        if message.prepared.is_peer_receipt() {
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
        self.deliver_to_peer(message, host, deadline)
    }
}
