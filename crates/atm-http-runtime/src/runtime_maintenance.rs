//! Runtime maintenance lifecycle handles and shutdown task contract.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::loopback_tcp::LoopbackEndpointRecordGuard;

/// Final slice of the shutdown deadline in which cancelled tasks are joined.
pub(crate) const ABORT_JOIN_GRACE: Duration = Duration::from_millis(100);
/// Last slice of the shutdown deadline, kept for endpoint-record cleanup.
pub(crate) const ENDPOINT_CLEANUP_RESERVE: Duration = Duration::from_millis(100);

/// A runtime-owned maintenance task that follows the server shutdown signal.
pub trait RuntimeMaintenance: Send + Sync {
    fn start(&self, shutdown: watch::Receiver<()>) -> JoinHandle<()>;
}

pub struct Running {
    pub(crate) local_address: SocketAddr,
    pub(crate) direct_peer_address: Option<SocketAddr>,
    pub(crate) shutdown_tx: watch::Sender<()>,
    pub(crate) server_stopped_rx: watch::Receiver<bool>,
    pub(crate) server_task: JoinHandle<std::io::Result<()>>,
    pub(crate) maintenance_task: Option<JoinHandle<()>>,
    pub(crate) endpoint_record: LoopbackEndpointRecordGuard,
}

pub struct Draining {
    pub(crate) server_task: JoinHandle<std::io::Result<()>>,
    pub(crate) maintenance_task: Option<JoinHandle<()>>,
    pub(crate) endpoint_record: LoopbackEndpointRecordGuard,
}

pub struct Stopped;

/// Joins an already-aborted task until `deadline`; a task that is still
/// running then (blocked in synchronous code) is reported and detached.
pub(crate) async fn join_aborted<T>(task: &mut JoinHandle<T>, deadline: Instant) {
    if tokio::time::timeout_at(deadline, task).await.is_err() {
        tracing::warn!(
            abort_join_grace_ms = ABORT_JOIN_GRACE.as_millis(),
            "runtime task exceeded the bounded abort-join grace"
        );
    }
}
