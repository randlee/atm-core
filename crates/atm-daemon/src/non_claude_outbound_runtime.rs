use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use atm_core::boundary::{
    self, NonClaudeOutboundDeliveryRequest, NonClaudeOutboundDeliveryResponse,
};
use atm_core::error::AtmError;
use atm_core::protocol::DirectDeliveryRequest;
use atm_core::send::RemoteTargetHost;

use crate::peer_transport::delivery::CrossHostDelivery;

type OutputPathFactory = Arc<dyn Fn() -> Result<PathBuf, AtmError> + Send + Sync>;
const MAX_NON_CLAUDE_PAYLOAD_BYTES: usize = 1024 * 1024;
#[derive(Clone)]
pub(crate) struct DaemonNonClaudeOutbound {
    path_factory: OutputPathFactory,
    cross_host_delivery: Option<Arc<dyn CrossHostDelivery + Send + Sync>>,
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

    pub(crate) fn new_with_cross_host_delivery(
        cross_host_delivery: Arc<dyn CrossHostDelivery + Send + Sync>,
    ) -> Self {
        Self {
            path_factory: Arc::new(|| {
                Ok(atm_core::home::host_runtime_dir()?.join("non_claude_outbound.jsonl"))
            }),
            cross_host_delivery: Some(cross_host_delivery),
        }
    }

    fn new_with_path_factory(path_factory: OutputPathFactory) -> Self {
        Self {
            path_factory,
            cross_host_delivery: None,
        }
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
        if let Some(remote_host) = request.remote_host.as_deref() {
            let cross_host_delivery = self.cross_host_delivery.as_ref().ok_or_else(|| {
                AtmError::daemon_unavailable(
                    "cross-host outbound delivery is unavailable for remote acknowledgement reply routing",
                )
                .with_recovery(
                    "Restart atm-daemon with the cross-host runtime fully assembled before retrying the remote acknowledgement reply.",
                )
            })?;
            let remote_host = RemoteTargetHost::parse(remote_host)?;
            let outcome = cross_host_delivery
                .deliver_direct(
                    DirectDeliveryRequest {
                        team: request.team.clone(),
                        agent: request.agent.clone(),
                        remote_host: Some(remote_host.as_str().to_string()),
                        messages: request.messages.clone(),
                    },
                    remote_host,
                )
                .map_err(|error| error.into_atm_error())?;
            return Ok(NonClaudeOutboundDeliveryResponse {
                delivered_messages: outcome.delivered_messages,
            });
        }
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
    use super::DaemonNonClaudeOutbound;
    use crate::peer_transport::delivery::{
        CrossHostDelivery, CrossHostDeliveryInfraError, SendOutcome,
    };
    use atm_core::boundary::{NonClaudeOutbound, NonClaudeOutboundDeliveryRequest};
    use atm_core::protocol::{DirectDeliveryOutcome, DirectDeliveryRequest};
    use atm_core::schema::{AtmMessageId, InboxMessage};
    use atm_core::send::{RemoteTargetHost, SendRequest};
    use atm_core::test_support::{TEST_SENDER, TEST_TEAM};
    use atm_core::types::IsoTimestamp;
    use serde_json::Map;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    #[derive(Default)]
    struct RecordingCrossHostDelivery {
        requests: Mutex<Vec<(String, DirectDeliveryRequest)>>,
    }

    impl atm_core::boundary::sealed::Sealed for RecordingCrossHostDelivery {}

    impl CrossHostDelivery for RecordingCrossHostDelivery {
        fn deliver_remote(
            &self,
            _request: SendRequest,
            _remote_host: RemoteTargetHost,
            _deferred_receipt_message_id: AtmMessageId,
        ) -> Result<SendOutcome, CrossHostDeliveryInfraError> {
            unreachable!("test only exercises direct delivery")
        }

        fn deliver_direct(
            &self,
            request: DirectDeliveryRequest,
            remote_host: RemoteTargetHost,
        ) -> Result<DirectDeliveryOutcome, CrossHostDeliveryInfraError> {
            self.requests
                .lock()
                .expect("requests")
                .push((remote_host.as_str().to_string(), request.clone()));
            Ok(DirectDeliveryOutcome {
                delivered_messages: request.messages.len(),
            })
        }
    }

    fn request() -> NonClaudeOutboundDeliveryRequest {
        NonClaudeOutboundDeliveryRequest {
            team: TEST_TEAM.parse().expect("team"),
            agent: "recipient".parse().expect("agent"),
            remote_host: None,
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
    fn daemon_non_claude_outbound_routes_remote_host_requests_through_cross_host_delivery() {
        let cross_host_delivery = Arc::new(RecordingCrossHostDelivery::default());
        let runtime =
            DaemonNonClaudeOutbound::new_with_cross_host_delivery(cross_host_delivery.clone());
        let mut request = request();
        request.remote_host = Some("10.10.100.98".to_string());

        let response = runtime
            .deliver_payloads(request.clone())
            .expect("deliver payload");
        assert_eq!(response.delivered_messages, 1);

        let captured = cross_host_delivery.requests.lock().expect("requests");
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].0, "10.10.100.98");
        assert_eq!(captured[0].1.team, request.team);
        assert_eq!(captured[0].1.agent, request.agent);
        assert_eq!(captured[0].1.remote_host, request.remote_host);
        assert_eq!(captured[0].1.messages, request.messages);
    }
}
