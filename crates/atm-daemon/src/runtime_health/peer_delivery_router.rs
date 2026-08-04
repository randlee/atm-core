use super::{DaemonRequestDispatcher, MessageRecord, PostCommitWorkKey, PostWriteRouter};

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
        if message
            .outbound_request
            .to
            .as_ref()
            .and_then(|address| address.host())
            .is_some()
        {
            // Host-qualified origin writes are durable immutable records only
            // until AK.4 introduces the direct peer HTTP sender. They neither
            // emit a local nudge nor start work after this admission response.
            return;
        }
        self.signal_local_post_write(message);
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
