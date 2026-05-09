use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use atm_core::error::AtmError;

#[derive(Debug, Clone)]
pub(crate) struct LifecycleControlSourceAdapter {
    pub(crate) terminate: Arc<AtomicBool>,
    pub(crate) reload: Arc<AtomicBool>,
}

#[derive(Debug)]
struct SharedLifecycleControlState {
    terminate: Arc<AtomicBool>,
    reload: Arc<AtomicBool>,
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
            install_platform_hooks(&terminate, &reload)?;
            *shared = Some(SharedLifecycleControlState { terminate, reload });
        }
        let shared = shared.as_ref().ok_or_else(|| {
            AtmError::daemon_unavailable("daemon lifecycle control source was not initialized")
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

    pub(crate) fn terminate_requested(&self) -> bool {
        self.terminate.load(Ordering::SeqCst)
    }

    pub(crate) fn take_reload_requested(&self) -> bool {
        self.reload.swap(false, Ordering::SeqCst)
    }

    pub(crate) fn terminate_flag(&self) -> Option<Arc<AtomicBool>> {
        Some(Arc::clone(&self.terminate))
    }
}

#[cfg(unix)]
fn install_platform_hooks(
    terminate: &Arc<AtomicBool>,
    reload: &Arc<AtomicBool>,
) -> Result<(), AtmError> {
    use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
    use signal_hook::flag;

    for signal in [SIGINT, SIGTERM] {
        flag::register(signal, Arc::clone(terminate)).map_err(|source| {
            AtmError::daemon_unavailable("failed to install daemon shutdown signal handler")
                .with_source(source)
        })?;
    }
    flag::register(SIGHUP, Arc::clone(reload)).map_err(|source| {
        AtmError::daemon_unavailable("failed to install daemon reload signal handler")
            .with_source(source)
    })?;
    Ok(())
}

#[cfg(windows)]
fn install_platform_hooks(
    terminate: &Arc<AtomicBool>,
    reload: &Arc<AtomicBool>,
) -> Result<(), AtmError> {
    use signal_hook::consts::signal::{SIGBREAK, SIGINT, SIGTERM};
    use signal_hook::flag;

    for signal in [SIGINT, SIGTERM] {
        flag::register(signal, Arc::clone(terminate)).map_err(|source| {
            AtmError::daemon_unavailable("failed to install daemon shutdown signal handler")
                .with_source(source)
        })?;
    }
    flag::register(SIGBREAK, Arc::clone(reload)).map_err(|source| {
        AtmError::daemon_unavailable("failed to install daemon reload signal handler")
            .with_source(source)
    })?;
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn install_platform_hooks(
    _terminate: &Arc<AtomicBool>,
    _reload: &Arc<AtomicBool>,
) -> Result<(), AtmError> {
    Ok(())
}
