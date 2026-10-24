//! A test-controlled hook deadline source for deterministic runner tests.

/// Advances only when a test explicitly requests expiry, never from elapsed
/// wall-clock time.
#[derive(Debug)]
pub(crate) struct ControlledHookDeadline {
    generation: tokio::sync::watch::Sender<u64>,
    process_registrations: tokio::sync::watch::Sender<u64>,
}

impl ControlledHookDeadline {
    pub(crate) fn new() -> Self {
        Self {
            generation: tokio::sync::watch::channel(0).0,
            process_registrations: tokio::sync::watch::channel(0).0,
        }
    }

    /// Expires all waits already registered against this test-owned source.
    #[allow(dead_code)] // Used by feature-gated timeout tests; composition only retains the source.
    pub(crate) fn advance(&self) {
        self.generation
            .send_modify(|value| *value = value.wrapping_add(1));
    }

    #[allow(dead_code)] // Used by feature-gated runner expiry tests.
    pub(crate) async fn wait_for_process_registration(&self) {
        let mut registrations = self.process_registrations.subscribe();
        let observed = *registrations.borrow_and_update();
        registrations
            .wait_for(|value| *value > observed)
            .await
            .expect("controlled hook deadline sender alive");
    }

    pub(super) async fn wait_for_advance(&self) {
        let mut generation = self.generation.subscribe();
        let current = *generation.borrow_and_update();
        generation
            .wait_for(|value| *value != current)
            .await
            .expect("controlled hook deadline sender alive");
    }

    pub(super) async fn wait_for_process_advance(&self) {
        self.process_registrations
            .send_modify(|value| *value = value.wrapping_add(1));
        self.wait_for_advance().await;
    }
}
