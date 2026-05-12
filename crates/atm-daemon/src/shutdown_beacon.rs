use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(test)]
use std::sync::{Condvar, Mutex};
#[cfg(test)]
use std::time::Duration;

#[derive(Debug)]
pub(crate) struct ShutdownBeacon {
    // This bit is polled by accept, lifecycle, and serve-loop threads, so the hot path remains
    // lock-free even though tests also mirror shutdown through a condvar-backed waiter.
    tripped: AtomicBool,
    #[cfg(test)]
    wait_state: Mutex<bool>,
    #[cfg(test)]
    wait_condvar: Condvar,
}

impl Default for ShutdownBeacon {
    fn default() -> Self {
        Self {
            tripped: AtomicBool::new(false),
            #[cfg(test)]
            wait_state: Mutex::new(false),
            #[cfg(test)]
            wait_condvar: Condvar::new(),
        }
    }
}

impl ShutdownBeacon {
    pub(crate) fn trip(&self) {
        self.tripped.store(true, Ordering::SeqCst);
        #[cfg(test)]
        {
            let mut guard = self
                .wait_state
                .lock()
                .expect("lock shutdown beacon wait state");
            *guard = true;
            self.wait_condvar.notify_all();
        }
    }

    pub(crate) fn is_tripped(&self) -> bool {
        self.tripped.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    pub(crate) fn wait_until_tripped(&self, timeout: Duration) -> bool {
        if self.is_tripped() {
            return true;
        }
        let guard = self
            .wait_state
            .lock()
            .expect("lock shutdown beacon wait state");
        let (guard, _) = self
            .wait_condvar
            .wait_timeout_while(guard, timeout, |tripped| !*tripped)
            .expect("wait on shutdown beacon condvar");
        *guard || self.is_tripped()
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
            assert!(waiter.wait_until_tripped(Duration::from_secs(5)));
        });

        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("waiter entered polling loop");
        beacon.trip();

        join.join().expect("join shutdown beacon waiter");
    }
}
