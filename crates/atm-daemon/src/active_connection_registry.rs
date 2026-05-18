use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use atm_core::error::AtmError;

pub(crate) struct TrackedDispatchHandle {
    pub(crate) completion_rx: std::sync::mpsc::Receiver<()>,
    pub(crate) join_handle: JoinHandle<()>,
}

impl std::fmt::Debug for TrackedDispatchHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrackedDispatchHandle")
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
pub(crate) struct ActiveConnectionRegistry {
    // These counters are updated from independent accept, connection, and dispatch threads, so
    // atomics keep the shutdown/drain accounting wait-free on the hot path.
    active_connections: AtomicUsize,
    active_dispatches: AtomicUsize,
    // JoinHandles stay behind this mutex so shutdown and post-request reap paths can
    // deterministically join finished dispatch workers without racing the accept loop.
    dispatch_handles: Mutex<Vec<TrackedDispatchHandle>>,
    // The drain-state mutex has no payload of its own; it exists only to pair the condition
    // variable with a stable lock while shutdown waits for active work to change.
    drain_state: Mutex<()>,
    drain_wake: Condvar,
}

impl ActiveConnectionRegistry {
    pub(crate) fn register(self: &Arc<Self>) -> ActiveConnectionGuard {
        self.active_connections.fetch_add(1, Ordering::SeqCst);
        ActiveConnectionGuard {
            registry: Arc::clone(self),
        }
    }

    pub(crate) fn register_dispatch_work(self: &Arc<Self>) -> ActiveDispatchGuard {
        self.active_dispatches.fetch_add(1, Ordering::SeqCst);
        ActiveDispatchGuard {
            registry: Arc::clone(self),
        }
    }

    pub(crate) fn active_connections(&self) -> usize {
        self.active_connections.load(Ordering::SeqCst)
    }

    pub(crate) fn active_work_items(&self) -> usize {
        self.active_connections.load(Ordering::SeqCst)
            + self.active_dispatches.load(Ordering::SeqCst)
    }

    pub(crate) fn interrupt_all(&self) {
        self.drain_wake.notify_all();
    }

    pub(crate) fn lock_dispatch_handles(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, Vec<TrackedDispatchHandle>>, AtmError> {
        self.dispatch_handles.lock().map_err(|_| {
            AtmError::daemon_unavailable("active dispatch handle lock poisoned").with_recovery(
                "Restart the daemon; tracked dispatch worker state may be inconsistent after the poisoned lock.",
            )
        })
    }

    pub(crate) fn push_dispatch_handle(
        &self,
        handle: TrackedDispatchHandle,
        max_handles: usize,
    ) -> Result<(), AtmError> {
        let mut handles = match self.lock_dispatch_handles() {
            Ok(handles) => handles,
            Err(error) => {
                let _ = handle.join_handle.join();
                return Err(error);
            }
        };
        if handles.len() >= max_handles {
            let _ = handle.join_handle.join();
            return Err(
                AtmError::daemon_lifecycle_wedge(format!(
                    "tracked daemon dispatch registry exceeded its bounded capacity of {max_handles} handles"
                ))
                .with_recovery(
                    "Restart the daemon; tracked request-work accounting lost its bounded-cap invariant.",
                ),
            );
        }
        handles.push(handle);
        Ok(())
    }

    pub(crate) fn reap_finished_dispatches(&self) -> Result<(), AtmError> {
        let finished = {
            let mut handles = self.lock_dispatch_handles()?;
            let mut pending = Vec::with_capacity(handles.len());
            let mut finished = Vec::new();
            for handle in handles.drain(..) {
                match handle.completion_rx.try_recv() {
                    Ok(()) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        finished.push(handle);
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        pending.push(handle);
                    }
                }
            }
            *handles = pending;
            finished
        };
        for handle in finished {
            join_dispatch_handle(handle)?;
        }
        Ok(())
    }

    pub(crate) fn join_tracked_dispatches(&self, timeout: Duration) -> Result<(), AtmError> {
        self.reap_finished_dispatches()?;
        let handles = {
            let mut handles = self.lock_dispatch_handles()?;
            std::mem::take(&mut *handles)
        };
        for handle in handles {
            join_dispatch_handle_with_timeout(handle, timeout)?;
        }
        Ok(())
    }

    pub(crate) fn wait_for_connection_change(&self, timeout: Duration) -> Result<(), AtmError> {
        let state = self.drain_state.lock().map_err(|_| {
            AtmError::daemon_unavailable("active connection drain lock poisoned").with_recovery(
                "Restart the daemon; shutdown drain coordination can no longer observe connection progress safely.",
            )
        })?;
        let (_state, wait_result) = self
            .drain_wake
            .wait_timeout(state, timeout)
            .map_err(|_| {
                AtmError::daemon_unavailable("active connection drain lock poisoned").with_recovery(
                    "Restart the daemon; shutdown drain coordination can no longer observe connection progress safely.",
                )
            })?;
        if wait_result.timed_out() {
            return Ok(());
        }
        Ok(())
    }

    fn release_connection(&self) {
        self.active_connections.fetch_sub(1, Ordering::SeqCst);
        self.drain_wake.notify_all();
    }
}

pub(crate) struct ActiveConnectionGuard {
    registry: Arc<ActiveConnectionRegistry>,
}

pub(crate) struct ActiveDispatchGuard {
    registry: Arc<ActiveConnectionRegistry>,
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        self.registry.release_connection();
    }
}

impl Drop for ActiveDispatchGuard {
    fn drop(&mut self) {
        self.registry
            .active_dispatches
            .fetch_sub(1, Ordering::SeqCst);
        self.registry.drain_wake.notify_all();
    }
}

fn join_dispatch_handle(handle: TrackedDispatchHandle) -> Result<(), AtmError> {
    handle.join_handle.join().map_err(|_| {
        AtmError::daemon_unavailable("daemon dispatch thread panicked").with_recovery(
            "Restart the daemon; one completed dispatch worker panicked before it could be reaped cleanly.",
        )
    })
}

fn join_dispatch_handle_with_timeout(
    handle: TrackedDispatchHandle,
    timeout: Duration,
) -> Result<(), AtmError> {
    match handle.completion_rx.recv_timeout(timeout) {
        Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            join_dispatch_handle(handle)
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            tracing::warn!(
                subsystem = "active_connection_registry",
                action = "shutdown_join",
                outcome = "deadline_exceeded",
                timeout_ms = timeout.as_millis() as u64,
                "tracked daemon dispatch worker exceeded the shutdown join deadline; detaching"
            );
            Err(AtmError::daemon_lifecycle_wedge(
                "tracked daemon dispatch worker exceeded the shutdown join deadline",
            )
            .with_recovery(
                "Restart the daemon; a request worker outlived the bounded shutdown window.",
            ))
        }
    }
}
