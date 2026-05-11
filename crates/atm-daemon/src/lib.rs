#![forbid(unsafe_code)]
//! Daemon runtime composition and portability adapters.

mod active_connection_registry;
mod boundary_adapters;
pub(crate) mod composition;
mod daemon_runtime_observability;
mod direct_boundaries;
// ADR-002 (`docs/adr/ADR-002-host-wide-daemon-singleton.md`) intentionally splits
// launch.lock admission from owner.lock serving ownership so only one launcher can
// fork while only one daemon can publish the local IPC endpoint; see
// tests::host_ownership_record_uses_pid_and_token_while_held_and_clears_on_release.
mod host_ownership;
mod lifecycle_control;
mod local_ipc_transport;
mod notification_runtime;
mod peer_transport;
mod reconcile_runtime;
mod runtime_health;
mod shutdown_beacon;
#[cfg(test)]
mod test_observability;
#[cfg(test)]
mod test_support;
mod watch_runtime;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use atm_core::error::{AtmError, AtmErrorCode};
use atm_rusqlite::SqliteBoundaryAssembly;
pub use daemon_runtime_observability::DaemonRuntimeObservability;

pub(crate) use atm_rusqlite::RemoteReplayStateRecord;
pub(crate) use local_ipc_transport::LocalIpcServerTransportAdapter;
pub(crate) use peer_transport::{PeerTransportRuntime, RemoteReplayStore};

pub(crate) const GRACEFUL_DRAIN_DEADLINE: Duration = Duration::from_secs(2);
pub(crate) const FORCE_CANCEL_DEADLINE: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonExitCode {
    CleanStop = 0,
    InternalBug = 1,
    DoNotRestart = 64,
    TransportFatal = 70,
    LifecycleWedge = 71,
}

impl DaemonExitCode {
    pub const fn as_i32(self) -> i32 {
        self as i32
    }
}

pub fn daemon_exit_code_for_error(error: &AtmError) -> DaemonExitCode {
    if error.code == AtmErrorCode::DaemonLifecycleWedge {
        return DaemonExitCode::LifecycleWedge;
    }
    if matches!(
        error.code,
        AtmErrorCode::DaemonServingStateRejected
            | AtmErrorCode::DaemonStaleOwnerRecoveryFailed
            | AtmErrorCode::DaemonLaunchGateRejected
            | AtmErrorCode::ConfigHomeUnavailable
            | AtmErrorCode::ObservabilityBootstrapFailed
    ) || error.is_config()
    {
        return DaemonExitCode::DoNotRestart;
    }
    if error.is_daemon_unavailable() {
        return DaemonExitCode::TransportFatal;
    }
    DaemonExitCode::InternalBug
}

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
