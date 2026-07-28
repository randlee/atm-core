use atm_core::protocol::next_request_id;

use super::{DaemonRequestDispatcher, MessageRecord, PostCommitWorkKey, PostWriteRouter};
use crate::peer_delivery_observability::{PeerDeliveryEvent, PeerDeliveryEventKind};

impl PostWriteRouter for DaemonRequestDispatcher {
    fn dispatch(&self, message: &mut MessageRecord) {
        if message.prepared.is_peer_receipt() {
            tracing::info!(
                subsystem = "runtime_health",
                action = "post_write",
                outcome = "peer_ingress_local_post_write",
                message_id = ?message.outbound_request.origin_message_id,
                "authenticated peer receipt uses the canonical local post-write route"
            );
            self.signal_local_post_write(message);
            return;
        }
        let Some(host) = message
            .outbound_request
            .to
            .as_ref()
            .and_then(|address| address.host())
        else {
            self.signal_local_post_write(message);
            return;
        };
        let request_id = next_request_id();
        let message_id = message.prepared.persisted_message_id();
        self.record_peer_delivery_event(PeerDeliveryEvent {
            kind: PeerDeliveryEventKind::WritePersisted,
            request_id,
            message_id: Some(message_id),
            peer: host.clone(),
            error_code: None,
            candidate_count: Some(1),
            next_attempt_at: None,
        });
        // The immutable write is already committed.  The coordinator keeps
        // only a bounded wake-up by host and performs its own storage/DNS/TLS
        // work after this IPC response has been written.
        self.post_commit_work_queue
            .signal(PostCommitWorkKey::PeerDelivery {
                peer: host.clone(),
                message_id,
            });
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
