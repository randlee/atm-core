use atm_core::{RequestDeadline, protocol::next_request_id};

use super::{DaemonRequestDispatcher, MessageRecord, PostCommitWorkKey, PostWriteRouter};
use crate::peer_delivery_observability::{PeerDeliveryEvent, PeerDeliveryEventKind};
use atm_core::send::WarningEntry;

impl PostWriteRouter for DaemonRequestDispatcher {
    fn dispatch(
        &self,
        message: &mut MessageRecord,
        deadline: RequestDeadline,
    ) -> Vec<WarningEntry> {
        let Some(host) = message
            .outbound_request
            .to
            .as_ref()
            .and_then(|address| address.host())
        else {
            return self.run_received_hook(message, deadline);
        };
        if message.prepared.is_peer_receipt() {
            return self.run_received_hook(message, deadline);
        }
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
        Vec::new()
    }
}

impl DaemonRequestDispatcher {
    fn run_received_hook(
        &self,
        message: &MessageRecord,
        deadline: RequestDeadline,
    ) -> Vec<WarningEntry> {
        tracing::info!(
            subsystem = "runtime_health",
            action = "post_write",
            outcome = "received_hook",
            message_id = ?message.outbound_request.origin_message_id,
            "newly persisted inbound write uses the canonical received-hook route"
        );
        if let Some(warning) = received_hook_deadline_warning(deadline) {
            return vec![warning];
        }
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
                crate::message_received_emitter::message_received_emitter_for_harness(&member)
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
                        %message_id,
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
    WarningEntry::with_code(
        error.code(),
        format!("message received successfully, but its receiver hook did not run: {error}"),
        Some("inspect the receiver hook endpoint or harness, then continue normally"),
    )
}

fn received_hook_deadline_warning(deadline: RequestDeadline) -> Option<WarningEntry> {
    deadline.remaining().is_none().then(|| {
        hook_warning(atm_core::error::AtmError::daemon_unavailable(
            "received-message hook was skipped because the request deadline was exhausted after persistence",
        ))
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use atm_core::error_codes::AtmErrorCode;

    use super::{RequestDeadline, received_hook_deadline_warning};

    #[test]
    fn exhausted_inherited_budget_returns_a_retained_hook_warning() {
        let warning = received_hook_deadline_warning(RequestDeadline::after(Duration::ZERO))
            .expect("an exhausted post-persistence hook budget must be retained as a warning");
        assert_eq!(warning.code, Some(AtmErrorCode::DaemonUnavailable));
        assert!(warning.message.contains("hook was skipped"));
    }
}
