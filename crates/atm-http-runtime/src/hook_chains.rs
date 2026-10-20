//! Live hook-chain tracking for daemon shutdown.
//!
//! Every hook chain runs under [`ChainTracker`]. Shutdown refuses new chains,
//! waits for the live ones up to a deadline, then aborts whatever is left.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use atm_core::api::RequestDeadline;
use atm_core::error::AtmError;
use tokio::sync::watch;
use tokio::task::{AbortHandle, JoinHandle};

struct Shared {
    live: watch::Sender<usize>,
    /// Serializes admission/registration with `ChainGuard::drop` removing one
    /// handle and `drain` taking all remaining handles. All lock scopes are
    /// synchronous and end before `drain` awaits. If a panic poisons this lock,
    /// callers recover the inner map so shutdown can still abort handles that
    /// remain in it; this is best-effort cleanup, not a guarantee that a
    /// panicking critical section left the map complete.
    aborts: Mutex<HashMap<u64, AbortHandle>>,
}

pub struct ChainTracker {
    shared: Arc<Shared>,
    /// Written only under the `aborts` lock, so admission and registration
    /// observe shutdown consistently.
    closing: AtomicBool,
    /// Set under the `aborts` lock when `drain` takes the handles; a chain that
    /// registers after that point is aborted by its own registration.
    collected: AtomicBool,
    /// Unique map keys only; `admit` increments this while holding `aborts`,
    /// so the counter publishes no state and needs only relaxed ordering.
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

/// A live slot reserved by [`ChainTracker::admit`]. Dropping it unstarted
/// releases the slot.
struct Admission {
    guard: ChainGuard,
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
            collected: AtomicBool::new(false),
            next: AtomicU64::new(0),
        }
    }

    /// Starts one hook chain under the tracker.
    pub fn spawn<F, T>(&self, chain: F) -> Result<JoinHandle<T>, AtmError>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let admission = self.admit()?;
        Ok(self.start(admission, chain))
    }

    /// Refuses every chain started after this call.
    pub fn begin_shutdown(&self) {
        let _aborts = self.lock_aborts();
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
        let handles: Vec<AbortHandle> = {
            let mut aborts = self.lock_aborts();
            self.collected.store(true, Ordering::SeqCst);
            aborts.drain().map(|(_, handle)| handle).collect()
        };
        // Aborted outside the lock: an abort can drop a chain's guard inline,
        // and the guard takes this same lock.
        for handle in handles {
            handle.abort();
        }
        aborted
    }

    fn lock_aborts(&self) -> MutexGuard<'_, HashMap<u64, AbortHandle>> {
        self.shared
            .aborts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Reserves a live slot unless shutdown began. The closing check and the
    /// live increment share the `aborts` lock with `begin_shutdown`, so `drain`
    /// either counts this chain as live or refuses it.
    fn admit(&self) -> Result<Admission, AtmError> {
        let _aborts = self.lock_aborts();
        if self.closing.load(Ordering::SeqCst) {
            return Err(AtmError::daemon_unavailable(
                "daemon shutting down; hook chain not started",
            ));
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.shared.live.send_modify(|live| *live += 1);
        Ok(Admission {
            guard: ChainGuard {
                shared: Arc::clone(&self.shared),
                id,
            },
        })
    }

    /// Spawns an admitted chain and registers its abort handle. When `drain`
    /// has already taken the handles, the chain is aborted here instead, since
    /// no later collection will see it.
    fn start<F, T>(&self, admission: Admission, chain: F) -> JoinHandle<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let id = admission.guard.id;
        let handle = tokio::spawn(async move {
            let _guard = admission.guard;
            chain.await
        });
        let abort = handle.abort_handle();
        let collected = {
            let mut aborts = self.lock_aborts();
            if self.collected.load(Ordering::SeqCst) {
                true
            } else {
                // A chain that already finished left no entry for the guard to
                // remove; inserting now would leak one.
                if !handle.is_finished() {
                    aborts.insert(id, abort.clone());
                }
                false
            }
        };
        if collected {
            abort.abort();
        }
        handle
    }
}

#[cfg(test)]
mod tests {
    use std::pin::pin;
    use std::sync::Arc;
    use std::task::{Context, Waker};
    use std::time::Duration;

    use atm_core::api::RequestDeadline;
    use atm_core::boundary::{
        DummyHookRunner, RecordingHookActionDispatcher, RecordingTaskCommands, TaskAssign,
        TaskCommands,
    };
    use atm_core::hook::HookReply;
    use atm_runtime_test_support::InMemoryHookRegistrationStore;
    use tokio::sync::{Notify, oneshot, watch};

    use super::ChainTracker;
    use crate::hook_filter::test_support::{
        assign_request, ctx, deadline, expect_err, handled_action, register, reply,
    };
    use crate::hook_filter::{ActiveHookSnapshot, HookedTaskCommands};

    /// A `task.assign` filter whose one hook waits on `hold` before it replies.
    /// `entered` fires once a chain is live inside that wait.
    struct Held {
        commands: Arc<HookedTaskCommands<RecordingHookActionDispatcher>>,
        chains: Arc<ChainTracker>,
        actions: Arc<RecordingHookActionDispatcher>,
        runner: Arc<DummyHookRunner>,
    }

    fn held_hook(hold: &watch::Receiver<bool>, entered: &Arc<Notify>) -> Held {
        let store = Arc::new(InMemoryHookRegistrationStore::new());
        register(&store, "task.assign", "bk", 1);
        let runner = Arc::new(DummyHookRunner::default());
        {
            let hold = hold.clone();
            let entered = Arc::clone(entered);
            *runner.before_reply.lock().expect("before reply") = Some(Arc::new(move |_index| {
                let entered = Arc::clone(&entered);
                let mut hold = hold.clone();
                Box::pin(async move {
                    entered.notify_one();
                    hold.wait_for(|released| *released)
                        .await
                        .expect("hold sender lives");
                })
            }));
        }
        let actions = Arc::new(RecordingHookActionDispatcher::default());
        let chains = Arc::new(ChainTracker::new());
        let snapshot = Arc::new(ActiveHookSnapshot::load(store).expect("snapshot"));
        let commands = Arc::new(HookedTaskCommands::new(
            Arc::new(RecordingTaskCommands::default()),
            Arc::clone(&actions),
            snapshot,
            runner.clone(),
            Arc::clone(&chains),
            crate::hook_filter::test_support::bridge(),
        ));
        Held {
            commands,
            chains,
            actions,
            runner,
        }
    }

    /// Starts one held `task.assign` through the filter and returns its caller.
    fn start_held_assign(
        held: &Held,
    ) -> tokio::task::JoinHandle<Result<atm_core::api::ApiResponse, atm_core::error::AtmError>>
    {
        let request = assign_request(false);
        reply(
            &held.runner,
            HookReply::Handled(vec![handled_action("task.assign", &request)]),
        );
        let commands = Arc::clone(&held.commands);
        tokio::spawn(async move {
            commands
                .assign(TaskAssign(Box::new(request)), &ctx(deadline()))
                .await
        })
    }

    #[tokio::test(start_paused = true)]
    async fn drain_waits_for_a_held_chain_then_returns_zero() {
        let (release, hold) = watch::channel(false);
        let entered = Arc::new(Notify::new());
        let held = held_hook(&hold, &entered);
        let caller = start_held_assign(&held);
        entered.notified().await;

        // One poll while the chain is held: drain must not finish yet.
        let mut drain = pin!(
            held.chains
                .drain(RequestDeadline::after(Duration::from_secs(10)))
        );
        let mut cx = Context::from_waker(Waker::noop());
        assert!(drain.as_mut().poll(&mut cx).is_pending());

        release.send_replace(true);
        assert_eq!(drain.await, 0);
        caller
            .await
            .expect("caller task")
            .expect("assign completes");
        assert_eq!(held.actions.calls.lock().expect("calls").len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn drain_aborts_at_the_deadline() {
        let (_release, hold) = watch::channel(false);
        let entered = Arc::new(Notify::new());
        let held = held_hook(&hold, &entered);
        let caller = start_held_assign(&held);
        entered.notified().await;

        let aborted = held
            .chains
            .drain(RequestDeadline::after(Duration::from_secs(1)))
            .await;
        assert_eq!(aborted, 1);
        let error = expect_err(caller.await.expect("caller task"));
        assert!(
            error
                .message()
                .contains("hook chain was cancelled while the daemon was shutting down"),
            "unexpected error: {}",
            error.message()
        );
        assert!(
            error
                .message()
                .contains("Recovery: Retry the task after the daemon is serving again; if cancellation repeats, inspect daemon shutdown logs."),
            "missing recovery guidance: {}",
            error.message()
        );
        assert!(
            error
                .cause()
                .is_some_and(|cause| cause.contains("cancelled")),
            "missing cancellation cause: {:?}",
            error.cause()
        );
        assert!(held.actions.calls.lock().expect("calls").is_empty());
    }

    /// Interleaves an admitted chain with `begin_shutdown` and `drain`: the
    /// chain is admitted, drain's deadline passes before the chain registers its
    /// handle, and the registration must then abort the chain itself.
    #[tokio::test(start_paused = true)]
    async fn chain_registered_after_drain_collected_is_aborted() {
        let tracker = ChainTracker::new();
        let admission = tracker.admit().expect("admitted before shutdown");
        let aborted = tracker
            .drain(RequestDeadline::after(Duration::from_secs(1)))
            .await;
        assert_eq!(aborted, 1, "the admitted chain counts as live");
        let (_hold, held) = oneshot::channel::<()>();
        let chain = tracker.start(admission, async move {
            let _ = held.await;
        });
        assert!(
            chain
                .await
                .expect_err("registration after collection aborts the chain")
                .is_cancelled()
        );
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
