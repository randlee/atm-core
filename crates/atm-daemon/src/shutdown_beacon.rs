use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(test)]
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

    #[cfg(test)]
    pub(crate) fn wait_until_tripped(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.is_tripped() {
                return true;
            }
            std::thread::park_timeout(Duration::from_millis(5));
        }
        self.is_tripped()
    }
}

#[cfg(test)]
mod tests {
    use super::ShutdownBeacon;
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn wait_until_tripped_returns_true_after_trip() {
        let beacon = Arc::new(ShutdownBeacon::default());
        let waiter = Arc::clone(&beacon);
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let join = std::thread::spawn(move || {
            entered_tx
                .send(())
                .expect("notify waiter entered polling loop");
            assert!(waiter.wait_until_tripped(Duration::from_secs(1)));
        });

        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("waiter entered polling loop");
        beacon.trip();

        join.join().expect("join shutdown beacon waiter");
    }
}
