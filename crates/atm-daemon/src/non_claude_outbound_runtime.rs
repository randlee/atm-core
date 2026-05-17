use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use atm_core::boundary::{
    self, NonClaudeOutboundDeliveryRequest, NonClaudeOutboundDeliveryResponse,
};
use atm_core::error::AtmError;

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "Retained for the sealed non-Claude boundary implementation and targeted tests while production wiring stays on the retained runtime seam."
    )
)]
type OutputPathFactory = Arc<dyn Fn() -> Result<PathBuf, AtmError> + Send + Sync>;
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "Retained for the sealed non-Claude boundary implementation and targeted tests while production wiring stays on the retained runtime seam."
    )
)]
#[derive(Clone)]
pub(crate) struct DaemonNonClaudeOutbound {
    path_factory: OutputPathFactory,
}

impl std::fmt::Debug for DaemonNonClaudeOutbound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DaemonNonClaudeOutbound(..)")
    }
}

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "Retained for the sealed non-Claude boundary implementation and targeted tests while production wiring stays on the retained runtime seam."
    )
)]
impl DaemonNonClaudeOutbound {
    #[allow(dead_code)]
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
        // Daemon uses a thread-per-connection IPC model (`std::thread`), not
        // an async runtime. `spawn_blocking` is not applicable here; blocking
        // filesystem I/O is appropriate in this context.
        // Payload size is bounded upstream by `MAX_DAEMON_FRAME_BYTES` in
        // `atm_core::protocol`.
        let output_path = (self.path_factory)()?;
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

        let mut bytes = serde_json::to_vec(&request)?;
        bytes.push(b'\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&output_path)
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
        // Blocking filesystem I/O here is bounded by
        // `MAX_CONCURRENT_CONNECTIONS` (64) connections. A filesystem stall
        // holds one thread; `REQUEST_DEADLINE` bounds socket I/O only. The
        // connection ceiling is the accepted defense against thread
        // exhaustion.
        file.write_all(&bytes).map_err(|error| {
            AtmError::mailbox_write(format!(
                "failed to append non-Claude outbound payload {}: {error}",
                output_path.display()
            ))
            .with_recovery(
                "Check that ATM_HOME is writable and that the host runtime directory has available disk space before retrying non-Claude delivery.",
            )
            .with_source(error)
        })?;
        // The same connection-ceiling defense applies to `sync_data()`: a
        // stalled durability flush consumes one bounded thread-per-connection
        // slot rather than creating unbounded worker growth.
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

        Ok(NonClaudeOutboundDeliveryResponse {
            delivered_messages: request.messages.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::DaemonNonClaudeOutbound;
    use atm_core::boundary::{NonClaudeOutbound, NonClaudeOutboundDeliveryRequest};
    use atm_core::schema::{AtmMessageId, MessageEnvelope};
    use atm_core::test_support::{TEST_SENDER, TEST_TEAM};
    use atm_core::types::IsoTimestamp;
    use serde_json::Map;
    use tempfile::TempDir;

    fn request() -> NonClaudeOutboundDeliveryRequest {
        NonClaudeOutboundDeliveryRequest {
            team: TEST_TEAM.parse().expect("team"),
            agent: "recipient".parse().expect("agent"),
            recipient_pane_id: Some("pane-1".to_string()),
            messages: vec![MessageEnvelope {
                from: TEST_SENDER.parse().expect("sender"),
                text: "hello".to_string(),
                timestamp: IsoTimestamp::now(),
                read: false,
                source_team: Some(TEST_TEAM.parse().expect("source team")),
                summary: Some("hello".to_string()),
                message_id: Some(AtmMessageId::new()),
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
}
