use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use atm_core::boundary::{
    self, NonClaudeOutboundDeliveryRequest, NonClaudeOutboundDeliveryResponse,
};
use atm_core::error::AtmError;
use atm_core::schema::AtmMessageId;
use atm_core::send::SendRequest;

use crate::peer_transport::delivery::CrossHostDelivery;

type OutputPathFactory = Arc<dyn Fn() -> Result<PathBuf, AtmError> + Send + Sync>;
const MAX_NON_CLAUDE_PAYLOAD_BYTES: usize = 1024 * 1024;
#[derive(Clone)]
pub(crate) struct DaemonNonClaudeOutbound {
    path_factory: OutputPathFactory,
}

impl std::fmt::Debug for DaemonNonClaudeOutbound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DaemonNonClaudeOutbound(..)")
    }
}

impl DaemonNonClaudeOutbound {
    #[cfg_attr(
        test,
        allow(
            dead_code,
            reason = "Production runtime wiring is compiled out in daemon lib tests; the constructor stays exercised in non-test builds and targeted adapter tests."
        )
    )]
    pub(crate) fn new() -> Self {
        Self::new_with_path_factory(Arc::new(|| {
            Ok(atm_core::home::host_runtime_dir()?.join("non_claude_outbound.jsonl"))
        }))
    }

    fn new_with_path_factory(path_factory: OutputPathFactory) -> Self {
        Self { path_factory }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test_with_path(path: PathBuf) -> Self {
        Self::new_with_path_factory(Arc::new(move || Ok(path.clone())))
    }
}

impl boundary::sealed::Sealed for DaemonNonClaudeOutbound {}

impl boundary::NonClaudeOutbound for DaemonNonClaudeOutbound {
    fn deliver_payloads(
        &self,
        request: NonClaudeOutboundDeliveryRequest,
    ) -> Result<NonClaudeOutboundDeliveryResponse, AtmError> {
        // This trait is intentionally synchronous; blocking filesystem I/O is
        // the correct execution model here and `spawn_blocking` would violate
        // the trait contract rather than improve it.
        let output_path = (self.path_factory)()?;
        let mut bytes = serde_json::to_vec(&request)?;
        if bytes.len() > MAX_NON_CLAUDE_PAYLOAD_BYTES {
            return Err(AtmError::mailbox_write(format!(
                "non-Claude outbound payload for {} exceeded {MAX_NON_CLAUDE_PAYLOAD_BYTES} bytes",
                output_path.display()
            ))
            .with_recovery(
                "Reduce message count or body size before retrying non-Claude delivery through the outbound payload sink.",
            ));
        }
        bytes.push(b'\n');
        append_payload_to_file(&output_path, &bytes)?;

        Ok(NonClaudeOutboundDeliveryResponse {
            delivered_messages: request.messages.len(),
        })
    }
}

#[derive(Clone)]
pub(crate) struct DaemonRemoteSendRouter {
    cross_host_delivery: Arc<dyn CrossHostDelivery + Send + Sync>,
}

impl std::fmt::Debug for DaemonRemoteSendRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DaemonRemoteSendRouter(..)")
    }
}

impl DaemonRemoteSendRouter {
    pub(crate) fn new(cross_host_delivery: Arc<dyn CrossHostDelivery + Send + Sync>) -> Self {
        Self {
            cross_host_delivery,
        }
    }
}

impl boundary::sealed::Sealed for DaemonRemoteSendRouter {}

impl boundary::RemoteSendRouter for DaemonRemoteSendRouter {
    fn deliver_remote_send(&self, request: SendRequest) -> Result<(), AtmError> {
        let remote_host = request.remote_host.clone().ok_or_else(|| {
            AtmError::validation("remote send router requires request.remote_host")
                .with_recovery("Populate the remote host before retrying remote-target delivery.")
        })?;
        let outcome = self
            .cross_host_delivery
            .deliver_remote(request, remote_host, AtmMessageId::new())
            .map_err(|error| error.into_atm_error())?;
        match outcome {
            crate::peer_transport::delivery::SendOutcome::Delivered(_) => Ok(()),
            crate::peer_transport::delivery::SendOutcome::Deferred { error, .. } => Err(error),
            crate::peer_transport::delivery::SendOutcome::OutcomeUnknown { error, .. } => {
                Err(error)
            }
            crate::peer_transport::delivery::SendOutcome::RejectedTerminal(error) => Err(error),
        }
    }
}

fn append_payload_to_file(output_path: &Path, bytes: &[u8]) -> Result<(), AtmError> {
    let parent = output_path.parent().ok_or_else(|| {
        AtmError::mailbox_write(format!(
            "non-Claude outbound path {} has no parent directory",
            output_path.display()
        ))
        .with_recovery(
            "Check that ATM_HOME is writable and that the host runtime directory has available disk space before retrying non-Claude delivery.",
        )
    })?;
    std::fs::create_dir_all(parent).map_err(|error| {
        AtmError::mailbox_write(format!(
            "failed to create non-Claude outbound directory {}: {error}",
            parent.display()
        ))
        .with_recovery(
            "Check that ATM_HOME is writable and that the host runtime directory has available disk space before retrying non-Claude delivery.",
        )
        .with_source(error)
    })?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(output_path)
        .map_err(|error| {
            AtmError::mailbox_write(format!(
                "failed to open non-Claude outbound sink {} for append: {error}",
                output_path.display()
            ))
            .with_recovery(
                "Check that ATM_HOME is writable and that the host runtime directory has available disk space before retrying non-Claude delivery.",
            )
            .with_source(error)
        })?;
    // MAX_CONCURRENT_CONNECTIONS bounds callers here; FS stall under that
    // ceiling is accepted delivery latency.
    file.write_all(bytes).map_err(|error| {
        AtmError::mailbox_write(format!(
            "failed to append non-Claude outbound payload {}: {error}",
            output_path.display()
        ))
        .with_recovery(
            "Check that ATM_HOME is writable and that the host runtime directory has available disk space before retrying non-Claude delivery.",
        )
        .with_source(error)
    })?;
    // MAX_CONCURRENT_CONNECTIONS bounds callers here; FS stall under that
    // ceiling is accepted delivery latency.
    file.sync_data().map_err(|error| {
        AtmError::mailbox_write(format!(
            "failed to sync non-Claude outbound payload {}: {error}",
            output_path.display()
        ))
        .with_recovery(
            "Check that ATM_HOME is writable and that the host runtime directory has available disk space before retrying non-Claude delivery.",
        )
        .with_source(error)
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{DaemonNonClaudeOutbound, DaemonRemoteSendRouter};
    use crate::peer_transport::delivery::{
        CrossHostDelivery, CrossHostDeliveryInfraError, SendOutcome,
    };
    use atm_core::boundary::{
        NonClaudeOutbound, NonClaudeOutboundDeliveryRequest, RemoteSendRouter,
    };
    use atm_core::schema::{AtmMessageId, InboxMessage};
    use atm_core::send::{RemoteTargetHost, SendRequest};
    use atm_core::test_support::{TEST_SENDER, TEST_TEAM};
    use atm_core::types::IsoTimestamp;
    use serde_json::Map;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    #[derive(Default)]
    struct RecordingCrossHostDelivery {
        requests: Mutex<Vec<(String, SendRequest)>>,
    }

    impl atm_core::boundary::sealed::Sealed for RecordingCrossHostDelivery {}

    impl CrossHostDelivery for RecordingCrossHostDelivery {
        fn deliver_remote(
            &self,
            request: SendRequest,
            remote_host: RemoteTargetHost,
            _deferred_receipt_message_id: AtmMessageId,
        ) -> Result<SendOutcome, CrossHostDeliveryInfraError> {
            self.requests
                .lock()
                .expect("requests")
                .push((remote_host.as_str().to_string(), request));
            let response = atm_core::ResponseEnvelope::Send(
                atm_core::protocol::SendResponseEnvelope::Sent(atm_core::send::SendOutcome {
                    action: atm_core::types::CommandAction::Send,
                    team: atm_core::types::TeamName::from_validated(TEST_TEAM),
                    agent: atm_core::types::AgentName::from_validated("recipient"),
                    sender: atm_core::types::AgentName::from_validated(TEST_SENDER),
                    outcome: atm_core::send::SendCommandOutcome::Sent,
                    message_id: AtmMessageId::new(),
                    receipt_message_id: None,
                    requires_ack: false,
                    task_id: None,
                    summary: None,
                    message: None,
                    warnings: Vec::new(),
                    dry_run: false,
                }),
            );
            Ok(SendOutcome::Delivered(Box::new(response)))
        }
    }

    fn request() -> NonClaudeOutboundDeliveryRequest {
        NonClaudeOutboundDeliveryRequest {
            team: TEST_TEAM.parse().expect("team"),
            agent: "recipient".parse().expect("agent"),
            remote_host: None,
            origin: None,
            recipient_pane_id: Some(atm_core::types::PaneId::new("pane-1").expect("pane")),
            messages: vec![InboxMessage {
                from: TEST_SENDER.parse().expect("sender"),
                text: "hello".to_string(),
                timestamp: IsoTimestamp::now(),
                read: false,
                source_team: Some(TEST_TEAM.parse().expect("source team")),
                summary: Some("hello".to_string()),
                message_id: Some(AtmMessageId::new()),
                requires_ack: false,
                pending_ack_at: None,
                acknowledged_at: None,
                acknowledges_message_id: None,
                parent_message_id: None,
                thread_mode: None,
                expires_at: None,
                task_id: None,
                extra: Map::new(),
            }],
        }
    }

    #[test]
    fn daemon_non_claude_outbound_appends_typed_payload_record() {
        let tempdir = TempDir::new().expect("tempdir");
        let output_path = tempdir.path().join("non_claude_outbound.jsonl");
        let runtime = DaemonNonClaudeOutbound::new_for_test_with_path(output_path.clone());

        let response = runtime
            .deliver_payloads(request())
            .expect("deliver payload");
        assert_eq!(response.delivered_messages, 1);

        let written = std::fs::read_to_string(output_path).expect("read output");
        let record: NonClaudeOutboundDeliveryRequest =
            serde_json::from_str(written.trim()).expect("decode record");
        assert_eq!(record.team.as_str(), TEST_TEAM);
        assert_eq!(record.agent.as_str(), "recipient");
        assert_eq!(record.messages.len(), 1);
        assert_eq!(record.messages[0].from.as_str(), TEST_SENDER);
    }

    #[test]
    fn daemon_non_claude_outbound_rejects_payloads_above_size_cap() {
        let tempdir = TempDir::new().expect("tempdir");
        let output_path = tempdir.path().join("non_claude_outbound.jsonl");
        let runtime = DaemonNonClaudeOutbound::new_for_test_with_path(output_path.clone());

        let oversized = NonClaudeOutboundDeliveryRequest {
            team: TEST_TEAM.parse().expect("team"),
            agent: "recipient".parse().expect("agent"),
            remote_host: None,
            origin: None,
            recipient_pane_id: Some(atm_core::types::PaneId::new("pane-1").expect("pane")),
            messages: vec![InboxMessage {
                from: TEST_SENDER.parse().expect("sender"),
                text: "x".repeat(1024 * 1024),
                timestamp: IsoTimestamp::now(),
                read: false,
                source_team: Some(TEST_TEAM.parse().expect("source team")),
                summary: Some("oversized".to_string()),
                message_id: Some(AtmMessageId::new()),
                requires_ack: false,
                pending_ack_at: None,
                acknowledged_at: None,
                acknowledges_message_id: None,
                parent_message_id: None,
                thread_mode: None,
                expires_at: None,
                task_id: None,
                extra: Map::new(),
            }],
        };

        let error = runtime
            .deliver_payloads(oversized)
            .expect_err("oversized payload must fail");
        assert!(error.to_string().contains("exceeded"));
        assert!(!output_path.exists());
    }

    #[test]
    fn daemon_remote_send_router_routes_remote_host_requests_through_cross_host_delivery() {
        let cross_host_delivery = Arc::new(RecordingCrossHostDelivery::default());
        let runtime = DaemonRemoteSendRouter::new(cross_host_delivery.clone());
        let request = SendRequest {
            home_dir: PathBuf::from("/tmp/atm-home"),
            current_dir: PathBuf::from("/tmp/worktree"),
            caller_identity: "sender".parse().expect("sender"),
            caller_team: "test-team".parse().expect("team"),
            to: "qa-a@test-team".parse().expect("to"),
            message_source: atm_core::send::SendMessageSource::Inline("hello".to_string()),
            summary_override: None,
            requires_ack: false,
            task_id: None,
            parent_message_id: None,
            acknowledges_message_id: None,
            thread_mode: None,
            expires_at: None,
            source_remote_host: None,
            remote_host: Some(
                atm_core::send::RemoteTargetHost::parse("10.10.100.98").expect("remote host"),
            ),
            dry_run: false,
        };

        runtime
            .deliver_remote_send(request.clone())
            .expect("deliver remote send");

        let captured = cross_host_delivery.requests.lock().expect("requests");
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].0, "10.10.100.98");
        assert_eq!(captured[0].1.to, request.to);
        assert_eq!(
            captured[0].1.remote_host.as_ref().map(|host| host.as_str()),
            Some("10.10.100.98")
        );
        match &captured[0].1.message_source {
            atm_core::send::SendMessageSource::Inline(body) => assert_eq!(body, "hello"),
            other => panic!("unexpected send source: {other:?}"),
        }
    }
}
