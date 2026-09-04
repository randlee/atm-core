use atm_http_runtime::{BareCliFifo, BareCliQueueFullDrops};

/// Owns the daemon-lifetime bare-CLI FIFO and its bounded-overflow counter.
///
/// The composition root passes this one channel bundle to both the receiver
/// selector and the HTTP router, ensuring every queue-pull delivery observes
/// the same FIFO and metrics state.
#[derive(Clone, Default)]
pub(crate) struct BareCliRuntime {
    fifo: BareCliFifo,
    queue_full_drops: BareCliQueueFullDrops,
}

impl BareCliRuntime {
    pub(crate) fn fifo(&self) -> BareCliFifo {
        self.fifo.clone()
    }

    pub(crate) fn queue_full_drops(&self) -> BareCliQueueFullDrops {
        self.queue_full_drops.clone()
    }
}
