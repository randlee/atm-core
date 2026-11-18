//! Test-only synchronization seams for Herdr queue wake cancellation tests.

use std::sync::Arc;

use tokio::sync::Notify;

use crate::herdr_queue_wake::HerdrQueueWakePump;

pub(crate) type Gate = (Arc<Notify>, Arc<Notify>);

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
