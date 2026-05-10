use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

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

    #[cfg_attr(any(windows, not(test)), allow(dead_code))]
    pub(crate) fn wait_until_tripped(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.is_tripped() {
                return true;
            }
            std::thread::yield_now();
        }
        self.is_tripped()
    }
}

#[cfg(test)]
mod tests {
    use super::ShutdownBeacon;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn wait_until_tripped_returns_true_after_trip() {
        let beacon = Arc::new(ShutdownBeacon::default());
        let waiter = Arc::clone(&beacon);
        let join = std::thread::spawn(move || {
            assert!(waiter.wait_until_tripped(Duration::from_secs(1)));
        });

        std::thread::yield_now();
        beacon.trip();

        join.join().expect("join shutdown beacon waiter");
    }
}
