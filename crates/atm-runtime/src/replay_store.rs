use std::path::PathBuf;
use std::sync::Arc;

use atm_core::boundary::{self, MessageKey, RemoteReplayStateRecord};
use atm_core::error::AtmError;
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use atm_rusqlite::{NullSqliteObservability, SqliteBoundaryAssembly};

#[derive(Debug, Clone)]
pub(crate) struct SqliteRemoteReplayStore {
    assembly: Arc<SqliteBoundaryAssembly>,
}

impl SqliteRemoteReplayStore {
    pub(crate) fn new(assembly: Arc<SqliteBoundaryAssembly>) -> Self {
        Self { assembly }
    }
}

/// Test-only helper for daemon and runtime smoke coverage that need one real
/// replay-store implementation without widening the public runtime assembly API.
#[cfg(any(test, feature = "test-utils"))]
pub fn sqlite_remote_replay_store_for_test(
    db_path: PathBuf,
) -> Result<Arc<dyn boundary::RemoteReplayStore + Send + Sync>, AtmError> {
    let assembly = Arc::new(SqliteBoundaryAssembly::new_with_observability(
        db_path,
        Arc::new(NullSqliteObservability),
    )?);
    Ok(Arc::new(SqliteRemoteReplayStore::new(assembly)))
}

impl boundary::sealed::Sealed for SqliteRemoteReplayStore {}

impl boundary::RemoteReplayStore for SqliteRemoteReplayStore {
    fn enqueue(&self, record: RemoteReplayStateRecord) -> Result<(), AtmError> {
        self.assembly.record_remote_replay_state(record)
    }

    fn load_all(&self) -> Result<Vec<RemoteReplayStateRecord>, AtmError> {
        self.assembly.load_remote_replay_states()
    }

    fn delete(
        &self,
        team: &TeamName,
        agent: &AgentName,
        message_key: &MessageKey,
    ) -> Result<(), AtmError> {
        self.assembly
            .delete_remote_replay_state(team, agent, message_key)
    }

    fn purge_expired(&self, now: IsoTimestamp) -> Result<usize, AtmError> {
        self.assembly.purge_expired_remote_replay_states(now)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SqliteRuntimeStorageFinalizer {
    assembly: Arc<SqliteBoundaryAssembly>,
}

impl SqliteRuntimeStorageFinalizer {
    pub(crate) fn new(assembly: Arc<SqliteBoundaryAssembly>) -> Self {
        Self { assembly }
    }
}

impl boundary::sealed::Sealed for SqliteRuntimeStorageFinalizer {}

impl boundary::RuntimeStorageFinalizer for SqliteRuntimeStorageFinalizer {
    fn finalize_storage_shutdown(&self) -> Result<(), AtmError> {
        self.assembly.checkpoint_wal()
    }
}
