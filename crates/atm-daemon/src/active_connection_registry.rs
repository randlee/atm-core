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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DispatchReapSummary {
    pub(crate) recovered_panics: usize,
}

#[derive(Debug, Default)]
pub(crate) struct ActiveConnectionRegistry {
    // These counters are updated from independent accept, connection, and dispatch threads, so
    // atomics keep the shutdown/drain accounting wait-free on the hot path.
    active_connections: AtomicUsize,
    active_dispatches: AtomicUsize,
    // Reserve bounded join-table capacity before a worker thread is created. This keeps
    // saturation off the accept-loop hot path and prevents a full table from forcing a join.
    dispatch_handle_slots: AtomicUsize,
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

    /// Reserve a connection slot before spawning its worker.  Admission is
    /// decided atomically so an accept loop cannot over-admit while a newly
    /// spawned worker has not yet incremented the counter.
    pub(crate) fn try_register(
        self: &Arc<Self>,
        maximum_connections: usize,
    ) -> Option<ActiveConnectionGuard> {
        let mut current = self.active_connections.load(Ordering::SeqCst);
        loop {
            if current >= maximum_connections {
                return None;
            }
            match self.active_connections.compare_exchange_weak(
                current,
                current + 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    return Some(ActiveConnectionGuard {
                        registry: Arc::clone(self),
                    });
                }
                Err(observed) => current = observed,
            }
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
        self.dispatch_handles
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("active dispatch handle lock poisoned"))
    }

    pub(crate) fn try_reserve_dispatch_handle(
        self: &Arc<Self>,
        max_handles: usize,
    ) -> Result<Option<DispatchHandleReservation>, AtmError> {
        let mut current = self.dispatch_handle_slots.load(Ordering::SeqCst);
        loop {
            if current >= max_handles {
                return Ok(None);
            }
            match self.dispatch_handle_slots.compare_exchange_weak(
                current,
                current + 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    return Ok(Some(DispatchHandleReservation {
                        registry: Arc::clone(self),
                        committed: false,
                    }));
                }
                Err(observed) => current = observed,
            }
        }
    }

    pub(crate) fn push_reserved_dispatch_handle(
        &self,
        reservation: DispatchHandleReservation,
        handle: TrackedDispatchHandle,
    ) -> Result<(), AtmError> {
        let mut handles = match self.lock_dispatch_handles() {
            Ok(handles) => handles,
            Err(error) => {
                let _ = handle.join_handle.join();
                return Err(error);
            }
        };
        handles.push(handle);
        reservation.commit();
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
    pub(crate) fn reap_finished_dispatches(&self) -> Result<DispatchReapSummary, AtmError> {
        self.reap_finished_dispatches_with(DispatchPanicHandling::LogAndContinue)
    }

    /// Reaps completed dispatch workers, escalating a panicked worker as an error.
    ///
    /// This is used by the deliberate shutdown drain (via [`Self::join_tracked_dispatches`]),
    /// where a wedged or panicked worker blocking graceful shutdown is legitimately worth
    /// surfacing to the caller.
    fn reap_finished_dispatches_escalating(&self) -> Result<DispatchReapSummary, AtmError> {
        self.reap_finished_dispatches_with(DispatchPanicHandling::Escalate)
    }

    fn reap_finished_dispatches_with(
        &self,
        panic_handling: DispatchPanicHandling,
    ) -> Result<DispatchReapSummary, AtmError> {
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
        self.release_dispatch_handle_slots(finished.len());
        let mut summary = DispatchReapSummary::default();
        for handle in finished {
            if let Err(error) = join_dispatch_handle(handle) {
                match panic_handling {
                    DispatchPanicHandling::Escalate => return Err(error),
                    DispatchPanicHandling::LogAndContinue => {
                        summary.recovered_panics += 1;
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
        Ok(summary)
    }

    pub(crate) fn join_tracked_dispatches(&self, timeout: Duration) -> Result<(), AtmError> {
        let _ = self.reap_finished_dispatches_escalating()?;
        let handles = {
            let mut handles = self.lock_dispatch_handles()?;
            std::mem::take(&mut *handles)
        };
        self.release_dispatch_handle_slots(handles.len());
        for handle in handles {
            join_dispatch_handle_with_timeout(handle, timeout)?;
        }
        Ok(())
    }

    pub(crate) fn wait_for_connection_change(&self, timeout: Duration) -> Result<(), AtmError> {
        let state = self
            .drain_state
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("active connection drain lock poisoned"))?;
        let (_state, wait_result) = self
            .drain_wake
            .wait_timeout(state, timeout)
            .map_err(|_| AtmError::daemon_unavailable("active connection drain lock poisoned"))?;
        if wait_result.timed_out() {
            return Ok(());
        }
        Ok(())
    }

    fn release_connection(&self) {
        self.active_connections.fetch_sub(1, Ordering::SeqCst);
        self.drain_wake.notify_all();
    }

    fn release_dispatch_handle_slots(&self, count: usize) {
        if count != 0 {
            self.dispatch_handle_slots
                .fetch_sub(count, Ordering::SeqCst);
        }
    }
}

pub(crate) struct ActiveConnectionGuard {
    registry: Arc<ActiveConnectionRegistry>,
}

pub(crate) struct ActiveDispatchGuard {
    registry: Arc<ActiveConnectionRegistry>,
}

pub(crate) struct DispatchHandleReservation {
    registry: Arc<ActiveConnectionRegistry>,
    committed: bool,
}

impl DispatchHandleReservation {
    fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for DispatchHandleReservation {
    fn drop(&mut self) {
        if !self.committed {
            self.registry.release_dispatch_handle_slots(1);
        }
    }
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
    handle
        .join_handle
        .join()
        .map_err(|_| AtmError::daemon_unavailable("daemon dispatch thread panicked"))
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
        let reservation = registry
            .try_reserve_dispatch_handle(1)
            .expect("reserve dispatch handle")
            .expect("dispatch handle capacity");
        registry
            .push_reserved_dispatch_handle(
                reservation,
                TrackedDispatchHandle {
                    completion_rx,
                    join_handle,
                },
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

        let summary = registry
            .reap_finished_dispatches()
            .expect("opportunistic reap must not escalate a dispatch worker panic");
        assert_eq!(summary.recovered_panics, 1);
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
            error.message().contains("daemon dispatch thread panicked"),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn connection_admission_is_atomic_and_recovers_after_drop() {
        let registry = Arc::new(ActiveConnectionRegistry::default());
        let first = registry.try_register(1).expect("first slot admitted");
        assert!(
            registry.try_register(1).is_none(),
            "cap must reject admission"
        );
        drop(first);
        assert!(
            registry.try_register(1).is_some(),
            "released slot must become available again"
        );
    }

    #[test]
    fn shutdown_joins_completed_dispatch_without_detaching_tracked_work() {
        let registry = Arc::new(ActiveConnectionRegistry::default());
        let worker_registry = Arc::clone(&registry);
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let (completion_tx, completion_rx) = std::sync::mpsc::sync_channel(1);
        let join_handle = std::thread::spawn(move || {
            let _work = worker_registry.register_dispatch_work();
            started_tx.send(()).expect("report tracked worker start");
            release_rx.recv().expect("release tracked worker");
            completion_tx
                .send(())
                .expect("report tracked worker completion");
        });
        let reservation = registry
            .try_reserve_dispatch_handle(1)
            .expect("reserve dispatch handle")
            .expect("dispatch handle capacity");
        registry
            .push_reserved_dispatch_handle(
                reservation,
                TrackedDispatchHandle {
                    completion_rx,
                    join_handle,
                },
            )
            .expect("track dispatch worker");
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("worker entered tracked runtime work");
        assert_eq!(registry.active_work_items(), 1);
        release_tx.send(()).expect("release worker");

        registry
            .join_tracked_dispatches(Duration::from_secs(1))
            .expect("shutdown joins completed tracked work");
        assert_eq!(registry.active_work_items(), 0);
        assert!(
            registry
                .lock_dispatch_handles()
                .expect("lock dispatch handles")
                .is_empty(),
            "shutdown leaves no detached tracked dispatch"
        );
    }
}
