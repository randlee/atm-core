//! Tokio-only mailbox reader port.
//!
//! AV.1a constructs this port but does not route an HTTP handler through it;
//! AV.1b owns that atomic cutover.  Keeping the deadline translation here
//! prevents a forbidden `atm-storage -> atm-core` dependency.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use atm_core::api::RequestDeadline;
use atm_core::list::{AsyncListCommand, ListOutcome, complete_async_list};
use atm_core::read::ReadOutcome;
use atm_core::read::async_projection::{AsyncReadCommand, complete_async_read};
use atm_core::read::selection::{
    MailboxSelectionCandidate, MailboxSelectionRequest, MailboxSelectionResult,
    select_mailbox_candidates, sort_and_limit_mailbox_selection,
};
use atm_storage::{
    AsyncMailboxReader, AsyncMessageStore, AtmError, IsoTimestamp, MailboxScope, Message,
    MessageKey, MessageQuery, ReadDeadline,
};

/// Bounded, composition-owned handoff settings for post-read state updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandoffConfig {
    pub handoff_buffer: usize,
    pub handoff_retry_deadline: Duration,
    pub supervisor_max_restarts: u32,
}

impl Default for HandoffConfig {
    fn default() -> Self {
        Self {
            handoff_buffer: 64,
            handoff_retry_deadline: Duration::from_secs(5),
            supervisor_max_restarts: 3,
        }
    }
}

/// Explicit, observable rejection from the non-blocking read-side handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoffRejected {
    BufferFull,
    Unavailable,
}

/// The supervisor's externally observable availability state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorState {
    Ready,
    Unavailable,
    Restarting,
}

/// Value diagnostics sampled by doctor without taking the writer lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateHandoffDiagnostics {
    pub state: SupervisorState,
    pub buffered_depth: usize,
    pub restart_count: u32,
    pub rejected_buffer_full: u64,
    pub rejected_unavailable: u64,
    pub retry_deadline_exhaustions: u64,
}

#[derive(Debug, Clone)]
struct ReadDisplayTransition {
    scope: MailboxScope,
    message_ids: Vec<MessageKey>,
    seen_watermark: Option<IsoTimestamp>,
}

/// Owns the only read-to-writer handoff. `try_push` never awaits writer
/// admission or execution; the dedicated task owns retries and durability.
#[derive(Clone)]
pub struct StateHandoffSupervisor {
    inner: Arc<StateHandoffInner>,
    manager: Arc<ManagerTask>,
}

struct StateHandoffInner {
    config: HandoffConfig,
    writer: Arc<dyn AsyncMessageStore + Send + Sync>,
    queue: Mutex<VecDeque<ReadDisplayTransition>>,
    queue_changed: tokio::sync::Notify,
    state: std::sync::atomic::AtomicU8,
    restart_count: std::sync::atomic::AtomicU32,
    rejected_buffer_full: std::sync::atomic::AtomicU64,
    rejected_unavailable: std::sync::atomic::AtomicU64,
    retry_deadline_exhaustions: std::sync::atomic::AtomicU64,
    active_worker: Mutex<Option<tokio::task::AbortHandle>>,
}

/// Owns the manager handle outside the state shared with the task itself.
///
/// Keeping this handle in `StateHandoffInner` would form an `Arc` cycle:
/// manager task -> inner -> its own JoinHandle.  This separate owner aborts
/// the task when the final runtime clone goes away and therefore releases the
/// reader/writer boundary promptly in test and production shutdown paths.
struct ManagerTask {
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for ManagerTask {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

impl StateHandoffSupervisor {
    pub fn start(
        config: HandoffConfig,
        writer: Arc<dyn AsyncMessageStore + Send + Sync>,
    ) -> Result<Self, AtmError> {
        if config.handoff_buffer == 0 || config.handoff_retry_deadline.is_zero() {
            return Err(AtmError::validation(
                "state-handoff buffer and retry deadline must be non-zero",
            ));
        }
        tokio::runtime::Handle::try_current().map_err(|_| {
            AtmError::daemon_unavailable(
                "state-handoff supervisor must start inside the Tokio runtime",
            )
        })?;
        let inner = Arc::new(StateHandoffInner {
            config,
            writer,
            queue: Mutex::new(VecDeque::with_capacity(config.handoff_buffer)),
            queue_changed: tokio::sync::Notify::new(),
            state: std::sync::atomic::AtomicU8::new(STATE_READY),
            restart_count: std::sync::atomic::AtomicU32::new(0),
            rejected_buffer_full: std::sync::atomic::AtomicU64::new(0),
            rejected_unavailable: std::sync::atomic::AtomicU64::new(0),
            retry_deadline_exhaustions: std::sync::atomic::AtomicU64::new(0),
            active_worker: Mutex::new(None),
        });
        let manager_inner = Arc::clone(&inner);
        let manager = Arc::new(ManagerTask {
            handle: tokio::spawn(async move { supervise_handoff_worker(manager_inner).await }),
        });
        Ok(Self { inner, manager })
    }

    pub fn try_push(
        &self,
        scope: MailboxScope,
        message_ids: Vec<MessageKey>,
        seen_watermark: Option<IsoTimestamp>,
    ) -> Result<(), HandoffRejected> {
        if self.state() != SupervisorState::Ready {
            self.inner
                .rejected_unavailable
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Err(HandoffRejected::Unavailable);
        }
        let mut queue = self
            .inner
            .queue
            .lock()
            .expect("state-handoff queue mutex poisoned");
        if self.state() != SupervisorState::Ready {
            self.inner
                .rejected_unavailable
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Err(HandoffRejected::Unavailable);
        }
        if queue.len() >= self.inner.config.handoff_buffer {
            self.inner
                .rejected_buffer_full
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Err(HandoffRejected::BufferFull);
        }
        queue.push_back(ReadDisplayTransition {
            scope,
            message_ids,
            seen_watermark,
        });
        drop(queue);
        self.inner.queue_changed.notify_one();
        Ok(())
    }

    #[must_use]
    pub fn state(&self) -> SupervisorState {
        match self.inner.state.load(std::sync::atomic::Ordering::Acquire) {
            STATE_READY => SupervisorState::Ready,
            STATE_RESTARTING => SupervisorState::Restarting,
            _ => SupervisorState::Unavailable,
        }
    }

    #[must_use]
    pub fn buffered_depth(&self) -> usize {
        self.inner
            .queue
            .lock()
            .expect("state-handoff queue mutex poisoned")
            .len()
    }

    #[must_use]
    pub fn restart_count(&self) -> u32 {
        self.inner
            .restart_count
            .load(std::sync::atomic::Ordering::Acquire)
    }

    #[must_use]
    pub fn diagnostics(&self) -> StateHandoffDiagnostics {
        StateHandoffDiagnostics {
            state: self.state(),
            buffered_depth: self.buffered_depth(),
            restart_count: self.restart_count(),
            rejected_buffer_full: self
                .inner
                .rejected_buffer_full
                .load(std::sync::atomic::Ordering::Relaxed),
            rejected_unavailable: self
                .inner
                .rejected_unavailable
                .load(std::sync::atomic::Ordering::Relaxed),
            retry_deadline_exhaustions: self
                .inner
                .retry_deadline_exhaustions
                .load(std::sync::atomic::Ordering::Relaxed),
        }
    }

    /// Reports whether the composition-owned supervisor task has exited.
    /// This is intentionally separate from the handoff state: a manager can
    /// still be alive while it is restarting a worker.
    #[must_use]
    pub fn manager_finished(&self) -> bool {
        self.manager.handle.is_finished()
    }

    #[cfg(test)]
    fn abort_active_worker_for_test(&self) -> bool {
        let active_worker = self
            .inner
            .active_worker
            .lock()
            .expect("state-handoff worker mutex poisoned");
        let Some(worker) = active_worker.as_ref() else {
            return false;
        };
        worker.abort();
        true
    }
}

async fn supervise_handoff_worker(inner: Arc<StateHandoffInner>) {
    loop {
        let worker_inner = Arc::clone(&inner);
        let worker = tokio::spawn(async move { run_handoff_worker(worker_inner).await });
        *inner
            .active_worker
            .lock()
            .expect("state-handoff worker mutex poisoned") = Some(worker.abort_handle());
        match worker.await {
            Ok(Ok(())) => {
                inner
                    .state
                    .store(STATE_UNAVAILABLE, std::sync::atomic::Ordering::Release);
                return;
            }
            Ok(Err(error)) => {
                inner
                    .state
                    .store(STATE_UNAVAILABLE, std::sync::atomic::Ordering::Release);
                tracing::error!(%error, "mailbox read-state handoff retry deadline exhausted");
                return;
            }
            Err(error) => {
                let restart = inner
                    .restart_count
                    .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
                    + 1;
                if restart > inner.config.supervisor_max_restarts {
                    inner
                        .state
                        .store(STATE_UNAVAILABLE, std::sync::atomic::Ordering::Release);
                    tracing::error!(%error, restart, "mailbox read-state handoff restart budget exhausted");
                    return;
                }
                inner
                    .state
                    .store(STATE_RESTARTING, std::sync::atomic::Ordering::Release);
                tracing::warn!(%error, restart, "restarting mailbox read-state handoff worker");
                tokio::task::yield_now().await;
                inner
                    .state
                    .store(STATE_READY, std::sync::atomic::Ordering::Release);
            }
        }
        *inner
            .active_worker
            .lock()
            .expect("state-handoff worker mutex poisoned") = None;
    }
}

async fn run_handoff_worker(inner: Arc<StateHandoffInner>) -> Result<(), AtmError> {
    loop {
        let transition = loop {
            if let Some(transition) = inner
                .queue
                .lock()
                .expect("state-handoff queue mutex poisoned")
                .front()
                .cloned()
            {
                break transition;
            }
            inner.queue_changed.notified().await;
        };
        let retry_deadline = tokio::time::Instant::now() + inner.config.handoff_retry_deadline;
        loop {
            match inner
                .writer
                .apply_read_display_state_async(
                    transition.scope.clone(),
                    transition.message_ids.clone(),
                    transition.seen_watermark,
                )
                .await
            {
                Ok(()) => {
                    let removed = inner
                        .queue
                        .lock()
                        .expect("state-handoff queue mutex poisoned")
                        .pop_front();
                    debug_assert!(removed.is_some(), "worker owns the front transition");
                    break;
                }
                Err(error) if tokio::time::Instant::now() >= retry_deadline => {
                    inner
                        .retry_deadline_exhaustions
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    return Err(error);
                }
                Err(error) => {
                    tracing::warn!(%error, "retrying mailbox read-state handoff");
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }
    }
}

const STATE_READY: u8 = 0;
const STATE_UNAVAILABLE: u8 = 1;
const STATE_RESTARTING: u8 = 2;

#[async_trait::async_trait]
pub trait AsyncMailboxRuntime: Send + Sync {
    fn handoff_diagnostics(&self) -> Option<StateHandoffDiagnostics>;

    async fn list_command(
        &self,
        command: AsyncListCommand,
        deadline: RequestDeadline,
    ) -> Result<ListOutcome, AtmError>;

    async fn peek_command(
        &self,
        command: AsyncReadCommand,
        deadline: RequestDeadline,
    ) -> Result<ReadOutcome, AtmError>;

    async fn read_command(
        &self,
        command: AsyncReadCommand,
        deadline: RequestDeadline,
    ) -> Result<ReadOutcome, AtmError>;

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

/// Composition-owned implementation.  After supervisor startup, it retains
/// only the explicit handoff rather than a direct writer-lane handle.
#[derive(Clone)]
pub struct StorageAsyncMailboxRuntime {
    reader: Arc<dyn AsyncMailboxReader + Send + Sync>,
    state_handoff: StateHandoffLifecycle,
}

/// Before startup composition temporarily holds the writer ingress needed to
/// create the supervisor. After startup mailbox reads have no direct writer
/// handle at all.
#[derive(Clone)]
enum StateHandoffLifecycle {
    Unstarted(Arc<dyn AsyncMessageStore + Send + Sync>),
    Active(StateHandoffSupervisor),
}

impl StorageAsyncMailboxRuntime {
    #[must_use]
    pub fn new(
        reader: Arc<dyn AsyncMailboxReader + Send + Sync>,
        writer_lane: Arc<dyn AsyncMessageStore + Send + Sync>,
    ) -> Self {
        Self {
            reader,
            state_handoff: StateHandoffLifecycle::Unstarted(writer_lane),
        }
    }

    /// Starts the composition-owned writer handoff before read handlers are
    /// admitted.  Construction fails closed if no Tokio runtime is present.
    pub fn with_state_handoff(mut self, config: HandoffConfig) -> Result<Self, AtmError> {
        let writer = match &self.state_handoff {
            StateHandoffLifecycle::Unstarted(writer) => Arc::clone(writer),
            StateHandoffLifecycle::Active(_) => {
                return Err(AtmError::validation(
                    "mailbox state-handoff supervisor was started more than once",
                ));
            }
        };
        self.state_handoff =
            StateHandoffLifecycle::Active(StateHandoffSupervisor::start(config, writer)?);
        Ok(self)
    }

    #[cfg(test)]
    fn new_for_reader_test(reader: Arc<dyn AsyncMailboxReader + Send + Sync>) -> Self {
        // This constructor proves the reader port cannot accidentally depend
        // on the writer lane before AV.1b owns read-state mutation.
        Self {
            reader,
            state_handoff: StateHandoffLifecycle::Unstarted(Arc::new(TestOnlyWriterLane)),
        }
    }
}

#[async_trait::async_trait]
impl AsyncMailboxRuntime for StorageAsyncMailboxRuntime {
    fn handoff_diagnostics(&self) -> Option<StateHandoffDiagnostics> {
        match &self.state_handoff {
            StateHandoffLifecycle::Unstarted(_) => None,
            StateHandoffLifecycle::Active(handoff) => Some(handoff.diagnostics()),
        }
    }

    async fn list_command(
        &self,
        command: AsyncListCommand,
        deadline: RequestDeadline,
    ) -> Result<ListOutcome, AtmError> {
        let command = self.authorize_list(command, deadline).await?;
        let selection = self
            .select_all(
                command.scope().clone(),
                command.selection().clone(),
                deadline,
            )
            .await?;
        Ok(complete_async_list(&command, selection))
    }

    async fn peek_command(
        &self,
        command: AsyncReadCommand,
        deadline: RequestDeadline,
    ) -> Result<ReadOutcome, AtmError> {
        let command = self.authorize_read(command, deadline).await?;
        let selection = self
            .select_for_read(
                command.scope().clone(),
                command.selection().clone(),
                deadline,
                command.wait_timeout(),
            )
            .await?;
        let match_count = selection.selected.len();
        let mut selection = selection;
        sort_and_limit_mailbox_selection(&mut selection.selected, Some(1));
        Ok(complete_async_read(&command, selection, match_count, false))
    }

    async fn read_command(
        &self,
        command: AsyncReadCommand,
        deadline: RequestDeadline,
    ) -> Result<ReadOutcome, AtmError> {
        let command = self.authorize_read(command, deadline).await?;
        let selection = self
            .select_for_read(
                command.scope().clone(),
                command.selection().clone(),
                deadline,
                command.wait_timeout(),
            )
            .await?;
        let match_count = selection.selected.len();
        let mut selection = selection;
        sort_and_limit_mailbox_selection(&mut selection.selected, Some(1));
        let accepted = self.try_handoff_selected(
            command.scope().clone(),
            &selection,
            command.updates_seen_state(),
        )?;
        Ok(complete_async_read(
            &command,
            selection,
            match_count,
            accepted,
        ))
    }

    async fn list_mail(
        &self,
        scope: MailboxScope,
        request: MailboxSelectionRequest,
        deadline: RequestDeadline,
    ) -> Result<MailboxSelectionResult, AtmError> {
        self.select_all(scope, request, deadline).await
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
        let selection = self
            .select_single(scope.clone(), key, request, read_deadline(deadline)?)
            .await?;
        self.try_handoff_selected(scope, &selection, true)?;
        Ok(selection)
    }
}

impl StorageAsyncMailboxRuntime {
    async fn authorize_list(
        &self,
        mut command: AsyncListCommand,
        deadline: RequestDeadline,
    ) -> Result<AsyncListCommand, AtmError> {
        if command.explicit_target()
            && !self
                .reader
                .mailbox_member_exists(command.scope().clone(), read_deadline(deadline)?)
                .await
                .map_err(AtmError::from)?
        {
            return Err(AtmError::agent_not_found(
                &command.scope().agent,
                &command.scope().team,
            ));
        }
        if command.requires_seen_watermark() {
            let watermark = self
                .reader
                .load_seen_watermark(command.scope().clone(), read_deadline(deadline)?)
                .await
                .map_err(AtmError::from)?;
            command = command.with_seen_watermark(watermark);
        }
        Ok(command)
    }

    async fn authorize_read(
        &self,
        mut command: AsyncReadCommand,
        deadline: RequestDeadline,
    ) -> Result<AsyncReadCommand, AtmError> {
        if command.explicit_target()
            && !self
                .reader
                .mailbox_member_exists(command.scope().clone(), read_deadline(deadline)?)
                .await
                .map_err(AtmError::from)?
        {
            return Err(AtmError::agent_not_found(
                &command.scope().agent,
                &command.scope().team,
            ));
        }
        if command.requires_seen_watermark() {
            let watermark = self
                .reader
                .load_seen_watermark(command.scope().clone(), read_deadline(deadline)?)
                .await
                .map_err(AtmError::from)?;
            command = command.with_seen_watermark(watermark);
        }
        Ok(command)
    }

    async fn select_all(
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

    async fn select_for_read(
        &self,
        scope: MailboxScope,
        request: MailboxSelectionRequest,
        deadline: RequestDeadline,
        wait_timeout: Option<Duration>,
    ) -> Result<MailboxSelectionResult, AtmError> {
        let Some(wait_timeout) = wait_timeout else {
            return self.select_all(scope, request, deadline).await;
        };
        let wait_deadline = tokio::time::Instant::now() + wait_timeout;
        loop {
            let selection = self
                .select_all(scope.clone(), request.clone(), deadline)
                .await?;
            if !selection.selected.is_empty() || tokio::time::Instant::now() >= wait_deadline {
                return Ok(selection);
            }
            let request_remaining = deadline.remaining().ok_or_else(|| {
                AtmError::daemon_unavailable("mailbox request deadline expired while awaiting mail")
            })?;
            let until_wait_deadline =
                wait_deadline.saturating_duration_since(tokio::time::Instant::now());
            let pause = Duration::from_millis(25)
                .min(request_remaining)
                .min(until_wait_deadline);
            if pause.is_zero() {
                return Ok(selection);
            }
            tokio::time::sleep(pause).await;
        }
    }

    fn try_handoff_selected(
        &self,
        scope: MailboxScope,
        selection: &MailboxSelectionResult,
        update_seen_watermark: bool,
    ) -> Result<bool, AtmError> {
        let message_ids = selection
            .selected
            .iter()
            .filter_map(|message| message.message_key.parse::<MessageKey>().ok())
            .collect::<Vec<_>>();
        if message_ids.is_empty() {
            return Ok(false);
        }
        let StateHandoffLifecycle::Active(handoff) = &self.state_handoff else {
            return Err(AtmError::daemon_unavailable(
                "mailbox read handoff was not started before read admission",
            ));
        };
        let seen_watermark = update_seen_watermark
            .then(|| {
                selection
                    .selected
                    .iter()
                    .map(|message| message.envelope.timestamp)
                    .max()
            })
            .flatten();
        if let Err(rejected) = handoff.try_push(scope, message_ids, seen_watermark) {
            tracing::warn!(
                ?rejected,
                "mailbox read-state handoff rejected; message remains unread"
            );
            return Ok(false);
        }
        Ok(true)
    }

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
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use atm_core::list::{ListQuery, prepare_async_list};
    use atm_core::read::ReadQuery;
    use atm_core::read::async_projection::prepare_async_read;
    use atm_core::read::selection::MailboxSelectionRequest;
    use atm_core::types::ReadSelection;
    use atm_storage::testing::InMemoryMailboxReader;
    use atm_storage::{
        AgentName, AsyncMessageStore, AtmError, AtmErrorCode, IsoTimestamp, MailboxScope, Message,
        MessageEnvelope, MessageKey, MessageQuery, MessageStore, ReadLaneError, TeamName,
    };

    use super::{
        AsyncMailboxRuntime, HandoffConfig, HandoffRejected, StateHandoffSupervisor,
        StorageAsyncMailboxRuntime,
    };
    use crate::mailbox_runtime::RequestDeadline;

    fn scope() -> atm_storage::MailboxScope {
        atm_storage::MailboxScope::new(
            "team".parse::<TeamName>().expect("team"),
            "agent".parse::<AgentName>().expect("agent"),
        )
    }

    fn team() -> TeamName {
        "team".parse().expect("team")
    }

    fn agent() -> AgentName {
        "agent".parse().expect("agent")
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

    #[tokio::test]
    async fn handoff_returns_without_waiting_for_writer_execution() {
        let writer = Arc::new(RecordingWriter::default());
        let supervisor = StateHandoffSupervisor::start(
            HandoffConfig {
                handoff_buffer: 2,
                handoff_retry_deadline: Duration::from_secs(1),
                supervisor_max_restarts: 1,
            },
            writer.clone(),
        )
        .expect("supervisor starts");

        supervisor
            .try_push(scope(), vec!["unread".parse().expect("key")], None)
            .expect("synchronous handoff admission");
        assert!(
            writer.applied().is_empty(),
            "try_push never awaits writer work"
        );

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if writer.applied().len() == 1 {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("supervisor applies transition asynchronously");
        assert_eq!(supervisor.buffered_depth(), 0);
    }

    #[tokio::test]
    async fn handoff_rejects_full_buffer_without_blocking_the_read_path() {
        let supervisor = StateHandoffSupervisor::start(
            HandoffConfig {
                handoff_buffer: 1,
                handoff_retry_deadline: Duration::from_secs(1),
                supervisor_max_restarts: 1,
            },
            Arc::new(RecordingWriter::with_failures(usize::MAX)),
        )
        .expect("supervisor starts");

        supervisor
            .try_push(scope(), vec!["first".parse().expect("key")], None)
            .expect("first transition fits");
        assert_eq!(
            supervisor.try_push(scope(), vec!["second".parse().expect("key")], None),
            Err(HandoffRejected::BufferFull),
            "full queue is an explicit fail-safe rejection"
        );
        assert_eq!(
            supervisor.diagnostics().rejected_buffer_full,
            1,
            "doctor can surface every fail-safe full-buffer rejection"
        );
    }

    #[tokio::test]
    async fn handoff_retries_a_transient_writer_failure_without_losing_the_transition() {
        let writer = Arc::new(RecordingWriter::with_failures(1));
        let supervisor = StateHandoffSupervisor::start(
            HandoffConfig {
                handoff_buffer: 1,
                handoff_retry_deadline: Duration::from_secs(1),
                supervisor_max_restarts: 1,
            },
            writer.clone(),
        )
        .expect("supervisor starts");

        supervisor
            .try_push(scope(), vec!["retry".parse().expect("key")], None)
            .expect("transition enters buffer");
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if writer.applied().len() == 1 {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("transition survives transient writer failure");
        assert_eq!(writer.attempts(), 2);
        assert_eq!(supervisor.buffered_depth(), 0);
    }

    #[tokio::test]
    async fn runtime_read_returns_before_its_writer_state_followup_commits() {
        let writer = Arc::new(RecordingWriter::default());
        let runtime = StorageAsyncMailboxRuntime::new(
            Arc::new(InMemoryMailboxReader::with_messages(vec![message(
                "unread", false,
            )])),
            writer.clone(),
        )
        .with_state_handoff(HandoffConfig::default())
        .expect("runtime handoff starts");

        let response = runtime
            .read_mail(
                scope(),
                "unread".parse().expect("key"),
                MailboxSelectionRequest::default(),
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("read selection returns without writer completion");
        assert_eq!(response.selected.len(), 1);

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if writer.applied().len() == 1 {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("writer followup eventually commits");
    }

    #[tokio::test]
    async fn command_runtime_keeps_policy_in_core_and_writer_work_out_of_selection() {
        let writer = Arc::new(RecordingWriter::default());
        let runtime = StorageAsyncMailboxRuntime::new(
            Arc::new(InMemoryMailboxReader::with_messages(vec![message(
                "unread", false,
            )])),
            writer.clone(),
        )
        .with_state_handoff(HandoffConfig::default())
        .expect("runtime handoff starts");
        let query_root = PathBuf::from("/daemon-owned");
        let read = ReadQuery::new(
            query_root.clone(),
            query_root.clone(),
            agent(),
            None,
            team(),
            ReadSelection::Unread,
            false,
            true,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("read query");
        let outcome = runtime
            .read_command(
                prepare_async_read(&read).expect("core prepares read policy"),
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("runtime selects through reader lane");
        assert_eq!(outcome.count, 1);
        assert!(outcome.mutation_applied, "handoff admission is reported");

        let list = ListQuery::new(
            query_root.clone(),
            query_root,
            agent(),
            None,
            team(),
            ReadSelection::All,
            false,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("list query");
        let listed = runtime
            .list_command(
                prepare_async_list(&list).expect("core prepares list policy"),
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("runtime lists through reader lane");
        assert_eq!(listed.count, 1);
        assert_eq!(
            writer.applied().len(),
            0,
            "list never uses the writer handoff"
        );
    }

    #[tokio::test]
    async fn command_read_displays_and_handoffs_only_the_newest_message() {
        let writer = Arc::new(RecordingWriter::default());
        let runtime = StorageAsyncMailboxRuntime::new(
            Arc::new(InMemoryMailboxReader::with_messages(vec![
                message("first", false),
                message("second", false),
            ])),
            writer.clone(),
        )
        .with_state_handoff(HandoffConfig::default())
        .expect("runtime handoff starts");
        let query_root = PathBuf::from("/daemon-owned");
        let read = ReadQuery::new(
            query_root.clone(),
            query_root,
            agent(),
            None,
            team(),
            ReadSelection::Unread,
            false,
            true,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("read query");

        let outcome = runtime
            .read_command(
                prepare_async_read(&read).expect("core prepares read policy"),
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("read response");
        assert_eq!(outcome.count, 1);
        assert_eq!(outcome.match_count, 2, "response preserves all-match count");

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let applied = writer.applied();
                if !applied.is_empty() {
                    return applied;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("handoff applies")
        .iter()
        .for_each(|transition| {
            assert_eq!(transition.len(), 1, "only the displayed message is mutated");
        });
    }

    #[tokio::test]
    async fn handoff_fails_closed_after_a_permanent_writer_failure_without_discarding_queue() {
        let supervisor = StateHandoffSupervisor::start(
            HandoffConfig {
                handoff_buffer: 2,
                handoff_retry_deadline: Duration::from_millis(30),
                supervisor_max_restarts: 1,
            },
            Arc::new(RecordingWriter::with_failures(usize::MAX)),
        )
        .expect("supervisor starts");
        supervisor
            .try_push(scope(), vec!["unread".parse().expect("key")], None)
            .expect("first transition accepted");

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if matches!(supervisor.state(), super::SupervisorState::Unavailable) {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("permanent writer failure becomes an explicit runtime fault");
        assert_eq!(
            supervisor.buffered_depth(),
            1,
            "transition was not silently discarded"
        );
        assert_eq!(
            supervisor.try_push(scope(), vec!["later".parse().expect("key")], None),
            Err(HandoffRejected::Unavailable),
            "runtime fails closed after permanent writer failure"
        );
        let diagnostics = supervisor.diagnostics();
        assert_eq!(diagnostics.retry_deadline_exhaustions, 1);
        assert_eq!(diagnostics.rejected_unavailable, 1);
    }

    #[tokio::test]
    async fn handoff_restarts_after_a_forced_worker_exit_and_drains_the_retained_transition() {
        let writer = Arc::new(RecordingWriter::with_failures(usize::MAX));
        let supervisor = StateHandoffSupervisor::start(
            HandoffConfig {
                handoff_buffer: 2,
                handoff_retry_deadline: Duration::from_secs(1),
                supervisor_max_restarts: 1,
            },
            writer.clone(),
        )
        .expect("supervisor starts");
        supervisor
            .try_push(scope(), vec!["retry".parse().expect("key")], None)
            .expect("transition is retained before worker fault");

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if writer.attempts() > 0 {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("worker begins retrying the retained transition");
        assert!(
            supervisor.abort_active_worker_for_test(),
            "test forces the active worker to exit without touching the retained queue"
        );
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if supervisor.restart_count() == 1 {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("supervisor restarts a cancelled worker within its restart budget");
        writer.set_failures(0);
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if writer.applied().len() == 1 {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("restarted worker drains the preserved transition");
        assert_eq!(supervisor.buffered_depth(), 0);
    }

    #[tokio::test]
    async fn handoff_fails_closed_when_forced_worker_exit_exhausts_restart_budget() {
        let writer = Arc::new(RecordingWriter::with_failures(usize::MAX));
        let supervisor = StateHandoffSupervisor::start(
            HandoffConfig {
                handoff_buffer: 2,
                handoff_retry_deadline: Duration::from_secs(1),
                supervisor_max_restarts: 0,
            },
            writer.clone(),
        )
        .expect("supervisor starts");
        supervisor
            .try_push(scope(), vec!["retained".parse().expect("key")], None)
            .expect("transition is retained before worker fault");
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if writer.attempts() > 0 {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("worker begins retrying");
        assert!(supervisor.abort_active_worker_for_test());
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if matches!(supervisor.state(), super::SupervisorState::Unavailable) {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("restart-budget exhaustion becomes a runtime fault");
        assert_eq!(supervisor.buffered_depth(), 1, "fault never discards state");
        assert_eq!(
            supervisor.try_push(scope(), vec!["later".parse().expect("key")], None),
            Err(HandoffRejected::Unavailable),
            "future handoffs fail closed after restart-budget exhaustion"
        );
    }

    #[test]
    fn handoff_startup_fails_closed_without_a_tokio_runtime() {
        let result = StateHandoffSupervisor::start(
            HandoffConfig::default(),
            Arc::new(RecordingWriter::default()),
        );
        let Err(error) = result else {
            panic!("composition outside Tokio must not admit reads without a supervisor");
        };
        assert!(error.detail().contains("Tokio runtime"));
    }

    struct RecordingWriter {
        failures_remaining: std::sync::atomic::AtomicUsize,
        attempts: std::sync::atomic::AtomicUsize,
        applied_transitions: std::sync::Mutex<Vec<Vec<MessageKey>>>,
    }

    impl Default for RecordingWriter {
        fn default() -> Self {
            Self::with_failures(0)
        }
    }

    impl RecordingWriter {
        fn with_failures(failures: usize) -> Self {
            Self {
                failures_remaining: std::sync::atomic::AtomicUsize::new(failures),
                attempts: std::sync::atomic::AtomicUsize::new(0),
                applied_transitions: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn attempts(&self) -> usize {
            self.attempts.load(std::sync::atomic::Ordering::Acquire)
        }

        fn set_failures(&self, failures: usize) {
            self.failures_remaining
                .store(failures, std::sync::atomic::Ordering::Release);
        }

        fn applied(&self) -> Vec<Vec<MessageKey>> {
            self.applied_transitions
                .lock()
                .expect("recording writer mutex")
                .clone()
        }
    }

    impl atm_storage::contract::sealed::Sealed for RecordingWriter {}

    impl MessageStore for RecordingWriter {
        fn save_message(&self, _message: &Message) -> Result<(), AtmError> {
            unreachable!("handoff test writer only receives display transitions")
        }

        fn save_messages_atomically(&self, _messages: &[Message]) -> Result<(), AtmError> {
            unreachable!("handoff test writer only receives display transitions")
        }

        fn load_message(&self, _key: &MessageKey) -> Result<Option<Message>, AtmError> {
            unreachable!("handoff test writer only receives display transitions")
        }

        fn list_messages(&self, _query: &MessageQuery) -> Result<Vec<Message>, AtmError> {
            unreachable!("handoff test writer only receives display transitions")
        }

        fn delete_message(&self, _key: &MessageKey) -> Result<(), AtmError> {
            unreachable!("handoff test writer only receives display transitions")
        }
    }

    #[async_trait::async_trait]
    impl AsyncMessageStore for RecordingWriter {
        async fn apply_read_display_state_async(
            &self,
            _scope: MailboxScope,
            message_ids: Vec<MessageKey>,
            _seen_watermark: Option<IsoTimestamp>,
        ) -> Result<(), AtmError> {
            self.attempts
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            if self
                .failures_remaining
                .fetch_update(
                    std::sync::atomic::Ordering::AcqRel,
                    std::sync::atomic::Ordering::Acquire,
                    |remaining| remaining.checked_sub(1),
                )
                .is_ok()
            {
                return Err(AtmError::daemon_unavailable("transient writer failure"));
            }
            self.applied_transitions
                .lock()
                .expect("recording writer mutex")
                .push(message_ids);
            Ok(())
        }
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
