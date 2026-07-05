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

/// Controls whether reaping a panicked dispatch worker escalates as an error.
///
/// Two independent call sites race to reap the same tracked dispatch handles: the
/// accept-loop's opportunistic bookkeeping reap, and the per-connection worker's own
/// post-response reap. Whichever wins observes the panic; the other never sees it again,
/// since the handle has already been removed from the tracked list. Escalating
/// unconditionally would make a single dispatcher panic non-deterministically fatal or
/// benign depending on which caller happened to win that race.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DispatchPanicHandling {
    /// Log the panic and continue; used by opportunistic bookkeeping reaps where a single
    /// panicked worker must not be treated as a fatal accept-loop or connection error.
    LogAndContinue,
    /// Propagate the panic as an error; used by the deliberate shutdown drain, where a
    /// wedged or panicked worker blocking graceful shutdown is worth surfacing.
    Escalate,
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

    pub(crate) fn register_background_work(self: &Arc<Self>) -> ActiveDispatchGuard {
        self.register_dispatch_work()
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

    /// Reaps completed dispatch workers without escalating a panicked worker as an error.
    ///
    /// This is the opportunistic bookkeeping reap used by the accept loop between
    /// iterations, and by the per-connection worker after writing its response. Both call
    /// sites race each other to reap the same shared handles, so treating a panic here as
    /// fatal would non-deterministically escalate a single-request panic into tearing down
    /// the whole daemon runtime depending on which caller wins the race. A panicked dispatch
    /// worker is logged and otherwise ignored; it has already been removed from the tracked
    /// handle list and cannot be reaped again.
    pub(crate) fn reap_finished_dispatches(&self) -> Result<(), AtmError> {
        self.reap_finished_dispatches_with(DispatchPanicHandling::LogAndContinue)
    }

    /// Reaps completed dispatch workers, escalating a panicked worker as an error.
    ///
    /// This is used by the deliberate shutdown drain (via [`Self::join_tracked_dispatches`]),
    /// where a wedged or panicked worker blocking graceful shutdown is legitimately worth
    /// surfacing to the caller.
    fn reap_finished_dispatches_escalating(&self) -> Result<(), AtmError> {
        self.reap_finished_dispatches_with(DispatchPanicHandling::Escalate)
    }

    fn reap_finished_dispatches_with(
        &self,
        panic_handling: DispatchPanicHandling,
    ) -> Result<(), AtmError> {
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
            if let Err(error) = join_dispatch_handle(handle) {
                match panic_handling {
                    DispatchPanicHandling::Escalate => return Err(error),
                    DispatchPanicHandling::LogAndContinue => {
                        tracing::warn!(
                            subsystem = "active_connection_registry",
                            action = "reap_finished_dispatches",
                            outcome = "panic_recovered",
                            %error,
                            "dispatch worker panicked before completing; opportunistic reap continuing without escalating"
                        );
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn join_tracked_dispatches(&self, timeout: Duration) -> Result<(), AtmError> {
        self.reap_finished_dispatches_escalating()?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Pushes a dispatch handle whose worker thread panics immediately, then blocks until
    /// the panic has fully unwound so `completion_rx` deterministically observes
    /// `Disconnected` before the caller reaps it.
    ///
    /// This deliberately does not synchronize on [`ActiveConnectionRegistry::active_work_items`]:
    /// the worker only increments that counter after it starts running, so polling it from
    /// the caller races the thread scheduler (the caller can observe zero active work items
    /// before the worker has even registered, let alone panicked). Instead, a dedicated
    /// one-shot channel is dropped last during unwinding (after `completion_tx`), so
    /// observing it disconnect proves `completion_tx` has already been dropped too.
    fn push_panicking_dispatch_handle(registry: &Arc<ActiveConnectionRegistry>) {
        let dispatch_registry = Arc::clone(registry);
        let (completion_tx, completion_rx) = std::sync::mpsc::sync_channel(1);
        let (unwound_tx, unwound_rx) = std::sync::mpsc::sync_channel::<()>(1);
        let join_handle = std::thread::Builder::new()
            .name("reap-panic-test-dispatch".to_string())
            .spawn(move || {
                // Declared first so it drops last during unwinding (locals drop in
                // reverse declaration order), after `_completion_tx` below.
                let _unwound_tx = unwound_tx;
                let _dispatch_work = dispatch_registry.register_dispatch_work();
                let _completion_tx = completion_tx;
                panic!("intentional dispatch worker panic for reap test");
            })
            .expect("spawn panicking dispatch worker");
        registry
            .push_dispatch_handle(
                TrackedDispatchHandle {
                    completion_rx,
                    join_handle,
                },
                1,
            )
            .expect("push dispatch handle");
        match unwound_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                panic!("panicking dispatch worker did not unwind within 5s");
            }
        }
    }

    #[test]
    fn reap_finished_dispatches_logs_and_continues_after_panic() {
        let registry = Arc::new(ActiveConnectionRegistry::default());
        push_panicking_dispatch_handle(&registry);

        registry
            .reap_finished_dispatches()
            .expect("opportunistic reap must not escalate a dispatch worker panic");
        assert_eq!(
            registry
                .lock_dispatch_handles()
                .expect("lock dispatch handles")
                .len(),
            0,
            "the panicked handle should have been removed from the tracked list"
        );
    }

    #[test]
    fn join_tracked_dispatches_escalates_after_panic() {
        let registry = Arc::new(ActiveConnectionRegistry::default());
        push_panicking_dispatch_handle(&registry);

        let error = registry
            .join_tracked_dispatches(Duration::from_secs(5))
            .expect_err(
                "the deliberate shutdown drain must surface a panicked dispatch worker as fatal",
            );
        assert!(
            error.message.contains("daemon dispatch thread panicked"),
            "unexpected error: {error:?}"
        );
    }
}
