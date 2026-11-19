//! Test-only synchronization seams for Herdr queue wake cancellation tests.

use std::sync::{Arc, Condvar, Mutex};

use tokio::sync::Notify;

use crate::herdr_queue_wake::HerdrQueueWakePump;

pub(crate) type Gate = (Arc<Notify>, Arc<Notify>);

#[derive(Default)]
struct BlockingGateState {
    entered: bool,
    released: bool,
}

/// A test-owned gate for synchronous work submitted to a blocking thread.
#[derive(Default)]
pub(crate) struct BlockingGate {
    state: Mutex<BlockingGateState>,
    changed: Condvar,
}

impl BlockingGate {
    pub(crate) fn enter_and_wait(&self) {
        let mut state = self.state.lock().expect("blocking gate lock");
        state.entered = true;
        self.changed.notify_all();
        let state = self
            .changed
            .wait_while(state, |state| !state.released)
            .expect("blocking gate state is not poisoned");
        assert!(state.released, "blocking gate was released");
    }

    pub(crate) fn wait_until_entered(&self) {
        let state = self.state.lock().expect("blocking gate lock");
        let state = self
            .changed
            .wait_while(state, |state| !state.entered)
            .expect("blocking gate state is not poisoned");
        assert!(state.entered, "blocking gate was entered");
    }

    pub(crate) fn release(&self) {
        self.state.lock().expect("blocking gate lock").released = true;
        self.changed.notify_all();
    }
}

pub(crate) struct BlockingGateReleaseGuard(pub(crate) Arc<BlockingGate>);

impl Drop for BlockingGateReleaseGuard {
    fn drop(&mut self) {
        self.0.release();
    }
}

impl HerdrQueueWakePump {
    pub(crate) fn install_handoff_cleanup_test_gate(&self) -> Gate {
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        self.handoff_cleanup_test_gate
            .lock()
            .expect("handoff cleanup gate lock")
            .replace((Arc::clone(&entered), Arc::clone(&release)));
        (entered, release)
    }

    pub(crate) fn install_prompt_started_test_gate(&self) -> Arc<Notify> {
        let entered = Arc::new(Notify::new());
        self.prompt_started_test_gate
            .lock()
            .expect("prompt started gate lock")
            .replace(Arc::clone(&entered));
        entered
    }

    pub(crate) fn notify_prompt_started_test_gate(&self) {
        if let Some(entered) = self
            .prompt_started_test_gate
            .lock()
            .expect("prompt started gate lock")
            .take()
        {
            entered.notify_one();
        }
    }

    pub(crate) fn clear_handoff_cleanup_test_gate(&self) {
        self.handoff_cleanup_test_gate
            .lock()
            .expect("handoff cleanup gate lock")
            .take();
    }

    pub(crate) async fn await_handoff_cleanup_test_gate(&self) {
        let Some((entered, release)) = self
            .handoff_cleanup_test_gate
            .lock()
            .expect("handoff cleanup gate lock")
            .as_ref()
            .cloned()
        else {
            return;
        };
        entered.notify_one();
        release.notified().await;
    }
}
