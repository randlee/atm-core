//! Tokio-only mailbox reader port.
//!
//! AV.1a constructs this port but does not route an HTTP handler through it;
//! AV.1b owns that atomic cutover.  Keeping the deadline translation here
//! prevents a forbidden `atm-storage -> atm-core` dependency.

use std::sync::Arc;

use atm_core::api::RequestDeadline;
use atm_storage::{
    AsyncMailboxReader, AsyncMessageStore, AtmError, MailboxScope, Message, MessageKey,
    MessageQuery, ReadDeadline, ReadLaneError,
};

#[allow(
    async_fn_in_trait,
    reason = "The Tokio-only port is an in-repository composition seam; callers do not implement it."
)]
pub trait AsyncMailboxRuntime: Send + Sync {
    async fn list_mail(
        &self,
        scope: MailboxScope,
        query: MessageQuery,
        deadline: RequestDeadline,
    ) -> Result<Vec<Message>, AtmError>;

    async fn peek_mail(
        &self,
        scope: MailboxScope,
        key: MessageKey,
        deadline: RequestDeadline,
    ) -> Result<Option<Message>, AtmError>;

    async fn read_mail(
        &self,
        scope: MailboxScope,
        key: MessageKey,
        deadline: RequestDeadline,
    ) -> Result<Option<Message>, AtmError>;
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
}

impl AsyncMailboxRuntime for StorageAsyncMailboxRuntime {
    async fn list_mail(
        &self,
        scope: MailboxScope,
        query: MessageQuery,
        deadline: RequestDeadline,
    ) -> Result<Vec<Message>, AtmError> {
        self.reader
            .list_messages(scope, query, read_deadline(deadline)?)
            .await
            .map_err(read_error)
    }

    async fn peek_mail(
        &self,
        scope: MailboxScope,
        key: MessageKey,
        deadline: RequestDeadline,
    ) -> Result<Option<Message>, AtmError> {
        self.reader
            .load_message(scope, key, read_deadline(deadline)?)
            .await
            .map_err(read_error)
    }

    async fn read_mail(
        &self,
        scope: MailboxScope,
        key: MessageKey,
        deadline: RequestDeadline,
    ) -> Result<Option<Message>, AtmError> {
        self.peek_mail(scope, key, deadline).await
    }
}

fn read_deadline(deadline: RequestDeadline) -> Result<ReadDeadline, AtmError> {
    deadline
        .remaining()
        .ok_or_else(|| AtmError::daemon_unavailable("mailbox request deadline expired"))
        .and_then(ReadDeadline::new)
}

fn read_error(error: ReadLaneError) -> AtmError {
    AtmError::daemon_unavailable(error.to_string())
}
