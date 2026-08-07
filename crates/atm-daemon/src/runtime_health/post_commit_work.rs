//! Post-commit peer-delivery signals.
//!
//! Receiver hooks deliberately do not pass through this adapter. They run
//! synchronously after a newly persisted inbound write so a hook warning can
//! remain attached to the successful receive response.

use std::sync::Arc;

use atm_core::schema::AtmMessageId;

use crate::peer_drain_coordinator::PeerDeliveryCoordinator;

/// Identifier-only peer-delivery work admitted after an origin write commits.
///
/// This adapter is not a notification queue: received-message hooks have no
/// variant here and no background worker.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PostCommitWorkKey {
    PeerDelivery {
        peer: atm_core::types::HostName,
        message_id: AtmMessageId,
    },
}

/// The daemon-owned peer-delivery signal boundary.
pub(crate) trait PostCommitWorkQueue: Send + Sync {
    fn signal(&self, work: PostCommitWorkKey);
}

/// Thin adapter retained until AM removes legacy peer-delivery coordination.
/// It owns no hook worker, queue, retry task, or mutable delivery state.
pub(crate) struct PeerPostCommitWorkQueue {
    coordinator: Arc<dyn PeerDeliveryCoordinator>,
}

impl PeerPostCommitWorkQueue {
    pub(crate) fn new(coordinator: Arc<dyn PeerDeliveryCoordinator>) -> Self {
        Self { coordinator }
    }
}

impl PostCommitWorkQueue for PeerPostCommitWorkQueue {
    fn signal(&self, work: PostCommitWorkKey) {
        let PostCommitWorkKey::PeerDelivery { peer, message_id } = work;
        self.coordinator.signal_after_persist(peer, message_id);
    }
}
