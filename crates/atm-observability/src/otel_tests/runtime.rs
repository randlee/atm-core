//! Test-owned SDK runtime: tasks are real Tokio tasks, timers advance only on signals.
use std::fmt::Debug;
use std::future::Future;
use std::time::Duration;

use opentelemetry_sdk::runtime::{Runtime, RuntimeChannel, Tokio};
use tokio::sync::watch;

#[derive(Clone, Debug)]
pub(super) struct ControlledRuntime {
    periodic: watch::Sender<u64>,
    deadlines: watch::Sender<u64>,
    registrations: watch::Sender<u64>,
}

impl Default for ControlledRuntime {
    fn default() -> Self {
        Self {
            periodic: watch::channel(0).0,
            deadlines: watch::channel(0).0,
            registrations: watch::channel(0).0,
        }
    }
}

impl ControlledRuntime {
    pub fn tick(&self) {
        self.periodic.send_modify(|value| *value += 1);
    }

    pub fn expire(&self) {
        self.deadlines.send_modify(|value| *value += 1);
    }

    pub async fn tick_cycle(&self) {
        let mut registrations = self.registrations.subscribe();
        registrations
            .wait_for(|count| *count >= 3)
            .await
            .expect("timers registered");
        self.tick();
        registrations
            .wait_for(|count| *count >= 6)
            .await
            .expect("timers re-registered");
        self.tick();
    }
}

impl Runtime for ControlledRuntime {
    fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(future);
    }

    fn delay(&self, duration: Duration) -> impl Future<Output = ()> + Send + 'static {
        let mut signal = if duration == super::super::otel_setup::EXPORT_INTERVAL {
            self.periodic.subscribe()
        } else {
            self.deadlines.subscribe()
        };
        let registrations = self.registrations.clone();
        async move {
            let _ = *signal.borrow_and_update();
            if duration == super::super::otel_setup::EXPORT_INTERVAL {
                registrations.send_modify(|count| *count += 1);
            }
            signal.changed().await.expect("timer sender alive");
        }
    }
}

impl RuntimeChannel for ControlledRuntime {
    type Receiver<T: Debug + Send> = <Tokio as RuntimeChannel>::Receiver<T>;
    type Sender<T: Debug + Send> = <Tokio as RuntimeChannel>::Sender<T>;

    fn batch_message_channel<T: Debug + Send>(
        &self,
        capacity: usize,
    ) -> (Self::Sender<T>, Self::Receiver<T>) {
        Tokio.batch_message_channel(capacity)
    }
}

#[tokio::test(start_paused = true)]
async fn deadline_waits_for_explicit_expiry() {
    let runtime = ControlledRuntime::default();
    let future = runtime.delay(Duration::ZERO);
    tokio::pin!(future);
    let state = std::future::poll_fn(|cx| std::task::Poll::Ready(future.as_mut().poll(cx))).await;
    assert!(state.is_pending());
    runtime.expire();
    future.await;
}
