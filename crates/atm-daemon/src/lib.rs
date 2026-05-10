#![forbid(unsafe_code)]
//! Daemon runtime composition and portability adapters.

mod boundary_adapters;
pub(crate) mod composition;
mod daemon_runtime_observability;
mod direct_boundaries;
// ADR-002 intentionally splits launch.lock admission from owner.lock serving
// ownership so only one launcher can fork while only one daemon can publish the
// local IPC endpoint; see tests::host_ownership_record_uses_pid_and_token_while_held_and_clears_on_release.
mod host_ownership;
mod lifecycle_control;
mod local_ipc_transport;
mod notification_runtime;
mod peer_transport;
mod reconcile_runtime;
mod runtime_health;
#[cfg(test)]
mod test_observability;
mod watch_runtime;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use atm_core::error::AtmError;
use atm_rusqlite::SqliteBoundaryAssembly;
pub use daemon_runtime_observability::DaemonRuntimeObservability;

pub(crate) use atm_rusqlite::RemoteReplayStateRecord;
pub(crate) use local_ipc_transport::LocalIpcServerTransportAdapter;
pub(crate) use peer_transport::{PeerTransportRuntime, RemoteReplayStore};

pub(crate) const GRACEFUL_DRAIN_DEADLINE: Duration = Duration::from_secs(5);
pub(crate) const FORCE_CANCEL_DEADLINE: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub(crate) struct AtmHomeDir(PathBuf);

impl AtmHomeDir {
    pub(crate) fn resolve() -> Result<Self, AtmError> {
        Ok(Self(atm_core::home::atm_home()?))
    }

    pub(crate) fn as_path(&self) -> &std::path::Path {
        &self.0
    }

    pub(crate) fn into_inner(self) -> PathBuf {
        self.0
    }

    #[cfg(test)]
    pub(crate) fn from_path_for_test(path: PathBuf) -> Self {
        Self(path)
    }
}

#[derive(Debug, Clone)]
struct SqliteRemoteReplayStore {
    assembly: Arc<SqliteBoundaryAssembly>,
}

impl SqliteRemoteReplayStore {
    fn from_path(db_path: PathBuf) -> Result<Self, AtmError> {
        Ok(Self {
            assembly: Arc::new(SqliteBoundaryAssembly::new(db_path)?),
        })
    }
}

impl RemoteReplayStore for SqliteRemoteReplayStore {
    fn enqueue(&self, record: RemoteReplayStateRecord) -> Result<(), AtmError> {
        self.assembly.record_remote_replay_state(record)
    }

    fn load_all(&self) -> Result<Vec<RemoteReplayStateRecord>, AtmError> {
        self.assembly.load_remote_replay_states()
    }

    fn delete(
        &self,
        team: &atm_core::types::TeamName,
        agent: &atm_core::types::AgentName,
        message_key: &atm_core::boundary::MessageKey,
    ) -> Result<(), AtmError> {
        self.assembly
            .delete_remote_replay_state(team, agent, message_key)
    }

    fn purge_expired(&self, now: atm_core::types::IsoTimestamp) -> Result<usize, AtmError> {
        self.assembly.purge_expired_remote_replay_states(now)
    }
}

pub(crate) fn sqlite_remote_replay_store_from_path(
    db_path: PathBuf,
) -> Result<Arc<dyn RemoteReplayStore>, AtmError> {
    Ok(Arc::new(SqliteRemoteReplayStore::from_path(db_path)?))
}

/// Run the daemon entrypoint with the currently assembled runtime composition.
///
/// # Errors
///
/// Returns [`AtmError`] when the daemon transport cannot start or serve.
pub fn run_daemon_with_observability(
    observability: Arc<dyn DaemonRuntimeObservability>,
) -> Result<(), AtmError> {
    composition::compose_runtime(observability)?.start()
}

#[cfg(test)]
mod tests;
