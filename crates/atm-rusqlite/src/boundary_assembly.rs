use atm_core::{
    LocalFileNonClaudeOutbound, LocalFileNotificationSink, LocalServiceRuntime,
    home::host_runtime_dir,
};
use atm_core::boundary;
use atm_core::error::AtmError;
use atm_core::protocol::RequestEnvelope;
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use rusqlite::params;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use crate::mailbox_metadata::{query_mailbox_metadata_counts, query_mailbox_metadata_rows};
use crate::observability::{
    NullSqliteObservability, SqliteObservability, SqliteObservabilityEvent,
    SqliteObservabilityOutcome,
};
use crate::shared_db::{SharedDb, deserialize_json, serialize_json};
use crate::{SqliteMailStore, SqliteTaskStore};

#[derive(Debug)]
struct SqliteRosterStore {
    pub(crate) db: Arc<SharedDb>,
}

#[path = "roster_store.rs"]
mod roster_store_impl;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RemoteReplayStateRecord {
    pub team: TeamName,
    pub agent: AgentName,
    pub message_key: boundary::MessageKey,
    pub peer_addr: SocketAddr,
    pub request: RequestEnvelope,
    pub recorded_at: IsoTimestamp,
    pub expires_at: IsoTimestamp,
    #[serde(default)]
    pub attempt_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl SqliteRosterStore {
    fn new(db: Arc<SharedDb>) -> Self {
        Self { db }
    }
}

/// Internal assembly root for Phase R SQLite-backed boundary implementations.
#[derive(Clone)]
pub struct SqliteBoundaryAssembly {
    pub(crate) mail_store: Arc<SqliteMailStore>,
    pub(crate) task_store: Arc<SqliteTaskStore>,
    roster_store: Arc<SqliteRosterStore>,
    observability: Arc<dyn SqliteObservability>,
}

impl std::fmt::Debug for SqliteBoundaryAssembly {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteBoundaryAssembly")
            .field("mail_store", &"SqliteMailStore")
            .field("task_store", &"SqliteTaskStore")
            .field("roster_store", &"SqliteRosterStore")
            .finish_non_exhaustive()
    }
}

impl SqliteBoundaryAssembly {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, AtmError> {
        Self::new_with_observability(path, Arc::new(NullSqliteObservability))
    }

    pub fn new_with_observability(
        path: impl AsRef<Path>,
        observability: Arc<dyn SqliteObservability>,
    ) -> Result<Self, AtmError> {
        let db = Arc::new(
            SharedDb::open_with_observability(path, Arc::clone(&observability)).inspect_err(
                |error| {
                    observability.emit_or_warn(SqliteObservabilityEvent::new(
                        "boundary_assembly",
                        SqliteObservabilityOutcome::Failed,
                        error.message.clone(),
                        Some(error.code),
                    ));
                },
            )?,
        );
        Ok(Self {
            mail_store: Arc::new(SqliteMailStore::new(db.clone())),
            task_store: Arc::new(SqliteTaskStore::new(db.clone())),
            roster_store: Arc::new(SqliteRosterStore::new(db)),
            observability,
        })
    }

    pub fn default_production() -> Result<Self, AtmError> {
        Self::default_production_with_observability(Arc::new(NullSqliteObservability))
    }

    pub fn default_production_with_observability(
        observability: Arc<dyn SqliteObservability>,
    ) -> Result<Self, AtmError> {
        Self::new_with_observability(SharedDb::production_path()?, observability)
    }

    #[cfg(test)]
    pub(crate) fn in_memory_for_test() -> Result<Self, AtmError> {
        let observability: Arc<dyn SqliteObservability> = Arc::new(NullSqliteObservability);
        let db = Arc::new(SharedDb::open_in_memory_with_observability(Arc::clone(
            &observability,
        ))?);
        Ok(Self {
            mail_store: Arc::new(SqliteMailStore::new(db.clone())),
            task_store: Arc::new(SqliteTaskStore::new(db.clone())),
            roster_store: Arc::new(SqliteRosterStore::new(db)),
            observability,
        })
    }

    pub fn mail_store(&self) -> &dyn boundary::MailStore {
        self.mail_store.as_ref()
    }

    pub fn mail_store_arc(&self) -> Arc<dyn boundary::MailStore + Send + Sync> {
        self.mail_store.clone()
    }

    pub fn task_store(&self) -> &dyn boundary::TaskStore {
        self.task_store.as_ref()
    }

    pub fn task_store_arc(&self) -> Arc<dyn boundary::TaskStore + Send + Sync> {
        self.task_store.clone()
    }

    pub fn roster_store(&self) -> &dyn boundary::RosterStore {
        self.roster_store.as_ref()
    }

    pub fn roster_store_arc(&self) -> Arc<dyn boundary::RosterStore + Send + Sync> {
        self.roster_store.clone()
    }

    pub fn checkpoint_wal(&self) -> Result<(), AtmError> {
        self.mail_store.db.checkpoint_wal().inspect_err(|error| {
            self.observability
                .emit_or_warn(SqliteObservabilityEvent::new(
                    "boundary_checkpoint_wal",
                    SqliteObservabilityOutcome::Failed,
                    error.message.clone(),
                    Some(error.code),
                ));
        })
    }

    pub fn query_mailbox_metadata_rows(
        &self,
        team: &TeamName,
        agent: &AgentName,
        limit: Option<usize>,
    ) -> Result<Vec<boundary::MailStoreMailboxMetadataRow>, AtmError> {
        query_mailbox_metadata_rows(&self.mail_store.db, team, agent, limit)
    }

    pub fn query_mailbox_metadata_counts(
        &self,
        team: &TeamName,
        agent: &AgentName,
    ) -> Result<boundary::MailStoreMailboxMetadataCounts, AtmError> {
        query_mailbox_metadata_counts(&self.mail_store.db, team, agent)
    }

    pub fn record_remote_replay_state(
        &self,
        record: RemoteReplayStateRecord,
    ) -> Result<(), AtmError> {
        let state_json = serialize_json(&record, "daemon remote replay state")?;
        self.mail_store.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "INSERT INTO daemon_remote_replay_states(team, agent, message_key, state_json)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(team, agent, message_key) DO UPDATE SET
                       state_json = excluded.state_json;",
                    params![
                        record.team.as_str(),
                        record.agent.as_str(),
                        record.message_key.as_ref(),
                        state_json,
                    ],
                )
                .map_err(|error| {
                    self.mail_store
                        .db
                        .error("failed to record daemon remote replay state", error)
                })?;
            Ok(())
        })
    }

    pub fn load_remote_replay_states(&self) -> Result<Vec<RemoteReplayStateRecord>, AtmError> {
        self.mail_store.db.with_connection(|connection| {
            let mut statement = connection
                .prepare(
                    "SELECT state_json
                     FROM daemon_remote_replay_states
                     ORDER BY team, agent, message_key
                     LIMIT 10000;",
                )
                .map_err(|error| {
                    self.mail_store
                        .db
                        .error("failed to prepare daemon remote replay query", error)
                })?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|error| {
                    self.mail_store
                        .db
                        .error("failed to read daemon remote replay rows", error)
                })?;
            let mut records = Vec::new();
            for row in rows {
                let state_json = row.map_err(|error| {
                    self.mail_store
                        .db
                        .error("failed to decode daemon remote replay row", error)
                })?;
                records.push(deserialize_json(&state_json, "daemon remote replay state")?);
            }
            Ok(records)
        })
    }

    pub fn delete_remote_replay_state(
        &self,
        team: &TeamName,
        agent: &AgentName,
        message_key: &boundary::MessageKey,
    ) -> Result<(), AtmError> {
        self.mail_store.db.with_transaction(|transaction| {
            transaction
                .execute(
                    "DELETE FROM daemon_remote_replay_states
                     WHERE team = ?1 AND agent = ?2 AND message_key = ?3;",
                    params![team.as_str(), agent.as_str(), message_key.as_ref(),],
                )
                .map_err(|error| {
                    self.mail_store
                        .db
                        .error("failed to delete daemon remote replay state", error)
                })?;
            Ok(())
        })
    }

    pub fn purge_expired_remote_replay_states(&self, now: IsoTimestamp) -> Result<usize, AtmError> {
        let now = now.into_inner().to_rfc3339();
        self.mail_store.db.with_transaction(|transaction| {
            // Accepted risk: remote replay rows expire slowly and this purge
            // runs on a bounded maintenance path, so one unbounded DELETE is
            // acceptable until replay-volume data proves otherwise.
            transaction
                .execute(
                    "DELETE FROM daemon_remote_replay_states
                     WHERE json_extract(state_json, '$.expires_at') <= ?1;",
                    params![now],
                )
                .map_err(|error| {
                    self.mail_store
                        .db
                        .error("failed to purge expired daemon remote replay state", error)
                })
        })
    }
}

pub fn assemble_boundary(path: impl AsRef<Path>) -> Result<SqliteBoundaryAssembly, AtmError> {
    SqliteBoundaryAssembly::new(path)
}

pub fn assemble_boundary_with_observability(
    path: impl AsRef<Path>,
    observability: Arc<dyn SqliteObservability>,
) -> Result<SqliteBoundaryAssembly, AtmError> {
    SqliteBoundaryAssembly::new_with_observability(path, observability)
}

pub fn assemble_default_boundary() -> Result<SqliteBoundaryAssembly, AtmError> {
    SqliteBoundaryAssembly::default_production()
}

pub fn assemble_default_boundary_with_observability(
    observability: Arc<dyn SqliteObservability>,
) -> Result<SqliteBoundaryAssembly, AtmError> {
    SqliteBoundaryAssembly::default_production_with_observability(observability)
}

pub fn default_local_runtime() -> Result<LocalServiceRuntime, AtmError> {
    let assembly = assemble_default_boundary()?;
    Ok(LocalServiceRuntime::new_with_delivery_boundaries(
        assembly.mail_store_arc(),
        assembly.task_store_arc(),
        assembly.roster_store_arc(),
        Arc::new(LocalFileNonClaudeOutbound::new()),
        Arc::new(LocalFileNotificationSink::at_path(
            host_runtime_dir()?.join("notifications.jsonl"),
        )),
    ))
}
