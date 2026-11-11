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
use atm_core::send::MessageWriteOrigin;
#[cfg(test)]
use atm_storage::MessageQuery;
use atm_storage::{
    AsyncMailboxReader, AsyncMessageStore, AtmError, IsoTimestamp, MailboxListQuery, MailboxScope,
    Message, MessageKey, ReadDeadline, SearchCountGroupBy, SearchFilters, SearchMailboxSelection,
    TimeRange,
};

/// Bounded, composition-owned handoff settings for post-read state updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandoffConfig {
    pub handoff_buffer: usize,
    pub handoff_retry_deadline: Duration,
}

impl Default for HandoffConfig {
    fn default() -> Self {
        Self {
            handoff_buffer: 64,
            handoff_retry_deadline: minimum_retry_deadline(),
        }
    }
}

fn minimum_retry_deadline() -> Duration {
    atm_storage::SQLITE_WRITER_REPLY_TIMEOUT.saturating_add(Duration::from_secs(1))
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
    origin: MessageWriteOrigin,
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
    completed_writes: std::sync::atomic::AtomicU64,
    active_worker: Mutex<Option<tokio::task::AbortHandle>>,
}

/// Owns the manager handle outside the state shared with the task itself.
///
/// Keeping this handle in `StateHandoffInner` would form an `Arc` cycle:
/// manager task -> inner -> its own JoinHandle. This separate owner signals
/// the task to stop when the final runtime clone goes away, releasing the
/// reader/writer boundary promptly in test and production shutdown paths.
struct ManagerTask {
    handle: tokio::task::JoinHandle<()>,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl Drop for ManagerTask {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
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
        let config = HandoffConfig {
            handoff_retry_deadline: config.handoff_retry_deadline.max(minimum_retry_deadline()),
            ..config
        };
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
            completed_writes: std::sync::atomic::AtomicU64::new(0),
            active_worker: Mutex::new(None),
        });
        let manager_inner = Arc::clone(&inner);
        let (shutdown, shutdown_rx) = tokio::sync::watch::channel(false);
        let manager = Arc::new(ManagerTask {
            handle: tokio::spawn(async move {
                supervise_handoff_worker(manager_inner, shutdown_rx).await
            }),
            shutdown,
        });
        Ok(Self { inner, manager })
    }

    pub fn try_push(
        &self,
        scope: MailboxScope,
        message_ids: Vec<MessageKey>,
        seen_watermark: Option<IsoTimestamp>,
    ) -> Result<(), HandoffRejected> {
        self.try_push_with_origin(
            scope,
            message_ids,
            seen_watermark,
            MessageWriteOrigin::Local,
        )
    }

    pub fn try_push_with_origin(
        &self,
        scope: MailboxScope,
        message_ids: Vec<MessageKey>,
        seen_watermark: Option<IsoTimestamp>,
        origin: MessageWriteOrigin,
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
            origin,
        });
        drop(queue);
        self.inner.queue_changed.notify_one();
        Ok(())
    }

    #[must_use]
    pub fn state(&self) -> SupervisorState {
        match self.inner.state.load(std::sync::atomic::Ordering::Acquire) {
            STATE_READY => SupervisorState::Ready,
            _ => SupervisorState::Restarting,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestartCause {
    WriterError,
    WorkerExited,
    WorkerPanic,
    WorkerCancelled,
}

impl RestartCause {
    const fn as_str(self) -> &'static str {
        match self {
            Self::WriterError => "writer_error",
            Self::WorkerExited => "worker_exited",
            Self::WorkerPanic => "worker_panic",
            Self::WorkerCancelled => "worker_cancelled",
        }
    }
}

#[sc_lint_attributes::sc_lint(allow(SCB-RUNTIME-005), reason = "bounded retry delay between attempts; cancelled by worker or daemon shutdown")]
async fn wait_backoff(delay: Duration) {
    tokio::time::sleep(delay).await;
}

async fn supervise_handoff_worker(
    inner: Arc<StateHandoffInner>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    // Dropping the manager must also abort its spawned worker; dropping a
    // JoinHandle alone would detach it and retain the storage boundary.
    struct AbortWorker(tokio::task::AbortHandle);
    impl Drop for AbortWorker {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let mut backoff = Duration::from_millis(50);
    loop {
        let completed_before = inner
            .completed_writes
            .load(std::sync::atomic::Ordering::Acquire);
        let worker_inner = Arc::clone(&inner);
        let worker = tokio::spawn(async move { run_handoff_worker(worker_inner).await });
        let guard = AbortWorker(worker.abort_handle());
        *inner
            .active_worker
            .lock()
            .expect("state-handoff worker mutex poisoned") = Some(worker.abort_handle());
        let result = tokio::select! {
            result = worker => result,
            _ = shutdown.changed() => return,
        };
        *inner
            .active_worker
            .lock()
            .expect("state-handoff worker mutex poisoned") = None;
        drop(guard);
        let cause = match result {
            Ok(Err(error)) => {
                tracing::error!(
                    code = "ATM_MAILBOX_HANDOFF_FAILED",
                    failure_class = error.code().as_str(),
                    "read-state worker failed"
                );
                RestartCause::WriterError
            }
            Ok(Ok(())) => RestartCause::WorkerExited,
            Err(error) if error.is_panic() => RestartCause::WorkerPanic,
            Err(_) => RestartCause::WorkerCancelled,
        };
        if inner
            .completed_writes
            .load(std::sync::atomic::Ordering::Acquire)
            != completed_before
        {
            backoff = Duration::from_millis(50);
        }
        let restart = inner
            .restart_count
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
            .saturating_add(1);
        inner
            .state
            .store(STATE_RESTARTING, std::sync::atomic::Ordering::Release);
        tracing::warn!(
            code = "ATM_MAILBOX_HANDOFF_RESTART",
            failure_class = cause.as_str(),
            attempt = restart,
            "restarting read-state worker"
        );
        tokio::select! {
            _ = wait_backoff(backoff) => {},
            _ = shutdown.changed() => return,
        }
        backoff = backoff.saturating_mul(2).min(Duration::from_secs(5));
        inner
            .state
            .store(STATE_READY, std::sync::atomic::Ordering::Release);
    }
}

async fn next_handoff_transition(inner: &StateHandoffInner) -> ReadDisplayTransition {
    loop {
        if let Some(transition) = inner
            .queue
            .lock()
            .expect("state-handoff queue mutex poisoned")
            .front()
            .cloned()
        {
            return transition;
        }
        inner.queue_changed.notified().await;
    }
}

fn remove_handoff_transition(inner: &StateHandoffInner) {
    let removed = inner
        .queue
        .lock()
        .expect("state-handoff queue mutex poisoned")
        .pop_front();
    debug_assert!(removed.is_some(), "worker owns the front transition");
}

async fn run_handoff_worker(inner: Arc<StateHandoffInner>) -> Result<(), AtmError> {
    loop {
        let transition = next_handoff_transition(&inner).await;
        let retry_deadline = tokio::time::Instant::now() + inner.config.handoff_retry_deadline;
        let mut delay = Duration::from_millis(50);
        let mut last_error = None;
        loop {
            if tokio::time::Instant::now() >= retry_deadline {
                inner
                    .retry_deadline_exhaustions
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return Err(last_error.take().unwrap_or_else(|| {
                    AtmError::daemon_unavailable(
                        "read-state retry deadline expired before writer admission",
                    )
                }));
            }
            match inner
                .writer
                .apply_read_display_state_with_origin_async(
                    transition.scope.clone(),
                    transition.message_ids.clone(),
                    transition.seen_watermark,
                    transition.origin,
                )
                .await
            {
                Ok(()) => {
                    remove_handoff_transition(&inner);
                    inner
                        .completed_writes
                        .fetch_add(1, std::sync::atomic::Ordering::Release);
                    break;
                }
                Err(error)
                    if error.code() == atm_storage::AtmErrorCode::MessageValidationFailed =>
                {
                    tracing::error!(
                        code = "ATM_MAILBOX_HANDOFF_DISCARDED",
                        failure_class = error.code().as_str(),
                        "discarding permanently invalid read-state transition"
                    );
                    remove_handoff_transition(&inner);
                    break;
                }
                Err(error) => {
                    let remaining =
                        retry_deadline.saturating_duration_since(tokio::time::Instant::now());
                    if remaining.is_zero() {
                        inner
                            .retry_deadline_exhaustions
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        return Err(error);
                    }
                    tracing::warn!(
                        code = "ATM_MAILBOX_HANDOFF_RETRY",
                        failure_class = error.code().as_str(),
                        "retrying mailbox read-state handoff"
                    );
                    last_error = Some(error);
                    wait_backoff(delay.min(remaining)).await;
                    delay = delay.saturating_mul(2).min(Duration::from_secs(5));
                }
            }
        }
    }
}

const STATE_READY: u8 = 0;
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
            .select_all_for_tool(
                command.scope().clone(),
                command.selection().clone(),
                command.pushdown_limit(),
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
        let (selection, match_count) = self
            .select_for_read(
                command.scope().clone(),
                command.selection().clone(),
                deadline,
                command.wait_timeout(),
            )
            .await?;
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
        let (selection, match_count) = self
            .select_for_read(
                command.scope().clone(),
                command.selection().clone(),
                deadline,
                command.wait_timeout(),
            )
            .await?;
        let mut selection = selection;
        sort_and_limit_mailbox_selection(&mut selection.selected, Some(1));
        let accepted = self.try_handoff_selected(
            command.scope().clone(),
            &selection,
            command.updates_seen_state(),
            command.write_origin(),
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
        self.read_mail_for_local_ingress(scope, key, request, deadline)
            .await
    }
}

fn bucket_counts_from_storage(
    counts: Vec<atm_storage::SearchCount>,
) -> atm_core::read::BucketCounts {
    let mut buckets = atm_core::read::BucketCounts {
        unread: 0,
        pending_ack: 0,
        history: 0,
    };
    for count in counts {
        match count.key {
            Some(atm_storage::SearchCountKey::Bucket(atm_storage::MailboxBucket::Unread)) => {
                buckets.unread = count.count
            }
            Some(atm_storage::SearchCountKey::Bucket(atm_storage::MailboxBucket::PendingAck)) => {
                buckets.pending_ack = count.count
            }
            Some(atm_storage::SearchCountKey::Bucket(atm_storage::MailboxBucket::History)) => {
                buckets.history = count.count
            }
            None => {}
            _ => {}
        }
    }
    buckets
}

fn mailbox_filters(scope: &MailboxScope, request: &MailboxSelectionRequest) -> SearchFilters {
    SearchFilters {
        team: Some(scope.team.clone()),
        agent: Some(scope.agent.clone()),
        from_agent: request.sender_filter.clone(),
        message_id: request.message_id_filter,
        task_id: request.task_filter.clone(),
        contains: request.contains_filter.clone(),
        time_range: request.timestamp_filter.map(|since| TimeRange {
            since: Some(since),
            until: None,
        }),
        current_only: true,
        mailbox_selection: Some(match request.selection_mode {
            atm_core::types::ReadSelection::Actionable => SearchMailboxSelection::Actionable,
            atm_core::types::ReadSelection::Unread => SearchMailboxSelection::Unread,
            atm_core::types::ReadSelection::PendingAck => SearchMailboxSelection::PendingAck,
            atm_core::types::ReadSelection::All => SearchMailboxSelection::All,
        }),
        ..SearchFilters::default()
    }
}

impl StorageAsyncMailboxRuntime {
    async fn read_mail_for_local_ingress(
        &self,
        scope: MailboxScope,
        key: MessageKey,
        request: MailboxSelectionRequest,
        deadline: RequestDeadline,
    ) -> Result<MailboxSelectionResult, AtmError> {
        let selection = self
            .select_single(scope.clone(), key, request, read_deadline(deadline)?)
            .await?;
        self.try_handoff_selected(scope, &selection, true, MessageWriteOrigin::Local)?;
        Ok(selection)
    }

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
        self.select_matching(scope, request, None, deadline).await
    }

    /// An explicit `atm list` is exploratory/operator traffic. Keep its
    /// ordinary mailbox policy but reserve the agent-class reader capacity for
    /// ack/send/read work under saturation.
    async fn select_all_for_tool(
        &self,
        scope: MailboxScope,
        request: MailboxSelectionRequest,
        limit: Option<usize>,
        deadline: RequestDeadline,
    ) -> Result<MailboxSelectionResult, AtmError> {
        self.select_matching(scope, request, limit, deadline).await
    }

    async fn select_for_read(
        &self,
        scope: MailboxScope,
        request: MailboxSelectionRequest,
        deadline: RequestDeadline,
        wait_timeout: Option<Duration>,
    ) -> Result<(MailboxSelectionResult, usize), AtmError> {
        let Some(wait_timeout) = wait_timeout else {
            return self.select_one_for_read(scope, request, deadline).await;
        };
        let wait_deadline = tokio::time::Instant::now() + wait_timeout;
        loop {
            let (selection, match_count) = self
                .select_one_for_read(scope.clone(), request.clone(), deadline)
                .await?;
            if !selection.selected.is_empty() || tokio::time::Instant::now() >= wait_deadline {
                return Ok((selection, match_count));
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
                return Ok((selection, match_count));
            }
            tokio::time::sleep(pause).await;
        }
    }

    async fn select_one_for_read(
        &self,
        scope: MailboxScope,
        request: MailboxSelectionRequest,
        deadline: RequestDeadline,
    ) -> Result<(MailboxSelectionResult, usize), AtmError> {
        let filters = mailbox_filters(&scope, &request);
        let selection = self
            .select_matching(scope.clone(), request, Some(1), deadline)
            .await?;
        let counts = self
            .reader
            .count_messages(scope, filters, None, read_deadline(deadline)?)
            .await
            .map_err(AtmError::from)?;
        let match_count = counts.first().map_or(0, |count| count.count);
        Ok((selection, match_count))
    }

    async fn select_matching(
        &self,
        scope: MailboxScope,
        request: MailboxSelectionRequest,
        limit: Option<usize>,
        deadline: RequestDeadline,
    ) -> Result<MailboxSelectionResult, AtmError> {
        let filters = mailbox_filters(&scope, &request);
        let bucket_counts = self
            .reader
            .count_messages(
                scope.clone(),
                SearchFilters {
                    team: Some(scope.team.clone()),
                    agent: Some(scope.agent.clone()),
                    current_only: true,
                    ..SearchFilters::default()
                },
                Some(SearchCountGroupBy::Bucket),
                read_deadline(deadline)?,
            )
            .await
            .map_err(AtmError::from)?;
        let messages = self
            .reader
            .list_matching_messages(
                scope,
                MailboxListQuery { filters, limit },
                read_deadline(deadline)?,
            )
            .await
            .map_err(AtmError::from)?;
        let mut selection = select_mailbox_candidates(
            messages.into_iter().map(selection_candidate).collect(),
            &request,
        );
        selection.bucket_counts = bucket_counts_from_storage(bucket_counts);
        Ok(selection)
    }

    fn try_handoff_selected(
        &self,
        scope: MailboxScope,
        selection: &MailboxSelectionResult,
        update_seen_watermark: bool,
        origin: MessageWriteOrigin,
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
        if let Err(rejected) =
            handoff.try_push_with_origin(scope, message_ids, seen_watermark, origin)
        {
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

fn selection_candidate(message: Message) -> MailboxSelectionCandidate {
    MailboxSelectionCandidate {
        message_key: message.message_key.to_string(),
        envelope: message.envelope,
    }
}

pub fn read_deadline(deadline: RequestDeadline) -> Result<ReadDeadline, AtmError> {
    deadline
        .remaining()
        .ok_or_else(|| AtmError::daemon_unavailable("mailbox request deadline expired"))
        .and_then(ReadDeadline::new)
}

#[cfg(test)]
mod tests {
    use atm_runtime_test_support::RecordingWriter;
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
        AgentName, AtmError, AtmErrorCode, AtmMessageId, IsoTimestamp, Message, MessageEnvelope,
        MessageKey, ReadLaneError, TeamName,
    };

    use super::{
        AsyncMailboxRuntime, AsyncMessageStore, HandoffConfig, HandoffRejected, MessageWriteOrigin,
        StateHandoffSupervisor, StorageAsyncMailboxRuntime,
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
                placement: None,
                task_op: None,
                task_complete: None,
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

    struct OriginCaptureWriter {
        sender: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<MessageWriteOrigin>>>,
    }

    impl atm_storage::contract::sealed::Sealed for OriginCaptureWriter {}

    impl atm_storage::MessageStore for OriginCaptureWriter {
        fn save_message(&self, _message: &Message) -> Result<(), AtmError> {
            unreachable!("origin capture only handles display-state writes")
        }

        fn admit_message_with_provenance(
            &self,
            _message: &Message,
            _provenance: MessageWriteOrigin,
        ) -> Result<atm_storage::CommittedTaskWrite<atm_storage::MessageAdmissionOutcome>, AtmError>
        {
            unreachable!("origin capture only handles display-state writes")
        }

        fn save_messages_atomically(&self, _messages: &[Message]) -> Result<(), AtmError> {
            unreachable!("origin capture only handles display-state writes")
        }

        fn load_message(&self, _key: &MessageKey) -> Result<Option<Message>, AtmError> {
            unreachable!("origin capture only handles display-state writes")
        }

        fn list_messages(
            &self,
            _query: &atm_storage::MessageQuery,
        ) -> Result<Vec<Message>, AtmError> {
            unreachable!("origin capture only handles display-state writes")
        }

        fn delete_message(&self, _key: &MessageKey) -> Result<(), AtmError> {
            unreachable!("origin capture only handles display-state writes")
        }
    }

    #[async_trait::async_trait]
    impl AsyncMessageStore for OriginCaptureWriter {
        async fn apply_read_display_state_async(
            &self,
            _scope: atm_storage::MailboxScope,
            _message_ids: Vec<MessageKey>,
            _seen_watermark: Option<IsoTimestamp>,
        ) -> Result<(), AtmError> {
            unreachable!("cron handoff must use origin-aware writer admission")
        }

        async fn apply_read_display_state_with_origin_async(
            &self,
            _scope: atm_storage::MailboxScope,
            _message_ids: Vec<MessageKey>,
            _seen_watermark: Option<IsoTimestamp>,
            origin: MessageWriteOrigin,
        ) -> Result<(), AtmError> {
            if let Some(sender) = self.sender.lock().expect("origin sender mutex").take() {
                let _ = sender.send(origin);
            }
            Ok(())
        }

        async fn admit_message_with_provenance_async(
            &self,
            _message: Message,
            _provenance: MessageWriteOrigin,
        ) -> Result<atm_storage::CommittedTaskWrite<atm_storage::MessageAdmissionOutcome>, AtmError>
        {
            unreachable!("origin capture only handles display-state writes")
        }
    }

    #[tokio::test]
    async fn cron_origin_survives_read_handoff_until_writer_admission() {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let writer = Arc::new(OriginCaptureWriter {
            sender: std::sync::Mutex::new(Some(sender)),
        });
        let supervisor = StateHandoffSupervisor::start(HandoffConfig::default(), writer)
            .expect("supervisor starts");
        let origin = MessageWriteOrigin::CronRun {
            schedule_id: atm_storage::CronScheduleId::new(17),
            generation: atm_storage::CronGeneration::FIRST,
        };

        supervisor
            .try_push_with_origin(
                scope(),
                vec!["delayed-read".parse().expect("message key")],
                None,
                origin,
            )
            .expect("read transition enters the handoff queue");

        assert_eq!(
            receiver.await.expect("writer receives queued transition"),
            origin
        );
    }

    #[tokio::test]
    async fn handoff_returns_without_waiting_for_writer_execution() {
        let writer = Arc::new(RecordingWriter::default());
        let supervisor = StateHandoffSupervisor::start(
            HandoffConfig {
                handoff_buffer: 2,
                handoff_retry_deadline: Duration::from_secs(1),
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
        assert!(
            !outcome.message.expect("selected message").envelope.read,
            "the response remains the pre-handoff reader snapshot"
        );

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
    async fn exact_message_read_reports_handoff_acceptance_not_durability() {
        let writer = Arc::new(RecordingWriter::default());
        let mut selected = message("unread", false);
        let message_id = AtmMessageId::new();
        selected.envelope.message_id = Some(message_id);
        let runtime = StorageAsyncMailboxRuntime::new(
            Arc::new(InMemoryMailboxReader::with_messages(vec![selected])),
            writer,
        )
        .with_state_handoff(HandoffConfig::default())
        .expect("runtime handoff starts");
        let root = PathBuf::from("/daemon-owned");
        let id = message_id.to_string();
        let read = ReadQuery::new(
            root.clone(),
            root,
            agent(),
            None,
            team(),
            ReadSelection::Unread,
            false,
            true,
            Some(&id),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("exact read query");
        let outcome = runtime
            .read_command(
                prepare_async_read(&read).expect("core prepares exact policy"),
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .await
            .expect("runtime selects exact message");
        assert!(
            outcome.mutation_applied,
            "acceptance is reported for exact reads"
        );
        assert!(
            !outcome.message.expect("selected message").envelope.read,
            "exact reads share the pre-handoff snapshot contract"
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

    type ControlledAttempt = (
        Vec<MessageKey>,
        tokio::sync::oneshot::Sender<Result<(), AtmError>>,
    );
    struct ControlledWriter {
        attempts: tokio::sync::mpsc::UnboundedSender<ControlledAttempt>,
        applied: Arc<std::sync::Mutex<Vec<Vec<MessageKey>>>>,
    }

    async fn next_attempt(
        attempts: &mut tokio::sync::mpsc::UnboundedReceiver<ControlledAttempt>,
    ) -> ControlledAttempt {
        tokio::time::timeout(Duration::from_secs(30), attempts.recv())
            .await
            .expect("attempt deadline")
            .expect("writer still present")
    }

    impl atm_storage::contract::sealed::Sealed for ControlledWriter {}

    impl atm_storage::MessageStore for ControlledWriter {
        fn save_message(&self, _: &Message) -> Result<(), AtmError> {
            unreachable!()
        }
        fn admit_message_with_provenance(
            &self,
            _: &Message,
            _: MessageWriteOrigin,
        ) -> Result<atm_storage::CommittedTaskWrite<atm_storage::MessageAdmissionOutcome>, AtmError>
        {
            unreachable!()
        }
        fn save_messages_atomically(&self, _: &[Message]) -> Result<(), AtmError> {
            unreachable!()
        }
        fn load_message(&self, _: &MessageKey) -> Result<Option<Message>, AtmError> {
            unreachable!()
        }
        fn list_messages(&self, _: &atm_storage::MessageQuery) -> Result<Vec<Message>, AtmError> {
            unreachable!()
        }
        fn delete_message(&self, _: &MessageKey) -> Result<(), AtmError> {
            unreachable!()
        }
    }

    #[async_trait::async_trait]
    impl AsyncMessageStore for ControlledWriter {
        async fn apply_read_display_state_async(
            &self,
            _: atm_storage::MailboxScope,
            message_ids: Vec<MessageKey>,
            _: Option<IsoTimestamp>,
        ) -> Result<(), AtmError> {
            self.applied
                .lock()
                .expect("applied lock")
                .push(message_ids.clone());
            let (tx, rx) = tokio::sync::oneshot::channel();
            self.attempts
                .send((message_ids, tx))
                .expect("test receives attempt");
            rx.await.expect("test releases writer")
        }
        async fn admit_message_with_provenance_async(
            &self,
            _: Message,
            _: MessageWriteOrigin,
        ) -> Result<atm_storage::CommittedTaskWrite<atm_storage::MessageAdmissionOutcome>, AtmError>
        {
            unreachable!()
        }
    }

    #[tokio::test(start_paused = true)]
    async fn handoff_recovers_after_writer_failure_without_discarding_queue() {
        let (tx, mut attempts) = tokio::sync::mpsc::unbounded_channel();
        let config = HandoffConfig::default();
        assert!(config.handoff_retry_deadline > atm_storage::SQLITE_WRITER_REPLY_TIMEOUT);
        let supervisor = StateHandoffSupervisor::start(
            config,
            Arc::new(ControlledWriter {
                attempts: tx,
                applied: Default::default(),
            }),
        )
        .expect("supervisor starts");
        supervisor
            .try_push(scope(), vec!["unread".parse().expect("key")], None)
            .expect("queued");
        let (_, first) = next_attempt(&mut attempts).await;
        tokio::time::advance(config.handoff_retry_deadline).await;
        first
            .send(Err(AtmError::daemon_unavailable("controlled failure")))
            .expect("worker waiting");
        let (_, second) = next_attempt(&mut attempts).await;
        assert_eq!(supervisor.restart_count(), 1);
        assert_eq!(supervisor.buffered_depth(), 1);
        second.send(Ok(())).expect("worker waiting");
        // A subsequent queued request is an acknowledgement that the first
        // completed and was removed; no scheduling assumptions are needed.
        supervisor
            .try_push(scope(), vec!["next".parse().expect("key")], None)
            .expect("queued next");
        let (_, third) = next_attempt(&mut attempts).await;
        assert_eq!(supervisor.buffered_depth(), 1);
        third.send(Ok(())).expect("worker waiting");
    }

    #[tokio::test(start_paused = true)]
    async fn handoff_restarts_after_a_forced_worker_exit_and_drains_the_retained_transition() {
        let (tx, mut attempts) = tokio::sync::mpsc::unbounded_channel();
        let supervisor = StateHandoffSupervisor::start(
            HandoffConfig::default(),
            Arc::new(ControlledWriter {
                attempts: tx,
                applied: Default::default(),
            }),
        )
        .expect("supervisor starts");
        supervisor
            .try_push(scope(), vec!["retry".parse().expect("key")], None)
            .expect("queued");
        let (_, _blocked_reply) = next_attempt(&mut attempts).await;
        assert!(supervisor.abort_active_worker_for_test());
        let (_, retry) = next_attempt(&mut attempts).await;
        assert_eq!(supervisor.restart_count(), 1);
        assert_eq!(supervisor.buffered_depth(), 1);
        retry.send(Ok(())).expect("worker waiting");
        supervisor
            .try_push(scope(), vec!["next".parse().expect("key")], None)
            .expect("queued next");
        let (_, _next) = next_attempt(&mut attempts).await;
        assert_eq!(supervisor.buffered_depth(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn handoff_recovers_after_worker_panic() {
        let (tx, mut attempts) = tokio::sync::mpsc::unbounded_channel();
        let supervisor = StateHandoffSupervisor::start(
            HandoffConfig::default(),
            Arc::new(ControlledWriter {
                attempts: tx,
                applied: Default::default(),
            }),
        )
        .expect("supervisor starts");
        supervisor
            .try_push(scope(), vec!["retry".parse().expect("key")], None)
            .expect("queued");
        let (_, first) = next_attempt(&mut attempts).await;
        drop(first); // The test writer panics when its release channel closes.
        let (_, retry) = next_attempt(&mut attempts).await;
        assert_eq!(supervisor.restart_count(), 1);
        assert_eq!(supervisor.buffered_depth(), 1);
        retry.send(Ok(())).expect("worker waiting");
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_supervisor_aborts_a_blocked_worker() {
        let (tx, mut attempts) = tokio::sync::mpsc::unbounded_channel();
        let supervisor = StateHandoffSupervisor::start(
            HandoffConfig::default(),
            Arc::new(ControlledWriter {
                attempts: tx,
                applied: Default::default(),
            }),
        )
        .expect("supervisor starts");
        supervisor
            .try_push(scope(), vec!["blocked".parse().expect("key")], None)
            .expect("queued");
        let (_, mut reply) = next_attempt(&mut attempts).await;
        drop(supervisor);
        tokio::time::timeout(Duration::from_secs(30), reply.closed())
            .await
            .expect("worker shutdown deadline");
    }

    #[tokio::test(start_paused = true)]
    async fn permanent_head_failure_is_discarded_and_next_transition_runs() {
        let (tx, mut attempts) = tokio::sync::mpsc::unbounded_channel();
        let supervisor = StateHandoffSupervisor::start(
            HandoffConfig::default(),
            Arc::new(ControlledWriter {
                attempts: tx,
                applied: Default::default(),
            }),
        )
        .expect("supervisor starts");
        supervisor
            .try_push(scope(), vec!["deleted".parse().expect("key")], None)
            .expect("first queued");
        supervisor
            .try_push(scope(), vec!["next".parse().expect("key")], None)
            .expect("next queued");
        let (first_keys, first) = next_attempt(&mut attempts).await;
        assert_eq!(first_keys[0].as_str(), "deleted");
        first
            .send(Err(AtmError::validation("message was deleted")))
            .expect("worker waiting");
        let (second_keys, second) = next_attempt(&mut attempts).await;
        assert_eq!(second_keys[0].as_str(), "next");
        assert_eq!(supervisor.buffered_depth(), 1);
        second.send(Ok(())).expect("worker waiting");
        supervisor
            .try_push(scope(), vec!["barrier".parse().expect("key")], None)
            .expect("barrier queued");
        let (_, barrier) = next_attempt(&mut attempts).await;
        assert_eq!(
            supervisor.buffered_depth(),
            1,
            "failed and successful transitions left the queue"
        );
        barrier.send(Ok(())).expect("worker waiting");
    }

    #[tokio::test(start_paused = true)]
    async fn committed_timeout_retries_once_in_order_then_later_read_applies() {
        let (tx, mut attempts) = tokio::sync::mpsc::unbounded_channel();
        let applied = Arc::new(std::sync::Mutex::new(Vec::new()));
        let writer = Arc::new(ControlledWriter {
            attempts: tx,
            applied: applied.clone(),
        });
        let supervisor = StateHandoffSupervisor::start(HandoffConfig::default(), writer)
            .expect("supervisor starts");
        supervisor
            .try_push(scope(), vec!["first".parse().expect("key")], None)
            .expect("first queued");
        supervisor
            .try_push(scope(), vec!["later".parse().expect("key")], None)
            .expect("later queued");
        let (_, first) = next_attempt(&mut attempts).await;
        // ControlledWriter records the re-arm before allowing the reply to be released.
        first
            .send(Err(AtmError::daemon_unavailable(
                "writer reply timed out after commit",
            )))
            .expect("worker waiting");
        let (retry_keys, retry) = next_attempt(&mut attempts).await;
        assert_eq!(retry_keys[0].as_str(), "first");
        retry.send(Ok(())).expect("worker waiting");
        let (later_keys, later) = next_attempt(&mut attempts).await;
        assert_eq!(later_keys[0].as_str(), "later");
        let rearmed = applied.lock().expect("applied lock").clone();
        assert_eq!(
            rearmed
                .iter()
                .map(|keys| keys[0].as_str())
                .collect::<Vec<_>>(),
            ["first", "first", "later"]
        );
        later.send(Ok(())).expect("worker waiting");
    }

    #[test]
    fn restart_cause_has_stable_diagnostic_names() {
        use super::RestartCause;
        assert_eq!(RestartCause::WriterError.as_str(), "writer_error");
        assert_eq!(RestartCause::WorkerExited.as_str(), "worker_exited");
        assert_eq!(RestartCause::WorkerPanic.as_str(), "worker_panic");
        assert_eq!(RestartCause::WorkerCancelled.as_str(), "worker_cancelled");
    }

    #[tokio::test(start_paused = true)]
    async fn restart_backoff_resets_after_successful_transition() {
        let (tx, mut attempts) = tokio::sync::mpsc::unbounded_channel();
        let supervisor = StateHandoffSupervisor::start(
            HandoffConfig::default(),
            Arc::new(ControlledWriter {
                attempts: tx,
                applied: Default::default(),
            }),
        )
        .expect("supervisor starts");
        supervisor
            .try_push(scope(), vec!["first".parse().expect("key")], None)
            .expect("queued");
        let (_, first) = next_attempt(&mut attempts).await;
        drop(first);
        let start = tokio::time::Instant::now();
        let (_, second) = next_attempt(&mut attempts).await;
        assert_eq!(
            tokio::time::Instant::now() - start,
            Duration::from_millis(50)
        );
        drop(second);
        let start = tokio::time::Instant::now();
        let (_, third) = next_attempt(&mut attempts).await;
        assert_eq!(
            tokio::time::Instant::now() - start,
            Duration::from_millis(100)
        );
        third.send(Ok(())).expect("worker waiting");
        supervisor
            .try_push(scope(), vec!["next".parse().expect("key")], None)
            .expect("queued");
        let (_, next) = next_attempt(&mut attempts).await;
        drop(next);
        let start = tokio::time::Instant::now();
        let (_, retry) = next_attempt(&mut attempts).await;
        assert_eq!(
            tokio::time::Instant::now() - start,
            Duration::from_millis(50)
        );
        retry.send(Ok(())).expect("worker waiting");
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
                ReadLaneError::DeadlineExpired {
                    stage: "test",
                    budget_ms: 3_000,
                    elapsed_ms: 3_000,
                },
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

    #[test]
    fn reader_deadline_error_exposes_safe_variant_stage_and_budget() {
        let error = AtmError::from(ReadLaneError::DeadlineExpired {
            stage: "running sqlite query",
            budget_ms: 3_000,
            elapsed_ms: 2_997,
        });

        assert_eq!(error.code(), AtmErrorCode::MailboxLockTimeout);
        assert!(error.detail().contains("variant=deadline_expired"));
        assert!(error.detail().contains("stage=running sqlite query"));
        assert!(error.detail().contains("budget_ms=3000"));
        assert!(error.detail().contains("elapsed_ms=2997"));
    }

    #[test]
    fn reader_storage_error_keeps_diagnostic_cause_out_of_public_detail() {
        let error = AtmError::from(ReadLaneError::Storage {
            code: AtmErrorCode::MailboxReadFailed,
            message: "sqlite reader failed at /private/atm.db".to_owned(),
            cause: Some("/private/atm.db".to_owned()),
        });

        assert!(!error.detail().contains("/private/atm.db"));
        assert!(error.cause().is_some());
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

    fn admit_message_with_provenance(
        &self,
        _message: &Message,
        _provenance: atm_storage::MessageWriteOrigin,
    ) -> Result<atm_storage::CommittedTaskWrite<atm_storage::MessageAdmissionOutcome>, AtmError>
    {
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
#[async_trait::async_trait]
impl AsyncMessageStore for TestOnlyWriterLane {
    async fn admit_message_with_provenance_async(
        &self,
        _message: Message,
        _provenance: atm_storage::MessageWriteOrigin,
    ) -> Result<atm_storage::CommittedTaskWrite<atm_storage::MessageAdmissionOutcome>, AtmError>
    {
        unreachable!("reader-port test double must not use the writer lane")
    }
}
