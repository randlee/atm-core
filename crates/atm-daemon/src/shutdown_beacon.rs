use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use atm_core::error::AtmError;

#[derive(Debug, Default)]
pub(crate) struct ShutdownBeacon {
    // This bit is read by accept, lifecycle, and serve-loop threads, so it stays atomic while the
    // companion condvar provides immediate wakeups for blocking waiters.
    tripped: AtomicBool,
    state: Mutex<()>,
    wake: Condvar,
}

impl ShutdownBeacon {
    pub(crate) fn trip(&self) {
        self.tripped.store(true, Ordering::SeqCst);
        self.wake.notify_all();
    }

    pub(crate) fn is_tripped(&self) -> bool {
        self.tripped.load(Ordering::SeqCst)
    }

    #[allow(dead_code)]
    pub(crate) fn wait_for_trip_timeout(&self, timeout: Duration) -> Result<bool, AtmError> {
        if self.is_tripped() {
            return Ok(true);
        }
        let state = self.state.lock().map_err(|_| {
            AtmError::daemon_lifecycle_wedge("daemon shutdown beacon lock poisoned").with_recovery(
                "Restart the daemon; transport shutdown coordination can no longer wake blocked lifecycle waiters safely.",
            )
        })?;
        if self.is_tripped() {
            return Ok(true);
        }
        let (_state, wait_result) = self.wake.wait_timeout(state, timeout).map_err(|_| {
            AtmError::daemon_lifecycle_wedge("daemon shutdown beacon lock poisoned").with_recovery(
                "Restart the daemon; transport shutdown coordination can no longer wake blocked lifecycle waiters safely.",
            )
        })?;
        Ok(!wait_result.timed_out() && self.is_tripped())
    }
}
