//! Shared router-local async support primitives.

use std::future::Future;

use std::num::NonZeroUsize;

use std::sync::Arc;

use atm_core::LocalServiceRuntime;
use atm_core::api::RequestDeadline;
use atm_core::send::{
    SendMessageSource, WriteRequest, WriteSourcePreflight, preflight_write_source_request,
};

use atm_core::error::AtmError;
use atm_core::request_budget::RESPONSE_HANDOFF_GRACE;

use atm_core::protocol::{RequestId, ResponseEnvelope, SendResponseEnvelope};

use atm_core::send::{WarningEntry, WriteOutcome};
use atm_core::types::{AgentName, TeamName};

use crate::RuntimeHealth;

pub(crate) fn retry_deferred_marker<F>(health: &RuntimeHealth, mut mark: F) -> Result<(), AtmError>
where
    F: FnMut() -> Result<(), AtmError>,
{
    match mark() {
        Ok(()) => Ok(()),
        Err(error) => {
            health.record_queue_marker_set_failure();
            tracing::warn!(
                subsystem = "atm_core.queue",
                action = "queue_marker_set",
                outcome = "failed",
                %error,
                "retrying deferred write queue marker"
            );
            match mark() {
                Ok(()) => Ok(()),
                Err(retry_error) => {
                    health.record_queue_marker_set_failure();
                    Err(retry_error)
                }
            }
        }
    }
}

/// Bounded bridge for synchronous core operations that are not storage-writer
/// submissions.
///
/// Durable message admission uses the async storage boundary directly. The
/// deferred queue marker is the one post-admission exception: its capability
/// is intentionally synchronous, so the marker transaction enters this bridge
/// before the request leaves the router.
#[derive(Clone)]
pub(crate) struct ControlPathSyncBridge {
    bridge: BoundedBlockingBridge,
}

/// The completion behavior for a blocking operation admitted by
/// [`BoundedBlockingBridge`].
#[derive(Clone, Copy)]
enum BlockingCompletionPolicy {
    /// Durable control-path work must finish once it has started.
    AwaitCompletion,
    /// Source preflight may return at the request deadline; its blocking job
    /// keeps its permit until it exits so permanent stalls cannot accumulate.
    AbandonResponseAtDeadline,
}

#[derive(Clone, Copy)]
enum BlockingStallCounter {
    CoreBridge,
    WriteSourcePreflight,
}

#[derive(Clone)]
pub struct BoundedBlockingBridge {
    permits: Arc<tokio::sync::Semaphore>,
    runtime_health: RuntimeHealth,
    #[cfg(test)]
    clock: Option<Arc<dyn Fn() -> std::time::Instant + Send + Sync>>,
    #[cfg(test)]
    admission_count: Arc<std::sync::atomic::AtomicUsize>,
    #[cfg(test)]
    admission_signal: Arc<tokio::sync::Notify>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PromptHandoffFailureReason {
    Storage,
    Timeout,
    Saturated,
}

impl PromptHandoffFailureReason {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Storage => "storage",
            Self::Timeout => "timeout",
            Self::Saturated => "saturated",
        }
    }
}

enum BlockingBridgeError {
    DeadlineBeforeStart,
    DeadlineAfterStart,
    Closed,
    Join(tokio::task::JoinError),
    Operation(AtmError),
}

impl BoundedBlockingBridge {
    #[must_use]
    pub fn new(capacity: NonZeroUsize, runtime_health: RuntimeHealth) -> Self {
        Self {
            permits: Arc::new(tokio::sync::Semaphore::new(capacity.get())),
            runtime_health,
            #[cfg(test)]
            clock: None,
            #[cfg(test)]
            admission_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            #[cfg(test)]
            admission_signal: Arc::new(tokio::sync::Notify::new()),
        }
    }

    fn now(&self) -> std::time::Instant {
        #[cfg(test)]
        if let Some(clock) = &self.clock {
            return clock();
        }
        std::time::Instant::now()
    }

    #[cfg(test)]
    pub(crate) fn available_permits(&self) -> usize {
        self.permits.available_permits()
    }

    #[cfg(test)]
    pub(crate) fn admission_count(&self) -> usize {
        self.admission_count
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    #[cfg(test)]
    pub(crate) async fn wait_for_admissions(&self, count: usize) {
        loop {
            let notified = self.admission_signal.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.admission_count() >= count {
                return;
            }
            notified.await;
        }
    }

    /// Runs synchronous work without allowing either permit admission or the
    /// caller's response wait to outlive the supplied absolute deadline.
    ///
    /// A job which has already started remains alive with its permit held
    /// until it exits. This bounds stalled work without pretending Tokio can
    /// cancel a running blocking closure.
    pub async fn run<T, F>(&self, deadline: RequestDeadline, job: F) -> Result<T, AtmError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, AtmError> + Send + 'static,
    {
        self.run_with_policy(
            deadline,
            BlockingCompletionPolicy::AbandonResponseAtDeadline,
            BlockingStallCounter::CoreBridge,
            job,
        )
        .await
        .map_err(blocking_bridge_error)
    }

    async fn run_with_policy<T, F>(
        &self,
        deadline: RequestDeadline,
        policy: BlockingCompletionPolicy,
        stall_counter: BlockingStallCounter,
        job: F,
    ) -> Result<T, BlockingBridgeError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, AtmError> + Send + 'static,
    {
        let deadline = crate::request_deadline::capture(deadline);
        self.run_with_policy_at(deadline, policy, stall_counter, job)
            .await
    }

    async fn run_with_policy_at<T, F>(
        &self,
        deadline: tokio::time::Instant,
        policy: BlockingCompletionPolicy,
        stall_counter: BlockingStallCounter,
        job: F,
    ) -> Result<T, BlockingBridgeError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, AtmError> + Send + 'static,
    {
        if tokio::time::Instant::now() >= deadline {
            return Err(BlockingBridgeError::DeadlineBeforeStart);
        }
        #[cfg(test)]
        {
            self.admission_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.admission_signal.notify_waiters();
        }
        let permit = tokio::time::timeout_at(deadline, Arc::clone(&self.permits).acquire_owned())
            .await
            .map_err(|_| BlockingBridgeError::DeadlineBeforeStart)?
            .map_err(|_| BlockingBridgeError::Closed)?;
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(BlockingBridgeError::DeadlineBeforeStart);
        }
        let started_at = self.now();
        match policy {
            BlockingCompletionPolicy::AwaitCompletion => {
                let outcome = tokio::task::spawn_blocking(job)
                    .await
                    .map_err(BlockingBridgeError::Join)?;
                let elapsed = self.now().saturating_duration_since(started_at);
                if elapsed > remaining {
                    self.record_stall(stall_counter, elapsed, remaining);
                }
                drop(permit);
                outcome.map_err(BlockingBridgeError::Operation)
            }
            BlockingCompletionPolicy::AbandonResponseAtDeadline => {
                let outcome = tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    job()
                });
                match tokio::time::timeout_at(deadline, outcome).await {
                    Ok(Ok(result)) => result.map_err(BlockingBridgeError::Operation),
                    Ok(Err(source)) => Err(BlockingBridgeError::Join(source)),
                    Err(_) => {
                        self.record_stall(
                            stall_counter,
                            self.now().saturating_duration_since(started_at),
                            remaining,
                        );
                        Err(BlockingBridgeError::DeadlineAfterStart)
                    }
                }
            }
        }
    }

    fn record_stall(
        &self,
        counter: BlockingStallCounter,
        elapsed: std::time::Duration,
        budget: std::time::Duration,
    ) {
        let subsystem = match counter {
            BlockingStallCounter::CoreBridge => {
                self.runtime_health.record_blocking_core_bridge_stall();
                "atm_http_runtime.blocking_core_bridge"
            }
            BlockingStallCounter::WriteSourcePreflight => {
                self.runtime_health.record_write_source_preflight_stall();
                "atm_http_runtime.write_source_preflight"
            }
        };
        tracing::warn!(
            subsystem,
            action = "blocking_job",
            outcome = "budget_exceeded",
            ?elapsed,
            ?budget,
            "bounded blocking job outlived its remaining request budget"
        );
    }
}

fn blocking_bridge_error(error: BlockingBridgeError) -> AtmError {
    match error {
        BlockingBridgeError::DeadlineBeforeStart => AtmError::new(
            atm_core::error::AtmErrorCode::BlockingBridgeDeadlineBeforeStart,
            "blocking work did not start before its request deadline",
        ),
        BlockingBridgeError::DeadlineAfterStart => AtmError::new(
            atm_core::error::AtmErrorCode::BlockingBridgeDeadlineAfterStart,
            "blocking work timed out before its request deadline",
        ),
        BlockingBridgeError::Closed => {
            AtmError::daemon_unavailable("bounded blocking bridge is shutting down")
        }
        BlockingBridgeError::Join(source) => AtmError::new(
            atm_core::error::AtmErrorCode::InternalError,
            "blocking task ended unexpectedly",
        )
        .with_cause(source),
        BlockingBridgeError::Operation(error) => error,
    }
}

/// Bounded blocking admission for caller-owned write source preflight.
///
/// Template verification and file-policy evaluation can re-open caller-owned
/// paths and block on host filesystem policy. They are isolated from Tokio
/// workers and capped independently of reads. A timed-out task retains its
/// permit until it exits, preventing an unbounded blocked-task buildup.
#[derive(Clone)]
pub(crate) struct WriteSourcePreflightBridge {
    bridge: BoundedBlockingBridge,
}

impl WriteSourcePreflightBridge {
    pub(crate) fn new(runtime_health: RuntimeHealth) -> Self {
        Self {
            // Two permanently stalled filesystem requests remain observable
            // and fail closed without consuming Tokio workers or read-lane
            // capacity; a larger pool would only widen that stranded work.
            bridge: BoundedBlockingBridge::new(
                NonZeroUsize::new(2).expect("source preflight capacity is non-zero"),
                runtime_health,
            ),
        }
    }

    pub(crate) async fn preflight(
        &self,
        deadline: RequestDeadline,
        runtime: LocalServiceRuntime,
        request: WriteRequest,
    ) -> Result<WriteSourcePreflight, AtmError> {
        if matches!(request.message_source, SendMessageSource::Inline(_)) {
            return preflight_write_source_request(&runtime, &request);
        };
        let description = source_preflight_description(&request);
        self.bridge
            .run_with_policy(
                deadline,
                BlockingCompletionPolicy::AbandonResponseAtDeadline,
                BlockingStallCounter::WriteSourcePreflight,
                move || preflight_write_source_request(&runtime, &request),
            )
            .await
            .map_err(|error| source_preflight_error(&description, error))
    }
}

fn source_preflight_description(request: &WriteRequest) -> String {
    match &request.message_source {
        SendMessageSource::Template(source) => {
            format!("template {}", source.canonical_template_path.display())
        }
        SendMessageSource::File { path, .. } => format!("file {}", path.display()),
        SendMessageSource::Inline(_) => "inline message".to_owned(),
    }
}

fn source_preflight_error(description: &str, error: BlockingBridgeError) -> AtmError {
    match error {
        BlockingBridgeError::DeadlineBeforeStart => AtmError::daemon_unavailable(format!(
            "write source preflight for {description} did not start before the request deadline"
        )),
        BlockingBridgeError::DeadlineAfterStart => AtmError::daemon_unavailable(format!(
            "write source preflight for {description} did not finish before the request deadline"
        )),
        BlockingBridgeError::Closed => {
            AtmError::daemon_unavailable("write source preflight bridge is shutting down")
        }
        BlockingBridgeError::Join(source) => {
            AtmError::daemon_unavailable("write source preflight task ended unexpectedly")
                .with_cause(source)
        }
        BlockingBridgeError::Operation(error) => error,
    }
}

/// Reserves response encoding and loopback handoff time before a local
/// receiver hook begins.  A hook remains advisory after durable persistence;
/// it must never consume the daemon's final response budget.
pub(super) fn receiver_hook_deadline(
    deadline: RequestDeadline,
) -> Option<(RequestDeadline, tokio::time::Instant)> {
    let hook_at = crate::request_deadline::capture(deadline).checked_sub(RESPONSE_HANDOFF_GRACE)?;
    (hook_at > tokio::time::Instant::now())
        .then(|| (deadline.minus(RESPONSE_HANDOFF_GRACE), hook_at))
}

impl ControlPathSyncBridge {
    pub(crate) fn new(capacity: NonZeroUsize, runtime_health: RuntimeHealth) -> Self {
        Self {
            bridge: BoundedBlockingBridge::new(capacity, runtime_health),
        }
    }

    #[cfg(test)]
    pub(crate) fn set_test_clock(
        &mut self,
        clock: Arc<dyn Fn() -> std::time::Instant + Send + Sync>,
    ) {
        self.bridge.clock = Some(clock);
    }

    pub(crate) fn blocking_bridge(&self) -> &BoundedBlockingBridge {
        &self.bridge
    }

    pub(crate) async fn run<T, F>(&self, deadline: RequestDeadline, job: F) -> Result<T, AtmError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, AtmError> + Send + 'static,
    {
        self.bridge
            .run_with_policy(
                deadline,
                BlockingCompletionPolicy::AwaitCompletion,
                BlockingStallCounter::CoreBridge,
                job,
            )
            .await
            .map_err(|error| match error {
                BlockingBridgeError::DeadlineBeforeStart => AtmError::daemon_unavailable(
                    "request deadline expired before replacement blocking core operation",
                ),
                BlockingBridgeError::DeadlineAfterStart => {
                    unreachable!("durable control work awaits completion")
                }
                BlockingBridgeError::Closed => AtmError::daemon_unavailable(
                    "replacement blocking core bridge is shutting down",
                ),
                BlockingBridgeError::Join(source) => AtmError::new(
                    atm_core::error::AtmErrorCode::InternalError,
                    "replacement storage write task ended unexpectedly",
                )
                .with_cause(source),
                BlockingBridgeError::Operation(error) => error,
            })
    }
}

/// Receiver-hook work that a peer response does not wait for.
///
/// A peer write is acknowledged as soon as the message is durably persisted,
/// so its receiver hook (a tmux nudge, a graft handoff) cannot run on the
/// response path without risking the caller's absolute request budget. The
/// hook is therefore detached from the response but never unobserved: every
/// warning it produces is logged with the originating request id and counted
/// on `RuntimeHealth`, and daemon shutdown drains whatever is still in
/// flight instead of abandoning it mid-emission.
#[derive(Clone, Default)]
pub(crate) struct DetachedReceivedHooks {
    tasks: Arc<std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}

impl DetachedReceivedHooks {
    pub(crate) fn observe<F>(&self, runtime_health: RuntimeHealth, request_id: RequestId, hook: F)
    where
        F: Future<Output = Vec<WarningEntry>> + Send + 'static,
    {
        let deadline = crate::request_deadline::current();
        let task = tokio::spawn(crate::request_deadline::scope_optional(
            deadline,
            async move {
                for warning in hook.await {
                    runtime_health.record_detached_received_hook_warning();
                    tracing::warn!(
                        subsystem = "atm_http_runtime.received_hook",
                        action = "peer_received_hook",
                        outcome = "warning",
                        %request_id,
                        code = ?warning.code,
                        detail = %warning.message,
                        "receiver hook reported a warning after the peer write was durably persisted"
                    );
                }
            },
        ));
        let mut tasks = self.lock();
        tasks.retain(|task| !task.is_finished());
        tasks.push(task);
    }

    /// Awaits every in-flight detached hook, bounded by `deadline`.
    ///
    /// Tasks that outlive the bound stay detached: this drain must not delay
    /// daemon shutdown past its own budget.
    pub(crate) async fn drain(&self, deadline: std::time::Duration) {
        let pending = std::mem::take(&mut *self.lock());
        let _timed_out = tokio::time::timeout(deadline, async {
            for task in pending {
                let _joined = task.await;
            }
        })
        .await;
    }

    /// The registry holds only `JoinHandle`s and nothing under this guard can
    /// panic, so a poisoned lock would mean an unrelated invariant already
    /// broke; surfacing it is correct.
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<tokio::task::JoinHandle<()>>> {
        self.tasks
            .lock()
            .expect("detached received-hook registry is never held across a panic")
    }
}

pub(super) fn append_warnings(outcome: &mut WriteOutcome, warnings: Vec<WarningEntry>) {
    match outcome {
        WriteOutcome::Sent(outcome) => outcome.warnings.extend(warnings),
        WriteOutcome::Acknowledged(outcome) => outcome.warnings.extend(warnings),
    }
}

pub(super) fn write_response(outcome: WriteOutcome) -> ResponseEnvelope {
    match outcome {
        WriteOutcome::Sent(outcome) => ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)),
        WriteOutcome::Acknowledged(outcome) => {
            ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome))
        }
    }
}

pub(super) fn hook_warning(error: AtmError) -> WarningEntry {
    WarningEntry::with_code(
        error.code(),
        format!("message received successfully, but its receiver hook did not run: {error}"),
        Some("inspect the receiver hook endpoint or harness, then continue normally"),
    )
}

pub(super) fn validate_graft_receiver_member(
    runtime: &LocalServiceRuntime,
    team: &TeamName,
    agent: &AgentName,
) -> Result<(), AtmError> {
    if runtime.load_roster_member(team, agent).is_none() {
        return Err(AtmError::agent_not_found(agent.as_str(), team.as_str()));
    }
    Ok(())
}

#[cfg(test)]
mod deadline_tests {
    use super::{
        BlockingBridgeError, BlockingCompletionPolicy, BlockingStallCounter, BoundedBlockingBridge,
    };
    use crate::RuntimeHealth;
    use std::num::NonZeroUsize;
    use std::time::Duration;
    use tokio::time::Instant;

    #[tokio::test(start_paused = true)]
    async fn admission_and_execution_share_the_same_deadline() {
        let bridge =
            BoundedBlockingBridge::new(NonZeroUsize::new(1).unwrap(), RuntimeHealth::with_owner(1));
        let permit = bridge.permits.clone().acquire_owned().await.unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let (poll_tx, poll_rx) = tokio::sync::oneshot::channel();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let task = tokio::spawn(async move {
            poll_tx.send(()).unwrap();
            bridge
                .run_with_policy_at(
                    deadline,
                    BlockingCompletionPolicy::AbandonResponseAtDeadline,
                    BlockingStallCounter::CoreBridge,
                    move || {
                        started_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                        Ok(())
                    },
                )
                .await
        });
        poll_rx.await.unwrap();
        tokio::time::advance(Duration::from_secs(4)).await;
        drop(permit);
        started_rx.await.unwrap();
        tokio::time::advance(Duration::from_secs(6)).await;
        assert!(matches!(
            task.await.unwrap(),
            Err(BlockingBridgeError::DeadlineAfterStart)
        ));
        assert_eq!(Instant::now(), deadline);
        release_tx.send(()).unwrap();
    }
}
