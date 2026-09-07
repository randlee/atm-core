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
struct BoundedBlockingBridge {
    permits: Arc<tokio::sync::Semaphore>,
    runtime_health: RuntimeHealth,
}

enum BlockingBridgeError {
    DeadlineBeforeStart,
    DeadlineAfterStart,
    Closed,
    Join(tokio::task::JoinError),
    Operation(AtmError),
}

impl BoundedBlockingBridge {
    fn new(capacity: NonZeroUsize, runtime_health: RuntimeHealth) -> Self {
        Self {
            permits: Arc::new(tokio::sync::Semaphore::new(capacity.get())),
            runtime_health,
        }
    }

    async fn run<T, F>(
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
        let remaining = deadline
            .remaining()
            .ok_or(BlockingBridgeError::DeadlineBeforeStart)?;
        let permit = tokio::time::timeout(remaining, Arc::clone(&self.permits).acquire_owned())
            .await
            .map_err(|_| BlockingBridgeError::DeadlineBeforeStart)?
            .map_err(|_| BlockingBridgeError::Closed)?;
        if deadline.expired() {
            return Err(BlockingBridgeError::DeadlineBeforeStart);
        }
        let started_at = std::time::Instant::now();
        match policy {
            BlockingCompletionPolicy::AwaitCompletion => {
                let outcome = tokio::task::spawn_blocking(job)
                    .await
                    .map_err(BlockingBridgeError::Join)?;
                if started_at.elapsed() > remaining {
                    self.record_stall(stall_counter, started_at.elapsed(), remaining);
                }
                drop(permit);
                outcome.map_err(BlockingBridgeError::Operation)
            }
            BlockingCompletionPolicy::AbandonResponseAtDeadline => {
                let outcome = tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    job()
                });
                match tokio::time::timeout(remaining, outcome).await {
                    Ok(Ok(result)) => result.map_err(BlockingBridgeError::Operation),
                    Ok(Err(source)) => Err(BlockingBridgeError::Join(source)),
                    Err(_) => {
                        self.record_stall(stall_counter, started_at.elapsed(), remaining);
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
            .run(
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
pub(super) fn receiver_hook_deadline(deadline: RequestDeadline) -> Option<RequestDeadline> {
    deadline
        .remaining()
        .and_then(|remaining| remaining.checked_sub(RESPONSE_HANDOFF_GRACE))
        .filter(|remaining| !remaining.is_zero())
        .map(RequestDeadline::after)
}

impl ControlPathSyncBridge {
    pub(crate) fn new(capacity: NonZeroUsize, runtime_health: RuntimeHealth) -> Self {
        Self {
            bridge: BoundedBlockingBridge::new(capacity, runtime_health),
        }
    }

    pub(crate) async fn run<T, F>(&self, deadline: RequestDeadline, job: F) -> Result<T, AtmError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, AtmError> + Send + 'static,
    {
        self.bridge
            .run(
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
        let task = tokio::spawn(async move {
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
        });
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
