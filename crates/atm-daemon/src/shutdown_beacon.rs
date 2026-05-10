use std::sync::Condvar;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Default)]
pub(crate) struct ShutdownBeacon {
    // This bit is read by accept, lifecycle, and serve-loop threads, so it stays atomic while the
    // companion condvar provides immediate wakeups for blocking waiters.
    tripped: AtomicBool,
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
}
