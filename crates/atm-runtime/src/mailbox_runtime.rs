//! Tokio-only mailbox reader port.
//!
//! AV.1a constructs this port but does not route an HTTP handler through it;
//! AV.1b owns that atomic cutover.  Keeping the deadline translation here
//! prevents a forbidden `atm-storage -> atm-core` dependency.

use std::sync::Arc;

use atm_core::api::RequestDeadline;
use atm_core::read::selection::{
    MailboxSelectionCandidate, MailboxSelectionRequest, MailboxSelectionResult,
    select_mailbox_candidates,
};
use atm_storage::{
    AsyncMailboxReader, AsyncMessageStore, AtmError, MailboxScope, Message, MessageKey,
    MessageQuery, ReadDeadline,
};

#[allow(
    async_fn_in_trait,
    reason = "The Tokio-only port is an in-repository composition seam; callers do not implement it."
)]
pub trait AsyncMailboxRuntime: Send + Sync {
    async fn list_mail(
        &self,
        scope: MailboxScope,
        request: MailboxSelectionRequest,
        deadline: RequestDeadline,
    ) -> Result<MailboxSelectionResult, AtmError>;

    async fn peek_mail(
        &self,
        scope: MailboxScope,
        key: MessageKey,
        request: MailboxSelectionRequest,
        deadline: RequestDeadline,
    ) -> Result<MailboxSelectionResult, AtmError>;

    async fn read_mail(
        &self,
        scope: MailboxScope,
        key: MessageKey,
        request: MailboxSelectionRequest,
        deadline: RequestDeadline,
    ) -> Result<MailboxSelectionResult, AtmError>;
}

/// Composition-owned implementation.  The writer handle is intentionally
/// retained although AV.1a performs no read-state mutation; AV.1b uses that
/// explicit handoff rather than smuggling a writer through the reader API.
#[derive(Clone)]
pub struct StorageAsyncMailboxRuntime {
    reader: Arc<dyn AsyncMailboxReader + Send + Sync>,
    _writer_lane: Arc<dyn AsyncMessageStore + Send + Sync>,
}

impl StorageAsyncMailboxRuntime {
    #[must_use]
    pub fn new(
        reader: Arc<dyn AsyncMailboxReader + Send + Sync>,
        writer_lane: Arc<dyn AsyncMessageStore + Send + Sync>,
    ) -> Self {
        Self {
            reader,
            _writer_lane: writer_lane,
        }
    }

    #[cfg(test)]
    fn new_for_reader_test(reader: Arc<dyn AsyncMailboxReader + Send + Sync>) -> Self {
        // This constructor proves the reader port cannot accidentally depend
        // on the writer lane before AV.1b owns read-state mutation.
        Self {
            reader,
            _writer_lane: Arc::new(TestOnlyWriterLane),
        }
    }
}

impl AsyncMailboxRuntime for StorageAsyncMailboxRuntime {
    async fn list_mail(
        &self,
        scope: MailboxScope,
        request: MailboxSelectionRequest,
        deadline: RequestDeadline,
    ) -> Result<MailboxSelectionResult, AtmError> {
        let messages = self
            .reader
            .list_messages(scope.clone(), query(&scope), read_deadline(deadline)?)
            .await
            .map_err(AtmError::from)?;
        Ok(select_mailbox_candidates(
            messages.into_iter().map(selection_candidate).collect(),
            &request,
        ))
    }

    async fn peek_mail(
        &self,
        scope: MailboxScope,
        key: MessageKey,
        request: MailboxSelectionRequest,
        deadline: RequestDeadline,
    ) -> Result<MailboxSelectionResult, AtmError> {
        self.select_single(scope, key, request, read_deadline(deadline)?)
            .await
    }

    async fn read_mail(
        &self,
        scope: MailboxScope,
        key: MessageKey,
        request: MailboxSelectionRequest,
        deadline: RequestDeadline,
    ) -> Result<MailboxSelectionResult, AtmError> {
        self.select_single(scope, key, request, read_deadline(deadline)?)
            .await
    }
}

impl StorageAsyncMailboxRuntime {
    async fn select_single(
        &self,
        scope: MailboxScope,
        key: MessageKey,
        request: MailboxSelectionRequest,
        deadline: ReadDeadline,
    ) -> Result<MailboxSelectionResult, AtmError> {
        let message = self
            .reader
            .load_message(scope, key, deadline)
            .await
            .map_err(AtmError::from)?;
        Ok(select_mailbox_candidates(
            message.into_iter().map(selection_candidate).collect(),
            &request,
        ))
    }
}

fn query(scope: &MailboxScope) -> MessageQuery {
    MessageQuery {
        team: scope.team.clone(),
        agent: scope.agent.clone(),
        sender: None,
        task_id: None,
        limit: None,
    }
}

fn selection_candidate(message: Message) -> MailboxSelectionCandidate {
    MailboxSelectionCandidate {
        message_key: message.message_key.to_string(),
        envelope: message.envelope,
    }
}

fn read_deadline(deadline: RequestDeadline) -> Result<ReadDeadline, AtmError> {
    deadline
        .remaining()
        .ok_or_else(|| AtmError::daemon_unavailable("mailbox request deadline expired"))
        .and_then(ReadDeadline::new)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use atm_core::read::selection::MailboxSelectionRequest;
    use atm_storage::testing::InMemoryMailboxReader;
    use atm_storage::{
        AgentName, AtmError, AtmErrorCode, IsoTimestamp, Message, MessageEnvelope, ReadLaneError,
        TeamName,
    };

    use super::{AsyncMailboxRuntime, StorageAsyncMailboxRuntime};
    use crate::mailbox_runtime::RequestDeadline;

    fn scope() -> atm_storage::MailboxScope {
        atm_storage::MailboxScope::new(
            "team".parse::<TeamName>().expect("team"),
            "agent".parse::<AgentName>().expect("agent"),
        )
    }

    fn message(key: &str, read: bool) -> Message {
        let scope = scope();
        Message {
            team: scope.team,
            agent: scope.agent,
            message_key: key.parse().expect("key"),
            envelope: MessageEnvelope {
                from: "sender".parse().expect("sender"),
                source_chat_id: None,
                text: "body".to_owned(),
                timestamp: IsoTimestamp::now(),
                read,
                source_team: None,
                destination_chat_id: None,
                summary: None,
                message_id: None,
                requires_ack: false,
                pending_ack_at: None,
                acknowledged_at: None,
                acknowledges_message_id: None,
                parent_message_id: None,
                thread_mode: None,
                expires_at: None,
                task_id: None,
                extra: serde_json::Map::new(),
            },
        }
    }

    fn runtime(messages: Vec<Message>) -> StorageAsyncMailboxRuntime {
        let reader = Arc::new(InMemoryMailboxReader::with_messages(messages));
        StorageAsyncMailboxRuntime::new_for_reader_test(reader)
    }

    #[tokio::test]
    async fn list_peek_and_read_share_visibility_and_missing_behavior() {
        let runtime = runtime(vec![message("unread", false), message("history", true)]);
        let deadline = RequestDeadline::after(Duration::from_secs(1));
        let listed = runtime
            .list_mail(scope(), MailboxSelectionRequest::default(), deadline)
            .await
            .expect("list");
        assert_eq!(listed.bucket_counts.unread, 1);
        assert_eq!(listed.selected.len(), 1);

        let peeked = runtime
            .peek_mail(
                scope(),
                "unread".parse().expect("key"),
                MailboxSelectionRequest::default(),
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("peek");
        assert_eq!(peeked.selected.len(), 1);

        let missing = runtime
            .read_mail(
                scope(),
                "missing".parse().expect("key"),
                MailboxSelectionRequest::default(),
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("read");
        assert!(missing.selected.is_empty());
    }

    #[test]
    fn reader_lane_error_conversion_preserves_distinct_codes_and_causes() {
        let cases = [
            (
                ReadLaneError::UnauthorizedScope,
                AtmErrorCode::MailboxReadFailed,
            ),
            (
                ReadLaneError::Saturated { reason: "test" },
                AtmErrorCode::DaemonConnectionSaturated,
            ),
            (
                ReadLaneError::DeadlineExpired { stage: "test" },
                AtmErrorCode::MailboxLockTimeout,
            ),
            (
                ReadLaneError::Unavailable {
                    message: "test".to_owned(),
                },
                AtmErrorCode::DaemonUnavailable,
            ),
        ];
        for (lane_error, expected_code) in cases {
            let error = AtmError::from(lane_error);
            assert_eq!(error.code(), expected_code);
            assert!(error.cause().is_some());
        }
    }
}

#[cfg(test)]
#[derive(Debug)]
struct TestOnlyWriterLane;

#[cfg(test)]
impl atm_storage::contract::sealed::Sealed for TestOnlyWriterLane {}

#[cfg(test)]
impl atm_storage::MessageStore for TestOnlyWriterLane {
    fn save_message(&self, _message: &Message) -> Result<(), AtmError> {
        unreachable!("reader-port test double must not use the writer lane")
    }

    fn save_messages_atomically(&self, _messages: &[Message]) -> Result<(), AtmError> {
        unreachable!("reader-port test double must not use the writer lane")
    }

    fn load_message(&self, _key: &MessageKey) -> Result<Option<Message>, AtmError> {
        unreachable!("reader-port test double must not use the writer lane")
    }

    fn list_messages(&self, _query: &MessageQuery) -> Result<Vec<Message>, AtmError> {
        unreachable!("reader-port test double must not use the writer lane")
    }

    fn delete_message(&self, _key: &MessageKey) -> Result<(), AtmError> {
        unreachable!("reader-port test double must not use the writer lane")
    }
}

#[cfg(test)]
impl AsyncMessageStore for TestOnlyWriterLane {}
