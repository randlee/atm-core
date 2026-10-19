//! Live hook-chain tracking for daemon shutdown.
//!
//! Every hook chain runs under [`ChainTracker`]. Shutdown refuses new chains,
//! waits for the live ones up to a deadline, then aborts whatever is left.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use atm_core::api::RequestDeadline;
use atm_core::error::AtmError;
use tokio::sync::watch;
use tokio::task::{AbortHandle, JoinHandle};

struct Shared {
    live: watch::Sender<usize>,
    aborts: Mutex<HashMap<u64, AbortHandle>>,
}

pub struct ChainTracker {
    shared: Arc<Shared>,
    closing: AtomicBool,
    next: AtomicU64,
}

/// Removes its chain from the live set however the chain ends: normal
/// completion, panic, abort, or a never-polled future being dropped.
struct ChainGuard {
    shared: Arc<Shared>,
    id: u64,
}

impl Drop for ChainGuard {
    fn drop(&mut self) {
        self.shared
            .aborts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.id);
        self.shared.live.send_modify(|live| *live -= 1);
    }
}

impl Default for ChainTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ChainTracker {
    #[must_use]
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Shared {
                live: watch::channel(0).0,
                aborts: Mutex::new(HashMap::new()),
            }),
            closing: AtomicBool::new(false),
            next: AtomicU64::new(0),
        }
    }

    /// Starts one hook chain under the tracker.
    pub fn spawn<F, T>(&self, chain: F) -> Result<JoinHandle<T>, AtmError>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        if self.closing.load(Ordering::SeqCst) {
            return Err(AtmError::daemon_unavailable(
                "daemon shutting down; hook chain not started",
            ));
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.shared.live.send_modify(|live| *live += 1);
        let guard = ChainGuard {
            shared: Arc::clone(&self.shared),
            id,
        };
        let handle = tokio::spawn(async move {
            let _guard = guard;
            chain.await
        });
        // The guard removes its entry under this lock, so a chain that already
        // finished must not leave an abort handle behind.
        let mut aborts = self
            .shared
            .aborts
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !handle.is_finished() {
            aborts.insert(id, handle.abort_handle());
        }
        Ok(handle)
    }

    /// Refuses every chain started after this call.
    pub fn begin_shutdown(&self) {
        self.closing.store(true, Ordering::SeqCst);
    }

    /// Waits for live chains until `deadline`, then aborts the rest.
    /// Returns how many chains were still live at the deadline.
    pub async fn drain(&self, deadline: RequestDeadline) -> usize {
        self.begin_shutdown();
        let mut live = self.shared.live.subscribe();
        let idle = live.wait_for(|count| *count == 0);
        let remaining = deadline.remaining().unwrap_or(Duration::ZERO);
        if matches!(tokio::time::timeout(remaining, idle).await, Ok(Ok(_))) {
            return 0;
        }
        let aborted = *self.shared.live.borrow();
        let handles: Vec<AbortHandle> = self
            .shared
            .aborts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain()
            .map(|(_, handle)| handle)
            .collect();
        for handle in handles {
            handle.abort();
        }
        aborted
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use atm_core::api::RequestDeadline;
    use tokio::sync::oneshot;

    use super::ChainTracker;

    #[tokio::test(start_paused = true)]
    async fn drain_waits_for_a_held_chain_then_returns_zero() {
        let tracker = Arc::new(ChainTracker::new());
        let (release, held) = oneshot::channel::<()>();
        let chain = tracker
            .spawn(async move {
                let _ = held.await;
                7
            })
            .expect("chain starts");
        let drainer = tokio::spawn({
            let tracker = Arc::clone(&tracker);
            async move {
                tracker
                    .drain(RequestDeadline::after(Duration::from_secs(10)))
                    .await
            }
        });
        tokio::task::yield_now().await;
        release.send(()).expect("chain is still held");
        assert_eq!(drainer.await.expect("drain task"), 0);
        assert_eq!(chain.await.expect("chain completes"), 7);
    }

    #[tokio::test(start_paused = true)]
    async fn drain_aborts_at_the_deadline() {
        let tracker = ChainTracker::new();
        let (_hold, held) = oneshot::channel::<()>();
        let chain = tracker
            .spawn(async move {
                let _ = held.await;
            })
            .expect("chain starts");
        let aborted = tracker
            .drain(RequestDeadline::after(Duration::from_secs(1)))
            .await;
        assert_eq!(aborted, 1);
        assert!(chain.await.expect_err("chain is aborted").is_cancelled());
    }

    #[tokio::test]
    async fn new_chain_after_begin_shutdown_is_unavailable() {
        let tracker = ChainTracker::new();
        tracker.begin_shutdown();
        let error = tracker
            .spawn(async {})
            .map(|_| ())
            .expect_err("closed tracker refuses chains");
        assert!(error.message().contains("daemon shutting down"));
    }
}
