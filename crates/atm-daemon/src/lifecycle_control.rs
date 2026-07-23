use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use atm_core::error::AtmError;
use signal_hook::SigId;

#[cfg(test)]
use crate::DaemonSubsystem;
use crate::SubsystemObservability;

const LIFECYCLE_WORKER_JOIN_DEADLINE: Duration = Duration::from_secs(5);
// Keep the Unix wake worker responsive to lifecycle signals without turning the
// blocking read loop into a tight poll when no wake byte is pending.
#[cfg(unix)]
const UNIX_WAKE_READ_TIMEOUT: Duration = Duration::from_millis(100);

// Installation takes this global slot only after the outer install lock so concurrent daemon
// startup/teardown never races lifecycle-hook ownership or leaves a half-installed worker behind.
static SHARED_LIFECYCLE: Mutex<Option<SharedLifecycleControlState>> = Mutex::new(None);
// The separate install lock preserves a consistent install/shutdown order around signal-hook
// registration without forcing that cross-thread coordination through the shared state mutex.
static INSTALL_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone)]
pub(crate) struct LifecycleControlSourceAdapter {
    // The lifecycle flags are flipped from signal-hook handlers and read from normal daemon
    // threads, so atomics provide the minimum cross-thread synchronization without widening the
    // signal-facing surface to a heavier lock.
    terminate: Arc<AtomicBool>,
    #[cfg_attr(windows, allow(dead_code))]
    reload: Arc<AtomicBool>,
    #[cfg_attr(windows, allow(dead_code))]
    state_change: Arc<LifecycleStateChange>,
    observability: SubsystemObservability,
}

#[derive(Debug)]
struct SharedLifecycleControlState {
    terminate: Arc<AtomicBool>,
    reload: Arc<AtomicBool>,
    state_change: Arc<LifecycleStateChange>,
    worker: Option<LifecycleWorkerRegistration>,
}

#[derive(Debug)]
struct LifecycleWorkerRegistration {
    shutdown: Arc<AtomicBool>,
    signal_ids: Vec<SigId>,
    join_handle: std::thread::JoinHandle<()>,
}

#[derive(Debug)]
struct LifecycleStateChange {
    // The generation counter is guarded so waiters and signal hooks can observe an ordered
    // wake sequence without relying on lossy edge-triggered polling alone.
    generation: Mutex<u64>,
    wake: Condvar,
}

#[cfg_attr(windows, allow(dead_code))]
impl LifecycleStateChange {
    fn new() -> Self {
        Self {
            generation: Mutex::new(0),
            wake: Condvar::new(),
        }
    }

    fn notify(&self) -> Result<(), AtmError> {
        let mut generation = self.generation.lock().map_err(|_| {
            AtmError::daemon_unavailable("daemon lifecycle state-change lock poisoned")
        })?;
        *generation += 1;
        self.wake.notify_all();
        Ok(())
    }

    #[cfg_attr(windows, allow(dead_code))]
    fn snapshot(&self) -> Result<u64, AtmError> {
        self.generation
            .lock()
            .map(|generation| *generation)
            .map_err(|_| {
                AtmError::daemon_unavailable("daemon lifecycle state-change lock poisoned")
            })
    }

    fn wait_for_change_timeout(
        &self,
        observed_generation: &mut u64,
        timeout: std::time::Duration,
    ) -> Result<bool, AtmError> {
        let mut generation = self.generation.lock().map_err(|_| {
            AtmError::daemon_unavailable("daemon lifecycle state-change lock poisoned")
        })?;
        while *generation == *observed_generation {
            let (updated_generation, wait_result) = self
                .wake
                .wait_timeout_while(generation, timeout, |current_generation| {
                    *current_generation == *observed_generation
                })
                .map_err(|_| {
                    AtmError::daemon_unavailable("daemon lifecycle state-change lock poisoned")
                })?;
            generation = updated_generation;
            if wait_result.timed_out() && *generation == *observed_generation {
                return Ok(false);
            }
        }
        *observed_generation = *generation;
        Ok(true)
    }

    #[cfg_attr(any(windows, not(test)), allow(dead_code))]
    fn wait_for_change(&self, observed_generation: &mut u64) -> Result<(), AtmError> {
        // The Windows lifecycle worker still relies on bounded polling as the
        // accepted portability exception documented in
        // `docs/atm-daemon/architecture.md`.
        const LIFECYCLE_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
        while !self.wait_for_change_timeout(observed_generation, LIFECYCLE_WAIT_TIMEOUT)? {}
        Ok(())
    }
}

impl LifecycleControlSourceAdapter {
    #[cfg(test)]
    pub(crate) fn install() -> Result<Self, AtmError> {
        Self::install_with_observability(SubsystemObservability::disabled(
            DaemonSubsystem::LifecycleControl,
        ))
    }

    pub(crate) fn install_with_observability(
        observability: SubsystemObservability,
    ) -> Result<Self, AtmError> {
        let _guard = INSTALL_LOCK
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("daemon lifecycle install lock poisoned"))?;
        let mut shared = SHARED_LIFECYCLE.lock().map_err(|_| {
            AtmError::daemon_unavailable("daemon lifecycle control state lock poisoned")
        })?;
        if shared.is_none() {
            let terminate = Arc::new(AtomicBool::new(false));
            let reload = Arc::new(AtomicBool::new(false));
            let state_change = Arc::new(LifecycleStateChange::new());
            *shared = Some(SharedLifecycleControlState {
                terminate,
                reload,
                state_change,
                worker: None,
            });
        }
        let shared = shared.as_mut().ok_or_else(|| {
            AtmError::daemon_unavailable("daemon lifecycle control source was not initialized")
        })?;
        if shared.worker.is_none() {
            shared.worker = Some(install_platform_hooks(
                &shared.terminate,
                &shared.reload,
                &shared.state_change,
            )?);
        }
        observability.emit_or_warn(
            "install_hooks",
            "ok",
            "daemon lifecycle-control hooks are installed",
        );
        Ok(Self {
            terminate: Arc::clone(&shared.terminate),
            reload: Arc::clone(&shared.reload),
            state_change: Arc::clone(&shared.state_change),
            observability,
        })
    }

    #[cfg(test)]
    pub(crate) fn new_for_test() -> Self {
        let state_change = Arc::new(LifecycleStateChange::new());
        Self {
            terminate: Arc::new(AtomicBool::new(false)),
            reload: Arc::new(AtomicBool::new(false)),
            state_change,
            observability: SubsystemObservability::disabled(DaemonSubsystem::LifecycleControl),
        }
    }

    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) fn terminate_requested(&self) -> bool {
        self.terminate.load(Ordering::SeqCst)
    }

    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) fn take_reload_requested(&self) -> bool {
        self.reload.swap(false, Ordering::SeqCst)
    }

    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) fn event_generation(&self) -> Result<u64, AtmError> {
        self.state_change.snapshot()
    }

    pub(crate) fn notify_state_change(&self) -> Result<(), AtmError> {
        self.state_change.notify()
    }

    #[cfg_attr(any(windows, not(test)), allow(dead_code))]
    pub(crate) fn wait_for_state_change(
        &self,
        observed_generation: &mut u64,
    ) -> Result<(), AtmError> {
        self.state_change.wait_for_change(observed_generation)
    }

    #[cfg_attr(any(windows, not(test)), allow(dead_code))]
    pub(crate) fn wait_for_state_change_timeout(
        &self,
        observed_generation: &mut u64,
        timeout: std::time::Duration,
    ) -> Result<bool, AtmError> {
        self.state_change
            .wait_for_change_timeout(observed_generation, timeout)
    }

    #[cfg(all(test, unix))]
    pub(crate) fn terminate_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.terminate)
    }

    pub(crate) fn shutdown_worker_with_timeout(&self) -> Result<(), AtmError> {
        let Some(worker) = take_lifecycle_worker()? else {
            return Ok(());
        };
        worker.shutdown.store(true, Ordering::SeqCst);
        for signal_id in &worker.signal_ids {
            let _ = signal_hook::low_level::unregister(*signal_id);
        }
        let _ = self.state_change.notify();

        let (join_helper, result_rx) = spawn_lifecycle_join_helper(worker)?;
        finish_lifecycle_shutdown(&self.observability, join_helper, result_rx)
    }

    #[cfg(test)]
    pub(crate) fn reset_shared_state_for_test(&self) -> Result<(), AtmError> {
        self.shutdown_worker_with_timeout()?;
        let _guard = INSTALL_LOCK
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("daemon lifecycle install lock poisoned"))?;
        let mut shared = SHARED_LIFECYCLE.lock().map_err(|_| {
            AtmError::daemon_unavailable("daemon lifecycle control state lock poisoned")
        })?;
        *shared = None;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_terminate_for_test(&self, value: bool) {
        self.terminate.store(value, Ordering::SeqCst);
        self.state_change
            .notify()
            .expect("notify test terminate state change");
    }

    #[cfg(test)]
    pub(crate) fn set_reload_for_test(&self, value: bool) {
        self.reload.store(value, Ordering::SeqCst);
        self.state_change
            .notify()
            .expect("notify test reload state change");
    }

    #[cfg(test)]
    pub(crate) fn reload_requested_for_test(&self) -> bool {
        self.reload.load(Ordering::SeqCst)
    }
}

#[cfg(unix)]
fn run_unix_lifecycle_wake_worker(
    mut wake_read: std::os::unix::net::UnixStream,
    shutdown: Arc<AtomicBool>,
    terminate: Arc<AtomicBool>,
    reload: Arc<AtomicBool>,
    state_change: Arc<LifecycleStateChange>,
) {
    use std::io::Read;

    let _ = wake_read.set_read_timeout(Some(UNIX_WAKE_READ_TIMEOUT));
    let mut wake_buffer = [0_u8; 32];
    loop {
        if shutdown.load(Ordering::SeqCst) {
            let _ = state_change.notify();
            return;
        }
        match wake_read.read(&mut wake_buffer) {
            Ok(0) => {
                let _ = state_change.notify();
                return;
            }
            Ok(_) => {
                if terminate.load(Ordering::SeqCst) || reload.load(Ordering::SeqCst) {
                    let _ = state_change.notify();
                }
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return,
        }
    }
}

#[cfg(unix)]
fn install_platform_hooks(
    terminate: &Arc<AtomicBool>,
    reload: &Arc<AtomicBool>,
    state_change: &Arc<LifecycleStateChange>,
) -> Result<LifecycleWorkerRegistration, AtmError> {
    use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};

    let shutdown = Arc::new(AtomicBool::new(false));
    let (wake_read, wake_write) = create_lifecycle_wake_pair()?;
    let signal_ids = vec![
        register_lifecycle_signal_flag(SIGINT, Arc::clone(terminate))?,
        register_lifecycle_signal_flag(SIGTERM, Arc::clone(terminate))?,
        register_lifecycle_signal_flag(SIGHUP, Arc::clone(reload))?,
        register_lifecycle_signal_pipe(SIGINT, clone_lifecycle_wake_write(&wake_write)?)?,
        register_lifecycle_signal_pipe(SIGTERM, clone_lifecycle_wake_write(&wake_write)?)?,
        register_lifecycle_signal_pipe(SIGHUP, wake_write)?,
    ];
    let shutdown_for_worker = Arc::clone(&shutdown);
    let terminate = Arc::clone(terminate);
    let reload = Arc::clone(reload);
    let state_change = Arc::clone(state_change);
    let join_handle = std::thread::Builder::new()
        .name("atm-daemon-lifecycle-unix".to_string())
        .spawn(move || {
            run_unix_lifecycle_wake_worker(
                wake_read,
                shutdown_for_worker,
                terminate,
                reload,
                state_change,
            );
        })
        .map_err(|_source| {
            AtmError::daemon_unavailable("failed to spawn daemon lifecycle signal worker")
        })?;
    Ok(LifecycleWorkerRegistration {
        shutdown,
        signal_ids,
        join_handle,
    })
}

fn take_lifecycle_worker() -> Result<Option<LifecycleWorkerRegistration>, AtmError> {
    let _guard = INSTALL_LOCK
        .lock()
        .map_err(|_| AtmError::daemon_unavailable("daemon lifecycle install lock poisoned"))?;
    let mut shared = SHARED_LIFECYCLE.lock().map_err(|_| {
        AtmError::daemon_unavailable("daemon lifecycle control state lock poisoned")
    })?;
    Ok(shared.as_mut().and_then(|shared| shared.worker.take()))
}

fn spawn_lifecycle_join_helper(
    worker: LifecycleWorkerRegistration,
) -> Result<
    (
        std::thread::JoinHandle<()>,
        std::sync::mpsc::Receiver<std::thread::Result<()>>,
    ),
    AtmError,
> {
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    let join_helper = std::thread::Builder::new()
        .name("atm-daemon-lifecycle-join-helper".to_string())
        .spawn(move || {
            let _ = result_tx.send(worker.join_handle.join());
        })
        .map_err(|_source| AtmError::daemon_unavailable("failed to spawn lifecycle join helper"))?;
    Ok((join_helper, result_rx))
}

fn finish_lifecycle_shutdown(
    observability: &SubsystemObservability,
    join_helper: std::thread::JoinHandle<()>,
    result_rx: std::sync::mpsc::Receiver<std::thread::Result<()>>,
) -> Result<(), AtmError> {
    finish_lifecycle_shutdown_with_deadline(
        observability,
        join_helper,
        result_rx,
        LIFECYCLE_WORKER_JOIN_DEADLINE,
    )
}

fn finish_lifecycle_shutdown_with_deadline(
    observability: &SubsystemObservability,
    join_helper: std::thread::JoinHandle<()>,
    result_rx: std::sync::mpsc::Receiver<std::thread::Result<()>>,
    deadline: Duration,
) -> Result<(), AtmError> {
    let result = result_rx.recv_timeout(deadline);
    match result {
        Ok(Ok(())) => {
            join_completed_lifecycle_helper(join_helper)?;
            observability.emit_or_warn(
                "shutdown_worker",
                "ok",
                "daemon lifecycle-control worker shut down cleanly",
            );
            Ok(())
        }
        Ok(Err(_)) => {
            let _ = join_completed_lifecycle_helper(join_helper);
            observability.emit_or_warn(
                "shutdown_worker",
                "failed",
                "daemon lifecycle-control worker panicked during shutdown",
            );
            Err(AtmError::daemon_unavailable(
                "daemon lifecycle wake worker panicked during runtime teardown",
            ))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            drop(join_helper);
            observability.emit_or_warn(
                "shutdown_worker",
                "degraded",
                "daemon lifecycle-control worker exceeded its shutdown deadline",
            );
            Err(AtmError::daemon_unavailable(
                "daemon lifecycle wake worker exceeded the bounded shutdown deadline",
            ))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            if join_helper.is_finished() {
                let _ = join_completed_lifecycle_helper(join_helper);
            } else {
                drop(join_helper);
            }
            observability.emit_or_warn(
                "shutdown_worker",
                "failed",
                "daemon lifecycle-control join coordination disconnected during shutdown",
            );
            Err(AtmError::daemon_unavailable(
                "daemon lifecycle wake worker join coordination disconnected during runtime teardown",
            ))
        }
    }
}

fn join_completed_lifecycle_helper(
    join_helper: std::thread::JoinHandle<()>,
) -> Result<(), AtmError> {
    join_helper.join().map_err(|_| {
        AtmError::daemon_unavailable(
            "daemon lifecycle join helper panicked during runtime teardown",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{LIFECYCLE_WORKER_JOIN_DEADLINE, finish_lifecycle_shutdown_with_deadline};
    use crate::{DaemonSubsystem, SubsystemObservability};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    #[test]
    fn finish_lifecycle_shutdown_timeout_returns_without_waiting_for_join_helper_completion() {
        let observability = SubsystemObservability::disabled(DaemonSubsystem::LifecycleControl);
        let (_result_tx, result_rx) = mpsc::channel::<std::thread::Result<()>>();
        let (continue_tx, continue_rx) = mpsc::sync_channel::<()>(0);
        let (done_tx, done_rx) = mpsc::sync_channel::<()>(1);
        let join_helper = std::thread::spawn(move || {
            let _ = continue_rx.recv();
            let _ = done_tx.send(());
        });
        let deadline = LIFECYCLE_WORKER_JOIN_DEADLINE.min(Duration::from_millis(50));

        let started = Instant::now();
        let error = finish_lifecycle_shutdown_with_deadline(
            &observability,
            join_helper,
            result_rx,
            deadline,
        )
        .expect_err("timeout should fail");

        assert!(error.is_daemon_unavailable());
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "timed shutdown should return promptly after its deadline"
        );

        continue_tx
            .send(())
            .expect("release detached lifecycle join helper");
        done_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("join helper should exit after release");
    }
}

#[cfg(unix)]
fn create_lifecycle_wake_pair() -> Result<
    (
        std::os::unix::net::UnixStream,
        std::os::unix::net::UnixStream,
    ),
    AtmError,
> {
    std::os::unix::net::UnixStream::pair().map_err(|_source| {
        AtmError::daemon_unavailable("failed to create daemon lifecycle wake pipe")
    })
}

#[cfg(unix)]
fn clone_lifecycle_wake_write(
    wake_write: &std::os::unix::net::UnixStream,
) -> Result<std::os::unix::net::UnixStream, AtmError> {
    wake_write.try_clone().map_err(|_source| {
        AtmError::daemon_unavailable("failed to clone daemon lifecycle wake pipe")
    })
}

#[cfg(unix)]
fn register_lifecycle_signal_flag(
    signal: i32,
    flag: Arc<AtomicBool>,
) -> Result<signal_hook::SigId, AtmError> {
    signal_hook::flag::register(signal, flag).map_err(|_source| {
        AtmError::daemon_unavailable("failed to install daemon lifecycle signal handlers")
    })
}

#[cfg(unix)]
fn register_lifecycle_signal_pipe(
    signal: i32,
    wake_write: std::os::unix::net::UnixStream,
) -> Result<signal_hook::SigId, AtmError> {
    signal_hook::low_level::pipe::register(signal, wake_write).map_err(|_source| {
        AtmError::daemon_unavailable("failed to install daemon lifecycle signal handlers")
    })
}

#[cfg(windows)]
fn install_platform_hooks(
    terminate: &Arc<AtomicBool>,
    reload: &Arc<AtomicBool>,
    state_change: &Arc<LifecycleStateChange>,
) -> Result<LifecycleWorkerRegistration, AtmError> {
    use signal_hook::consts::{SIGBREAK, SIGINT, SIGTERM};
    use signal_hook::flag as signal_flag;

    let shutdown = Arc::new(AtomicBool::new(false));
    let mut signal_ids = Vec::new();
    signal_ids.push(
        signal_flag::register(SIGINT, Arc::clone(terminate)).map_err(|_source| {
            AtmError::daemon_unavailable("failed to install daemon lifecycle signal handlers")
        })?,
    );
    signal_ids.push(
        signal_flag::register(SIGTERM, Arc::clone(terminate)).map_err(|_source| {
            AtmError::daemon_unavailable("failed to install daemon lifecycle signal handlers")
        })?,
    );
    signal_ids.push(
        signal_flag::register(SIGBREAK, Arc::clone(reload)).map_err(|_source| {
            AtmError::daemon_unavailable("failed to install daemon lifecycle signal handlers")
        })?,
    );

    let shutdown_for_worker = Arc::clone(&shutdown);
    let terminate = Arc::clone(terminate);
    let reload = Arc::clone(reload);
    let state_change = Arc::clone(state_change);
    let join_handle = std::thread::Builder::new()
        .name("atm-daemon-lifecycle-windows".to_string())
        .spawn(move || {
            let mut observed_reload = reload.load(Ordering::SeqCst);
            loop {
                if shutdown_for_worker.load(Ordering::SeqCst) {
                    let _ = state_change.notify();
                    return;
                }
                if terminate.load(Ordering::SeqCst) {
                    let _ = state_change.notify();
                    return;
                }
                let reload_now = reload.load(Ordering::SeqCst);
                if reload_now != observed_reload {
                    observed_reload = reload_now;
                    let _ = state_change.notify();
                }
                // `signal_hook::flag` does not expose a blocking cross-platform wake primitive on
                // Windows, so the lifecycle worker uses one bounded polling exception recorded
                // explicitly in `docs/atm-daemon/architecture.md`.
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        })
        .map_err(|_source| {
            AtmError::daemon_unavailable("failed to spawn daemon lifecycle signal worker")
        })?;
    Ok(LifecycleWorkerRegistration {
        shutdown,
        signal_ids,
        join_handle,
    })
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::LifecycleControlSourceAdapter;
    use std::sync::mpsc;
    use std::thread::ThreadId;
    use std::time::Duration;

    struct SharedLifecycleResetGuard {
        adapter: LifecycleControlSourceAdapter,
    }

    impl SharedLifecycleResetGuard {
        fn install() -> Self {
            let seed = LifecycleControlSourceAdapter::install().expect("install lifecycle seed");
            seed.reset_shared_state_for_test()
                .expect("reset shared lifecycle before test");
            let adapter = LifecycleControlSourceAdapter::install().expect("install lifecycle");
            adapter.set_terminate_for_test(false);
            adapter.set_reload_for_test(false);
            Self { adapter }
        }
    }

    impl Drop for SharedLifecycleResetGuard {
        fn drop(&mut self) {
            self.adapter.set_terminate_for_test(false);
            self.adapter.set_reload_for_test(false);
            self.adapter
                .reset_shared_state_for_test()
                .expect("reset shared lifecycle after test");
        }
    }

    fn worker_thread_id() -> ThreadId {
        let shared = super::SHARED_LIFECYCLE.lock().expect("shared lifecycle");
        shared
            .as_ref()
            .and_then(|state| state.worker.as_ref())
            .map(|worker| worker.join_handle.thread().id())
            .expect("worker thread id")
    }

    #[test]
    #[serial_test::serial(env)]
    fn windows_reload_flag_is_shared_across_install_calls() {
        let _reset = SharedLifecycleResetGuard::install();
        let first = LifecycleControlSourceAdapter::install().expect("install first");
        first.set_terminate_for_test(false);
        first.set_reload_for_test(false);

        let second = LifecycleControlSourceAdapter::install().expect("install second");
        first.set_reload_for_test(true);

        assert!(second.take_reload_requested());
        assert!(!second.terminate_requested());
    }

    #[test]
    #[serial_test::serial(env)]
    fn windows_terminate_flag_is_shared_across_install_calls() {
        let _reset = SharedLifecycleResetGuard::install();
        let first = LifecycleControlSourceAdapter::install().expect("install first");
        first.set_terminate_for_test(false);
        first.set_reload_for_test(false);

        let second = LifecycleControlSourceAdapter::install().expect("install second");
        first.set_terminate_for_test(true);

        assert!(second.terminate_requested());
    }

    #[test]
    #[serial_test::serial(env)]
    fn windows_terminate_request_wakes_waiters() {
        let _reset = SharedLifecycleResetGuard::install();
        let first = LifecycleControlSourceAdapter::install().expect("install first");
        first.set_terminate_for_test(false);
        first.set_reload_for_test(false);
        let second = LifecycleControlSourceAdapter::install().expect("install second");
        let mut observed_generation = second.event_generation().expect("generation");
        let waiter = second.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        let join = std::thread::spawn(move || {
            waiter
                .wait_for_state_change(&mut observed_generation)
                .expect("wait for state change");
            tx.send(waiter.terminate_requested())
                .expect("send waiter result");
        });

        first.set_terminate_for_test(true);

        assert!(
            rx.recv_timeout(Duration::from_secs(5))
                .expect("waiter wake result"),
            "terminate wake should set the terminate flag"
        );
        join.join().expect("join waiter");
    }

    #[test]
    #[serial_test::serial(env)]
    fn windows_install_reuses_one_lifecycle_worker_until_shutdown() {
        let _reset = SharedLifecycleResetGuard::install();
        let first = LifecycleControlSourceAdapter::install().expect("install first");
        let first_worker = worker_thread_id();
        let second = LifecycleControlSourceAdapter::install().expect("install second");
        assert_eq!(worker_thread_id(), first_worker);
        second
            .shutdown_worker_with_timeout()
            .expect("shutdown lifecycle worker");
        let third = LifecycleControlSourceAdapter::install().expect("install third");
        assert_ne!(worker_thread_id(), first_worker);
        third
            .shutdown_worker_with_timeout()
            .expect("shutdown lifecycle worker");
        let _ = first;
    }
}

#[cfg(all(test, unix))]
mod unix_tests {
    use super::{LifecycleControlSourceAdapter, run_unix_lifecycle_wake_worker};
    use std::os::unix::net::UnixStream;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread::ThreadId;
    use std::time::Duration;

    fn worker_thread_id() -> ThreadId {
        let shared = super::SHARED_LIFECYCLE.lock().expect("shared lifecycle");
        shared
            .as_ref()
            .and_then(|state| state.worker.as_ref())
            .map(|worker| worker.join_handle.thread().id())
            .expect("worker thread id")
    }

    #[test]
    fn unix_eof_wake_notifies_waiters() {
        let adapter = LifecycleControlSourceAdapter::new_for_test();
        let (wake_read, wake_write) = UnixStream::pair().expect("unix pair");
        let worker_terminate = adapter.terminate_flag();
        let worker_reload = Arc::clone(&adapter.reload);
        let worker_state = Arc::clone(&adapter.state_change);
        let worker_shutdown = Arc::new(AtomicBool::new(false));
        let worker_shutdown_for_thread = Arc::clone(&worker_shutdown);
        let worker = std::thread::spawn(move || {
            run_unix_lifecycle_wake_worker(
                wake_read,
                worker_shutdown_for_thread,
                worker_terminate,
                worker_reload,
                worker_state,
            );
        });

        let waiter = adapter.clone();
        let mut observed_generation = waiter.event_generation().expect("generation");
        let (tx, rx) = mpsc::sync_channel(1);
        let wait_join = std::thread::spawn(move || {
            waiter
                .wait_for_state_change(&mut observed_generation)
                .expect("wait for state change");
            tx.send(()).expect("notify waiter wake");
        });

        drop(wake_write);

        rx.recv_timeout(Duration::from_secs(5))
            .expect("EOF should wake lifecycle waiters");
        worker_shutdown.store(true, Ordering::SeqCst);
        worker.join().expect("join unix lifecycle worker");
        wait_join.join().expect("join waiter");
    }

    #[test]
    #[serial_test::serial(env)]
    fn unix_install_reuses_one_lifecycle_worker_until_shutdown() {
        let first = LifecycleControlSourceAdapter::install().expect("install first");
        let first_worker = worker_thread_id();
        let second = LifecycleControlSourceAdapter::install().expect("install second");
        assert_eq!(worker_thread_id(), first_worker);
        second
            .shutdown_worker_with_timeout()
            .expect("shutdown lifecycle worker");
        let third = LifecycleControlSourceAdapter::install().expect("install third");
        assert_ne!(worker_thread_id(), first_worker);
        third
            .shutdown_worker_with_timeout()
            .expect("shutdown lifecycle worker");
        let _ = first;
    }

    #[test]
    fn wait_for_state_change_timeout_reports_timeout_then_wake() {
        let adapter = LifecycleControlSourceAdapter::new_for_test();
        let mut observed_generation = adapter.event_generation().expect("generation");

        let timed_out = adapter
            .wait_for_state_change_timeout(&mut observed_generation, Duration::from_millis(10))
            .expect("timeout wait");
        assert!(
            !timed_out,
            "short timeout without a state change should report false"
        );

        adapter
            .notify_state_change()
            .expect("notify lifecycle state change");
        let changed = adapter
            .wait_for_state_change_timeout(&mut observed_generation, Duration::from_secs(1))
            .expect("wake wait");
        assert!(changed, "explicit notify should wake the timed wait");
    }
}

#[cfg(not(any(unix, windows)))]
fn install_platform_hooks(
    _terminate: &Arc<AtomicBool>,
    _reload: &Arc<AtomicBool>,
    _state_change: &Arc<LifecycleStateChange>,
) -> Result<LifecycleWorkerRegistration, AtmError> {
    // Supported lifecycle-control implementations currently exist only for Unix and
    // Windows; other targets keep the daemon-private contract buildable without
    // claiming unsupported OS signal semantics.
    let shutdown = Arc::new(AtomicBool::new(false));
    let join_handle = std::thread::spawn(|| {});
    Ok(LifecycleWorkerRegistration {
        shutdown,
        signal_ids: Vec::new(),
        join_handle,
    })
}
