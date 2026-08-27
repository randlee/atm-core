//! Runtime maintenance lifecycle handles and shutdown task contract.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::loopback_tcp::LoopbackEndpointRecordGuard;

pub(crate) const ABORT_JOIN_GRACE: Duration = Duration::from_millis(100);

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

pub(crate) async fn abort_and_join<T>(task: &mut JoinHandle<T>) {
    task.abort();
    if tokio::time::timeout(ABORT_JOIN_GRACE, task).await.is_err() {
        tracing::warn!(
            abort_join_grace_ms = ABORT_JOIN_GRACE.as_millis(),
            "runtime task exceeded the bounded abort-join grace"
        );
    }
}

pub(crate) async fn finish_maintenance(mut task: JoinHandle<()>, shutdown_timeout: Duration) {
    if tokio::time::timeout(shutdown_timeout, &mut task)
        .await
        .is_err()
    {
        abort_and_join(&mut task).await;
    }
}
