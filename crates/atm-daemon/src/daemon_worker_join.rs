//! Bounded shutdown joins for daemon-owned worker threads.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::Duration;

use atm_core::error::AtmError;

/// Common shutdown deadline for local daemon worker pools.
#[cfg(unix)]
pub(crate) const LOCAL_WORKER_JOIN_DEADLINE: Duration = Duration::from_millis(250);

/// The completion signal and thread handle for one daemon-owned worker.
///
/// The worker owns the sender and drops it only when it exits, so a closed
/// receiver proves the subsequent `join` cannot block.
pub(crate) struct CompletionTrackedJoinHandle<T> {
    pub(crate) completion_rx: Receiver<()>,
    pub(crate) join_handle: JoinHandle<T>,
}

impl<T> std::fmt::Debug for CompletionTrackedJoinHandle<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CompletionTrackedJoinHandle")
            .finish_non_exhaustive()
    }
}

/// Static observability and error contract for one bounded worker join.
#[derive(Clone, Copy)]
pub(crate) struct JoinTimeoutPolicy {
    pub(crate) subsystem: &'static str,
    pub(crate) worker_kind: &'static str,
    pub(crate) panic_message: &'static str,
    pub(crate) timeout_message: &'static str,
}

/// Join one completion-tracked worker without allowing a wedge to block the
/// daemon indefinitely. Dropping the handle on timeout intentionally detaches
/// that worker after emitting a lifecycle-wedge error.
pub(crate) fn join_with_timeout<T>(
    worker: CompletionTrackedJoinHandle<T>,
    timeout: Duration,
    policy: JoinTimeoutPolicy,
) -> Result<T, AtmError> {
    match worker.completion_rx.recv_timeout(timeout) {
        Ok(()) | Err(RecvTimeoutError::Disconnected) => worker
            .join_handle
            .join()
            .map_err(|_| AtmError::daemon_unavailable(policy.panic_message)),
        Err(RecvTimeoutError::Timeout) => {
            tracing::warn!(
                subsystem = policy.subsystem,
                action = "shutdown_join",
                outcome = "deadline_exceeded",
                worker_kind = policy.worker_kind,
                timeout_ms = timeout.as_millis() as u64,
                "daemon worker exceeded the shutdown join deadline; detaching"
            );
            Err(AtmError::daemon_lifecycle_wedge(policy.timeout_message))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;

    use super::*;

    const TEST_POLICY: JoinTimeoutPolicy = JoinTimeoutPolicy {
        subsystem: "daemon_worker_join_test",
        worker_kind: "wedged test worker",
        panic_message: "test worker panicked",
        timeout_message: "test worker exceeded the shutdown join deadline",
    };

    #[test]
    fn timeout_detaches_a_wedged_worker_without_blocking_shutdown() {
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let (finished_tx, finished_rx) = mpsc::sync_channel(1);
        let (completion_tx, completion_rx) = mpsc::sync_channel(1);
        let join_handle = thread::spawn(move || {
            let _completion_tx = completion_tx;
            started_tx.send(()).expect("signal worker start");
            release_rx.recv().expect("release wedged worker");
            finished_tx.send(()).expect("signal worker exit");
        });
        started_rx.recv().expect("wait for worker start");

        let error = join_with_timeout(
            CompletionTrackedJoinHandle {
                completion_rx,
                join_handle,
            },
            Duration::ZERO,
            TEST_POLICY,
        )
        .expect_err("a wedged worker must not block shutdown");

        assert!(error.message().contains("shutdown join deadline"));
        release_tx.send(()).expect("release detached worker");
        finished_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("detached worker exits after release");
    }
}
