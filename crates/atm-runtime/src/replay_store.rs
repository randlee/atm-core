use std::path::PathBuf;
use std::sync::Arc;

use atm_core::boundary::{self, MessageKey, RemoteReplayStateRecord};
use atm_core::error::AtmError;
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use atm_storage_rusqlite::SqliteStorageBackend;

#[derive(Debug, Clone)]
pub(crate) struct SqliteRemoteReplayStore {
    backend: Arc<SqliteStorageBackend>,
}

impl SqliteRemoteReplayStore {
    pub(crate) fn new(backend: Arc<SqliteStorageBackend>) -> Self {
        Self { backend }
    }
}

/// Test-only helper for daemon and runtime smoke coverage that need one real
/// replay-store implementation without widening the public runtime assembly API.
#[cfg(any(test, feature = "test-utils"))]
pub fn sqlite_remote_replay_store_for_test(
    db_path: PathBuf,
) -> Result<Arc<dyn boundary::RemoteReplayStore + Send + Sync>, AtmError> {
    let backend = Arc::new(SqliteStorageBackend::new(&db_path)?);
    Ok(Arc::new(SqliteRemoteReplayStore::new(backend)))
}

impl boundary::sealed::Sealed for SqliteRemoteReplayStore {}

impl boundary::RemoteReplayStore for SqliteRemoteReplayStore {
    fn enqueue(&self, record: RemoteReplayStateRecord) -> Result<(), AtmError> {
        let state_json = serde_json::to_string(&record).map_err(|source| {
            AtmError::validation("failed to serialize daemon remote replay state")
                .with_recovery(
                    "Repair the remote replay state shape before retrying daemon replay persistence.",
                )
                .with_source(source)
        })?;
        self.backend.upsert_remote_replay_state(
            record.team.as_str(),
            record.agent.as_str(),
            record.message_key.as_ref(),
            &state_json,
        )
    }

    fn load_all(&self) -> Result<Vec<RemoteReplayStateRecord>, AtmError> {
        self.backend
            .load_all_remote_replay_states()?
            .into_iter()
            .map(|state_json| {
                serde_json::from_str(&state_json).map_err(|source| {
                    AtmError::validation("failed to deserialize daemon remote replay row")
                        .with_recovery(
                            "Repair the sqlite remote replay row before retrying daemon replay loading.",
                        )
                        .with_source(source)
                })
            })
            .collect()
    }

    fn delete(
        &self,
        team: &TeamName,
        agent: &AgentName,
        message_key: &MessageKey,
    ) -> Result<(), AtmError> {
        self.backend
            .delete_remote_replay_state(team.as_str(), agent.as_str(), message_key.as_ref())
    }

    fn purge_expired(&self, now: IsoTimestamp) -> Result<usize, AtmError> {
        let records = self.load_all()?;
        let mut purged = 0usize;
        for record in records
            .into_iter()
            .filter(|record| record.expires_at <= now)
        {
            self.delete(&record.team, &record.agent, &record.message_key)?;
            purged += 1;
        }
        Ok(purged)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SqliteRuntimeStorageFinalizer {
    backend: Arc<SqliteStorageBackend>,
}

impl SqliteRuntimeStorageFinalizer {
    pub(crate) fn new(backend: Arc<SqliteStorageBackend>) -> Self {
        Self { backend }
    }
}

impl boundary::sealed::Sealed for SqliteRuntimeStorageFinalizer {}

impl boundary::RuntimeStorageFinalizer for SqliteRuntimeStorageFinalizer {
    fn finalize_storage_shutdown(&self) -> Result<(), AtmError> {
        self.backend.checkpoint_wal()
    }
}
