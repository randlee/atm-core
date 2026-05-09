use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use atm_core::error::AtmError;

#[derive(Debug, Clone)]
pub(crate) struct LifecycleControlSourceAdapter {
    terminate: Arc<AtomicBool>,
    reload: Arc<AtomicBool>,
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
    generation: Mutex<u64>,
    wake: Condvar,
}

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

    fn snapshot(&self) -> Result<u64, AtmError> {
        self.generation
            .lock()
            .map(|generation| *generation)
            .map_err(|_| {
                AtmError::daemon_unavailable("daemon lifecycle state-change lock poisoned")
            })
    }

    fn wait_for_change(&self, observed_generation: &mut u64) -> Result<(), AtmError> {
        let mut generation = self.generation.lock().map_err(|_| {
            AtmError::daemon_unavailable("daemon lifecycle state-change lock poisoned")
        })?;
        while *generation == *observed_generation {
            generation = self.wake.wait(generation).map_err(|_| {
                AtmError::daemon_unavailable("daemon lifecycle state-change lock poisoned")
            })?;
        }
        *observed_generation = *generation;
        Ok(())
    }
}

impl LifecycleControlSourceAdapter {
    pub(crate) fn install() -> Result<Self, AtmError> {
        static SHARED: Mutex<Option<SharedLifecycleControlState>> = Mutex::new(None);
        static INSTALL_LOCK: Mutex<()> = Mutex::new(());

        let _guard = INSTALL_LOCK
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("daemon lifecycle install lock poisoned"))?;
        let mut shared = SHARED.lock().map_err(|_| {
            AtmError::daemon_unavailable("daemon lifecycle control state lock poisoned")
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

    pub(crate) fn terminate_requested(&self) -> bool {
        self.terminate.load(Ordering::SeqCst)
    }

    pub(crate) fn take_reload_requested(&self) -> bool {
        self.reload.swap(false, Ordering::SeqCst)
    }

    pub(crate) fn event_generation(&self) -> Result<u64, AtmError> {
        self.state_change.snapshot()
    }

    pub(crate) fn wait_for_state_change(
        &self,
        observed_generation: &mut u64,
    ) -> Result<(), AtmError> {
        self.state_change.wait_for_change(observed_generation)
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
fn install_platform_hooks(
    terminate: &Arc<AtomicBool>,
    reload: &Arc<AtomicBool>,
    state_change: &Arc<LifecycleStateChange>,
) -> Result<(), AtmError> {
    use std::io::Read;
    use std::os::unix::net::UnixStream;

    use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
    use signal_hook::flag as signal_flag;
    use signal_hook::low_level::pipe as signal_pipe;

    let (mut wake_read, wake_write) = UnixStream::pair().map_err(|source| {
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
            let mut wake_buffer = [0_u8; 32];
            loop {
                match wake_read.read(&mut wake_buffer) {
                    Ok(0) => return,
                    Ok(_) => {
                        if terminate.load(Ordering::SeqCst) || reload.load(Ordering::SeqCst) {
                            let _ = state_change.notify();
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => return,
                }
            }
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
