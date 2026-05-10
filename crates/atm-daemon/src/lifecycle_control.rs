use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use atm_core::error::AtmError;

#[derive(Debug, Clone)]
pub(crate) struct LifecycleControlSourceAdapter {
    terminate: Arc<AtomicBool>,
    #[cfg_attr(windows, allow(dead_code))]
    reload: Arc<AtomicBool>,
    #[cfg_attr(windows, allow(dead_code))]
    state_change: Arc<LifecycleStateChange>,
}

#[derive(Debug)]
struct SharedLifecycleControlState {
    terminate: Arc<AtomicBool>,
    reload: Arc<AtomicBool>,
    state_change: Arc<LifecycleStateChange>,
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
                .with_recovery("Restart the daemon; lifecycle wake state is no longer trustworthy.")
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
                    .with_recovery(
                        "Restart the daemon; lifecycle wake state is no longer trustworthy.",
                    )
            })
    }

    fn wait_for_change_timeout(
        &self,
        observed_generation: &mut u64,
        timeout: std::time::Duration,
    ) -> Result<bool, AtmError> {
        let mut generation = self.generation.lock().map_err(|_| {
            AtmError::daemon_unavailable("daemon lifecycle state-change lock poisoned")
                .with_recovery("Restart the daemon; lifecycle wake state is no longer trustworthy.")
        })?;
        while *generation == *observed_generation {
            let (updated_generation, wait_result) = self
                .wake
                .wait_timeout_while(generation, timeout, |current_generation| {
                    *current_generation == *observed_generation
                })
                .map_err(|_| {
                    AtmError::daemon_unavailable("daemon lifecycle state-change lock poisoned")
                        .with_recovery(
                            "Restart the daemon; lifecycle wake state is no longer trustworthy.",
                        )
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
        const LIFECYCLE_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
        while !self.wait_for_change_timeout(observed_generation, LIFECYCLE_WAIT_TIMEOUT)? {}
        Ok(())
    }
}

impl LifecycleControlSourceAdapter {
    pub(crate) fn install() -> Result<Self, AtmError> {
        static SHARED: Mutex<Option<SharedLifecycleControlState>> = Mutex::new(None);
        static INSTALL_LOCK: Mutex<()> = Mutex::new(());

        let _guard = INSTALL_LOCK
            .lock()
            .map_err(|_| {
                AtmError::daemon_unavailable("daemon lifecycle install lock poisoned")
                    .with_recovery(
                        "Restart the daemon; lifecycle hook installation cannot complete after the poisoned lock.",
                    )
            })?;
        let mut shared = SHARED.lock().map_err(|_| {
            AtmError::daemon_unavailable("daemon lifecycle control state lock poisoned")
                .with_recovery(
                    "Restart the daemon; shared lifecycle control state may be inconsistent.",
                )
        })?;
        if shared.is_none() {
            let terminate = Arc::new(AtomicBool::new(false));
            let reload = Arc::new(AtomicBool::new(false));
            let state_change = Arc::new(LifecycleStateChange::new());
            install_platform_hooks(&terminate, &reload, &state_change)?;
            *shared = Some(SharedLifecycleControlState {
                terminate,
                reload,
                state_change,
            });
        }
        let shared = shared.as_ref().ok_or_else(|| {
            AtmError::daemon_unavailable("daemon lifecycle control source was not initialized")
        })?;
        Ok(Self {
            terminate: Arc::clone(&shared.terminate),
            reload: Arc::clone(&shared.reload),
            state_change: Arc::clone(&shared.state_change),
        })
    }

    #[cfg(test)]
    pub(crate) fn new_for_test() -> Self {
        let state_change = Arc::new(LifecycleStateChange::new());
        Self {
            terminate: Arc::new(AtomicBool::new(false)),
            reload: Arc::new(AtomicBool::new(false)),
            state_change,
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

    #[cfg_attr(any(windows, not(test)), allow(dead_code))]
    pub(crate) fn wait_for_state_change(
        &self,
        observed_generation: &mut u64,
    ) -> Result<(), AtmError> {
        self.state_change.wait_for_change(observed_generation)
    }

    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) fn wait_for_state_change_timeout(
        &self,
        observed_generation: &mut u64,
        timeout: std::time::Duration,
    ) -> Result<bool, AtmError> {
        self.state_change
            .wait_for_change_timeout(observed_generation, timeout)
    }

    pub(crate) fn terminate_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.terminate)
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
    terminate: Arc<AtomicBool>,
    reload: Arc<AtomicBool>,
    state_change: Arc<LifecycleStateChange>,
) {
    use std::io::Read;

    let mut wake_buffer = [0_u8; 32];
    loop {
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
) -> Result<(), AtmError> {
    use std::os::unix::net::UnixStream;

    use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
    use signal_hook::flag as signal_flag;
    use signal_hook::low_level::pipe as signal_pipe;

    let (wake_read, wake_write) = UnixStream::pair().map_err(|source| {
        AtmError::daemon_unavailable("failed to create daemon lifecycle wake pipe")
            .with_source(source)
    })?;
    signal_flag::register(SIGINT, Arc::clone(terminate)).map_err(|source| {
        AtmError::daemon_unavailable("failed to install daemon lifecycle signal handlers")
            .with_source(source)
    })?;
    signal_flag::register(SIGTERM, Arc::clone(terminate)).map_err(|source| {
        AtmError::daemon_unavailable("failed to install daemon lifecycle signal handlers")
            .with_source(source)
    })?;
    signal_flag::register(SIGHUP, Arc::clone(reload)).map_err(|source| {
        AtmError::daemon_unavailable("failed to install daemon lifecycle signal handlers")
            .with_source(source)
    })?;
    signal_pipe::register(
        SIGINT,
        wake_write.try_clone().map_err(|source| {
            AtmError::daemon_unavailable("failed to clone daemon lifecycle wake pipe")
                .with_source(source)
        })?,
    )
    .map_err(|source| {
        AtmError::daemon_unavailable("failed to install daemon lifecycle signal handlers")
            .with_source(source)
    })?;
    signal_pipe::register(
        SIGTERM,
        wake_write.try_clone().map_err(|source| {
            AtmError::daemon_unavailable("failed to clone daemon lifecycle wake pipe")
                .with_source(source)
        })?,
    )
    .map_err(|source| {
        AtmError::daemon_unavailable("failed to install daemon lifecycle signal handlers")
            .with_source(source)
    })?;
    signal_pipe::register(SIGHUP, wake_write).map_err(|source| {
        AtmError::daemon_unavailable("failed to install daemon lifecycle signal handlers")
            .with_source(source)
    })?;
    let terminate = Arc::clone(terminate);
    let reload = Arc::clone(reload);
    let state_change = Arc::clone(state_change);
    std::thread::Builder::new()
        .name("atm-daemon-lifecycle-unix".to_string())
        .spawn(move || {
            run_unix_lifecycle_wake_worker(wake_read, terminate, reload, state_change);
        })
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to spawn daemon lifecycle signal worker")
                .with_source(source)
        })?;
    Ok(())
}

#[cfg(windows)]
fn install_platform_hooks(
    terminate: &Arc<AtomicBool>,
    reload: &Arc<AtomicBool>,
    state_change: &Arc<LifecycleStateChange>,
) -> Result<(), AtmError> {
    use signal_hook::consts::{SIGBREAK, SIGINT, SIGTERM};
    use signal_hook::flag as signal_flag;

    signal_flag::register(SIGINT, Arc::clone(terminate)).map_err(|source| {
        AtmError::daemon_unavailable("failed to install daemon lifecycle signal handlers")
            .with_source(source)
    })?;
    signal_flag::register(SIGTERM, Arc::clone(terminate)).map_err(|source| {
        AtmError::daemon_unavailable("failed to install daemon lifecycle signal handlers")
            .with_source(source)
    })?;
    signal_flag::register(SIGBREAK, Arc::clone(reload)).map_err(|source| {
        AtmError::daemon_unavailable("failed to install daemon lifecycle signal handlers")
            .with_source(source)
    })?;

    let terminate = Arc::clone(terminate);
    let reload = Arc::clone(reload);
    let state_change = Arc::clone(state_change);
    std::thread::Builder::new()
        .name("atm-daemon-lifecycle-windows".to_string())
        .spawn(move || {
            let mut observed_terminate = terminate.load(Ordering::SeqCst);
            let mut observed_reload = reload.load(Ordering::SeqCst);
            loop {
                if terminate.load(Ordering::SeqCst) {
                    let _ = state_change.notify();
                    return;
                }
                let terminate_now = terminate.load(Ordering::SeqCst);
                let reload_now = reload.load(Ordering::SeqCst);
                if terminate_now != observed_terminate || reload_now != observed_reload {
                    observed_terminate = terminate_now;
                    observed_reload = reload_now;
                    let _ = state_change.notify();
                }
                // `signal_hook::flag` does not expose a blocking cross-platform wake primitive on
                // Windows, so the lifecycle worker uses one bounded polling exception that Phase S
                // documents explicitly in plan-phase-S.md §4.1.
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        })
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to spawn daemon lifecycle signal worker")
                .with_source(source)
        })?;
    Ok(())
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::LifecycleControlSourceAdapter;
    use serial_test::serial;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    #[serial]
    fn windows_reload_flag_is_shared_across_install_calls() {
        let first = LifecycleControlSourceAdapter::install().expect("install first");
        first.set_terminate_for_test(false);
        first.set_reload_for_test(false);

        let second = LifecycleControlSourceAdapter::install().expect("install second");
        first.set_reload_for_test(true);

        assert!(second.take_reload_requested());
        assert!(!second.terminate_requested());
    }

    #[test]
    #[serial]
    fn windows_terminate_flag_is_shared_across_install_calls() {
        let first = LifecycleControlSourceAdapter::install().expect("install first");
        first.set_terminate_for_test(false);
        first.set_reload_for_test(false);

        let second = LifecycleControlSourceAdapter::install().expect("install second");
        first.set_terminate_for_test(true);

        assert!(second.terminate_requested());
    }

    #[test]
    #[serial]
    fn windows_terminate_request_wakes_waiters() {
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
            rx.recv_timeout(Duration::from_secs(1))
                .expect("waiter wake result"),
            "terminate wake should set the terminate flag"
        );
        join.join().expect("join waiter");
    }
}

#[cfg(all(test, unix))]
mod unix_tests {
    use super::{LifecycleControlSourceAdapter, run_unix_lifecycle_wake_worker};
    use std::os::unix::net::UnixStream;
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn unix_eof_wake_notifies_waiters() {
        let adapter = LifecycleControlSourceAdapter::new_for_test();
        let (wake_read, wake_write) = UnixStream::pair().expect("unix pair");
        let worker_terminate = adapter.terminate_flag();
        let worker_reload = Arc::clone(&adapter.reload);
        let worker_state = Arc::clone(&adapter.state_change);
        let worker = std::thread::spawn(move || {
            run_unix_lifecycle_wake_worker(
                wake_read,
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

        rx.recv_timeout(Duration::from_secs(1))
            .expect("EOF should wake lifecycle waiters");
        worker.join().expect("join unix lifecycle worker");
        wait_join.join().expect("join waiter");
    }
}

#[cfg(not(any(unix, windows)))]
fn install_platform_hooks(
    _terminate: &Arc<AtomicBool>,
    _reload: &Arc<AtomicBool>,
    _state_change: &Arc<LifecycleStateChange>,
) -> Result<(), AtmError> {
    // Supported lifecycle-control implementations currently exist only for Unix and
    // Windows; other targets keep the daemon-private contract buildable without
    // claiming unsupported OS signal semantics.
    Ok(())
}
