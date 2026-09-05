//! Shared router-local async support primitives.

use std::future::Future;

use std::num::NonZeroUsize;

use std::sync::Arc;

use atm_core::api::RequestDeadline;

use atm_core::error::AtmError;

use atm_core::protocol::RequestId;

use atm_core::send::WarningEntry;

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
    permits: Arc<tokio::sync::Semaphore>,
    pub(crate) runtime_health: RuntimeHealth,
}

impl ControlPathSyncBridge {
    pub(crate) fn new(capacity: NonZeroUsize, runtime_health: RuntimeHealth) -> Self {
        Self {
            permits: Arc::new(tokio::sync::Semaphore::new(capacity.get())),
            runtime_health,
        }
    }

    pub(crate) async fn run<T, F>(&self, deadline: RequestDeadline, job: F) -> Result<T, AtmError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, AtmError> + Send + 'static,
    {
        let remaining = deadline.remaining().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "request deadline expired before replacement blocking core operation",
            )
        })?;
        let permit = tokio::time::timeout(remaining, Arc::clone(&self.permits).acquire_owned())
            .await
            .map_err(|_| {
                AtmError::daemon_unavailable(
                    "request deadline expired before replacement blocking core operation",
                )
            })?
            .map_err(|_| {
                AtmError::daemon_unavailable("replacement blocking core bridge is shutting down")
            })?;
        if deadline.expired() {
            return Err(AtmError::daemon_unavailable(
                "request deadline expired before replacement blocking core operation started",
            ));
        }
        // The blocking job itself is intentionally not wrapped in a
        // `tokio::time::timeout`: a durable storage write must run to
        // completion rather than be abandoned mid-transaction. `elapsed` is
        // therefore observability, not enforcement -- it records when a job
        // outlived the budget it was dispatched with, without changing
        // whether or how long the job runs.
        let started_at = std::time::Instant::now();
        let outcome = tokio::task::spawn_blocking(job).await.map_err(|source| {
            AtmError::new(
                atm_core::error::AtmErrorCode::InternalError,
                "replacement storage write task ended unexpectedly",
            )
            .with_cause(source)
        })?;
        let elapsed = started_at.elapsed();
        if elapsed > remaining {
            self.runtime_health.record_blocking_core_bridge_stall();
            tracing::warn!(
                subsystem = "atm_http_runtime.blocking_core_bridge",
                action = "blocking_job",
                outcome = "budget_exceeded",
                elapsed = ?elapsed,
                budget = ?remaining,
                "blocking core bridge job outlived its remaining request budget"
            );
        }
        drop(permit);
        outcome
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
