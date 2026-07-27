use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use atm_core::boundary::{
    self, NonClaudeOutboundDeliveryRequest, NonClaudeOutboundDeliveryResponse,
};
use atm_core::error::AtmError;

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
            )));
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
    })?;
    std::fs::create_dir_all(parent).map_err(|error| {
        AtmError::mailbox_write(format!(
            "failed to create non-Claude outbound directory {}: {error}",
            parent.display()
        ))
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
        })?;
    // MAX_CONCURRENT_CONNECTIONS bounds callers here; FS stall under that
    // ceiling is accepted delivery latency.
    file.write_all(bytes).map_err(|error| {
        AtmError::mailbox_write(format!(
            "failed to append non-Claude outbound payload {}: {error}",
            output_path.display()
        ))
    })?;
    // MAX_CONCURRENT_CONNECTIONS bounds callers here; FS stall under that
    // ceiling is accepted delivery latency.
    file.sync_data().map_err(|error| {
        AtmError::mailbox_write(format!(
            "failed to sync non-Claude outbound payload {}: {error}",
            output_path.display()
        ))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::DaemonNonClaudeOutbound;
    use atm_core::boundary::{NonClaudeOutbound, NonClaudeOutboundDeliveryRequest};
    use atm_core::schema::{AtmMessageId, InboxMessage};
    use atm_core::test_support::{TEST_SENDER, TEST_TEAM};
    use atm_core::types::IsoTimestamp;
    use serde_json::Map;
    use tempfile::TempDir;

    fn request() -> NonClaudeOutboundDeliveryRequest {
        NonClaudeOutboundDeliveryRequest {
            team: TEST_TEAM.parse().expect("team"),
            agent: "recipient".parse().expect("agent"),
            recipient_pane_id: Some(atm_core::types::PaneId::new("pane-1").expect("pane")),
            messages: vec![InboxMessage {
                from: TEST_SENDER.parse().expect("sender"),
                source_chat_id: None,
                text: "hello".to_string(),
                timestamp: IsoTimestamp::now(),
                read: false,
                source_team: Some(TEST_TEAM.parse().expect("source team")),
                destination_chat_id: None,
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
            recipient_pane_id: Some(atm_core::types::PaneId::new("pane-1").expect("pane")),
            messages: vec![InboxMessage {
                from: TEST_SENDER.parse().expect("sender"),
                source_chat_id: None,
                text: "x".repeat(1024 * 1024),
                timestamp: IsoTimestamp::now(),
                read: false,
                source_team: Some(TEST_TEAM.parse().expect("source team")),
                destination_chat_id: None,
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
}
