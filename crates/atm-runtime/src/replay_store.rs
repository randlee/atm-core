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
    db_path: std::path::PathBuf,
) -> Result<Arc<dyn boundary::RemoteReplayStore + Send + Sync>, AtmError> {
    let backend = Arc::new(SqliteStorageBackend::new(&db_path)?);
    Ok(Arc::new(SqliteRemoteReplayStore::new(backend)))
}

impl boundary::sealed::Sealed for SqliteRemoteReplayStore {}

impl boundary::RemoteReplayStore for SqliteRemoteReplayStore {
    fn enqueue(&self, record: RemoteReplayStateRecord) -> Result<(), AtmError> {
        let state_json = serde_json::to_string(&record).map_err(|error| {
            AtmError::validation(format!(
                "failed to serialize sqlite remote replay state: {error}"
            ))
            .with_recovery(
                "Repair the replay-state payload before retrying SQLite replay persistence.",
            )
            .with_source(error)
        })?;
        self.backend.record_remote_replay_state_json(
            &record.team,
            &record.agent,
            &record.message_key,
            &state_json,
        )
    }

    fn load_all(&self) -> Result<Vec<RemoteReplayStateRecord>, AtmError> {
        self.backend
            .load_remote_replay_state_json()
            .and_then(|rows| {
                rows.into_iter()
                    .map(|state_json| {
                        serde_json::from_str::<RemoteReplayStateRecord>(&state_json).map_err(
                            |error| {
                                AtmError::validation(format!(
                                    "failed to parse sqlite remote replay state: {error}"
                                ))
                                .with_recovery(
                                    "Repair the persisted daemon_remote_replay_states row before retrying replay resume.",
                                )
                                .with_source(error)
                            },
                        )
                    })
                    .collect()
            })
    }

    fn delete(
        &self,
        team: &TeamName,
        agent: &AgentName,
        message_key: &MessageKey,
    ) -> Result<(), AtmError> {
        self.backend
            .delete_remote_replay_state(team, agent, message_key)
    }

    fn purge_expired(&self, now: IsoTimestamp) -> Result<usize, AtmError> {
        self.backend.purge_expired_remote_replay_states(now)
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
