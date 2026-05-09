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
    use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
    use signal_hook::iterator::Signals;

    let mut signals = Signals::new([SIGINT, SIGTERM, SIGHUP]).map_err(|source| {
        AtmError::daemon_unavailable("failed to install daemon lifecycle signal handlers")
            .with_source(source)
    })?;
    let terminate = Arc::clone(terminate);
    let reload = Arc::clone(reload);
    let state_change = Arc::clone(state_change);
    std::thread::Builder::new()
        .name("atm-daemon-lifecycle-unix".to_string())
        .spawn(move || {
            for signal in signals.forever() {
                match signal {
                    SIGINT | SIGTERM => terminate.store(true, Ordering::SeqCst),
                    SIGHUP => reload.store(true, Ordering::SeqCst),
                    _ => continue,
                }
                let _ = state_change.notify();
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
    _reload: &Arc<AtomicBool>,
    state_change: &Arc<LifecycleStateChange>,
) -> Result<(), AtmError> {
    use signal_hook::consts::signal::SIGINT;
    use signal_hook::iterator::Signals;

    let mut signals = Signals::new([SIGINT]).map_err(|source| {
        AtmError::daemon_unavailable("failed to install daemon lifecycle signal handlers")
            .with_source(source)
    })?;
    let terminate = Arc::clone(terminate);
    let state_change = Arc::clone(state_change);
    std::thread::Builder::new()
        .name("atm-daemon-lifecycle-windows".to_string())
        .spawn(move || {
            for signal in signals.forever() {
                match signal {
                    SIGINT => terminate.store(true, Ordering::SeqCst),
                    _ => continue,
                }
                let _ = state_change.notify();
            }
        })
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to spawn daemon lifecycle signal worker")
                .with_source(source)
        })?;
    Ok(())
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
