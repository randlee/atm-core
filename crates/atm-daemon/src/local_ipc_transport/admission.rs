use std::sync::Mutex;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::thread::JoinHandle;
use std::time::Duration;

use atm_core::error::AtmError;

/// Retry interval used only after a bounded local-work queue is full.
pub(crate) const BOUNDED_ADMISSION_RETRY_INTERVAL: Duration = Duration::from_millis(1);

/// Upper bound for waiting on a local worker that has been told to stop.
///
/// The local IPC shutdown path already budgets this duration for tracked
/// dispatches. A worker that does not exit by this deadline is detached so a
/// stalled request cannot wedge daemon shutdown.
pub(crate) const LOCAL_WORKER_JOIN_DEADLINE: Duration = Duration::from_millis(250);

/// A worker whose completion channel closes immediately before its thread can
/// be joined. Dropping this value detaches an unfinished worker deliberately.
pub(crate) struct ShutdownTrackedWorker {
    // `Receiver` is Send but not Sync. The pool is referenced by scoped test
    // threads, so place it behind a mutex even though shutdown consumes it.
    pub(crate) completion_rx: Mutex<Receiver<()>>,
    pub(crate) join_handle: JoinHandle<()>,
}

/// Join stopped local workers without allowing one wedged handler to block
/// daemon shutdown indefinitely.
pub(crate) fn join_workers_with_timeout(
    workers: impl IntoIterator<Item = ShutdownTrackedWorker>,
    timeout: Duration,
    worker_pool: &'static str,
) -> Result<(), AtmError> {
    for worker in workers {
        let completion_rx = worker.completion_rx.into_inner().map_err(|_| {
            AtmError::daemon_unavailable("local IPC worker completion state lock poisoned")
        })?;
        match completion_rx.recv_timeout(timeout) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                worker.join_handle.join().map_err(|_| {
                    AtmError::daemon_unavailable("local IPC worker panicked during shutdown")
                })?
            }
            Err(RecvTimeoutError::Timeout) => {
                tracing::warn!(
                    subsystem = "local_ipc_transport",
                    action = "shutdown_join",
                    outcome = "deadline_exceeded",
                    worker_pool,
                    timeout_ms = timeout.as_millis() as u64,
                    "local IPC worker exceeded the shutdown join deadline; detaching"
                );
                return Err(AtmError::daemon_lifecycle_wedge(
                    "local IPC worker exceeded the shutdown join deadline",
                ));
            }
        }
    }
    Ok(())
}

/// Hand work to a bounded queue with one common saturation contract.
///
/// The caller supplies the lifecycle or deadline check that applies after a
/// full queue. The normal path remains one `try_send` with no retry delay.
pub(crate) fn send_with_bounded_admission<T>(
    sender: &SyncSender<T>,
    work: T,
    mut retry_delay: impl FnMut() -> Result<Duration, AtmError>,
    disconnected_message: &'static str,
) -> Result<(), AtmError> {
    let mut work = work;
    loop {
        match sender.try_send(work) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Full(returned)) => {
                work = returned;
                std::thread::sleep(retry_delay()?);
            }
            Err(TrySendError::Disconnected(_)) => {
                return Err(AtmError::daemon_unavailable(disconnected_message));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;

    use super::*;

    #[test]
    fn full_queue_stops_when_the_retry_boundary_expires() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        sender.send(()).expect("fill queue");

        let error = send_with_bounded_admission(
            &sender,
            (),
            || Err(AtmError::daemon_unavailable("test admission stopped")),
            "test receiver stopped",
        )
        .expect_err("retry boundary must stop a full queue");

        assert!(error.message().contains("test admission stopped"));
    }

    #[test]
    fn worker_join_timeout_detaches_a_wedged_worker() {
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

        let error = join_workers_with_timeout(
            [ShutdownTrackedWorker {
                completion_rx: Mutex::new(completion_rx),
                join_handle,
            }],
            Duration::ZERO,
            "test worker pool",
        )
        .expect_err("a wedged worker must not block shutdown");

        assert!(error.message().contains("shutdown join deadline"));
        release_tx.send(()).expect("release detached worker");
        finished_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("detached worker exits after release");
    }
}
