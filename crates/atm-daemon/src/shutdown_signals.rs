#[cfg(unix)]
use std::sync::atomic::AtomicBool;
#[cfg(unix)]
use std::sync::{Arc, Mutex};

#[cfg(unix)]
use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
#[cfg(unix)]
use signal_hook::flag;

#[cfg(unix)]
use atm_core::error::AtmError;

#[cfg(unix)]
#[derive(Debug)]
pub(crate) struct DaemonShutdownSignals {
    pub(crate) terminate: Arc<AtomicBool>,
    pub(crate) reload: Arc<AtomicBool>,
}

#[cfg(unix)]
#[derive(Debug)]
struct SharedDaemonShutdownSignals {
    terminate: Arc<AtomicBool>,
    reload: Arc<AtomicBool>,
}

#[cfg(unix)]
impl DaemonShutdownSignals {
    pub(crate) fn install() -> Result<Self, AtmError> {
        static SIGNALS: Mutex<Option<SharedDaemonShutdownSignals>> = Mutex::new(None);
        static INSTALL_LOCK: Mutex<()> = Mutex::new(());
        // Signal-hook registration is process-global and cannot be cleanly
        // unregistered in tests, so this shared slot owns the registered flags
        // for the lifetime of the process while the mutex serializes setup.
        let _guard = INSTALL_LOCK
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("daemon signal install lock poisoned"))?;
        let mut shared = SIGNALS.lock().map_err(|_| {
            AtmError::daemon_unavailable("daemon shutdown signal state lock poisoned")
        })?;
        if shared.is_none() {
            let terminate = Arc::new(AtomicBool::new(false));
            let reload = Arc::new(AtomicBool::new(false));
            for signal in [SIGINT, SIGTERM] {
                flag::register(signal, Arc::clone(&terminate)).map_err(|source| {
                    AtmError::daemon_unavailable("failed to install daemon shutdown signal handler")
                        .with_source(source)
                })?;
            }
            flag::register(SIGHUP, Arc::clone(&reload)).map_err(|source| {
                AtmError::daemon_unavailable("failed to install daemon reload signal handler")
                    .with_source(source)
            })?;
            *shared = Some(SharedDaemonShutdownSignals { terminate, reload });
        }
        let shared = shared.as_ref().ok_or_else(|| {
            AtmError::daemon_unavailable("daemon shutdown signals were not initialized")
        })?;
        Ok(Self {
            terminate: Arc::clone(&shared.terminate),
            reload: Arc::clone(&shared.reload),
        })
    }

    #[cfg(test)]
    pub(crate) fn new_for_test() -> Self {
        Self {
            terminate: Arc::new(AtomicBool::new(false)),
            reload: Arc::new(AtomicBool::new(false)),
        }
    }
}
