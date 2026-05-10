use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Default)]
pub(crate) struct ShutdownBeacon {
    // This bit is polled by accept, lifecycle, and serve-loop threads. No shutdown-beacon waiter
    // blocks on a companion condition variable, so an atomic flag is sufficient here.
    tripped: AtomicBool,
}

impl ShutdownBeacon {
    pub(crate) fn trip(&self) {
        self.tripped.store(true, Ordering::SeqCst);
    }

    pub(crate) fn is_tripped(&self) -> bool {
        self.tripped.load(Ordering::SeqCst)
    }
}
