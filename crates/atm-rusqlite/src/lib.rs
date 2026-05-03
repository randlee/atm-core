use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use atm_core::home::mail_db_path_from_home;
use atm_core::mail_store::{
    AckStateRecord, IngestRecord, MailStore, MessageSourceKind, PendingExportRecord,
    StoredMessageRecord, VisibilityStateRecord,
};
use atm_core::roster_store::{PidUpdate, RosterMemberRecord, RosterStore};
use atm_core::schema::{AtmMessageId, LegacyMessageId};
use atm_core::store::{
    BusyTimeoutMs, InsertOutcome, MessageKey, ProcessId, SourceFingerprint, SqliteHandleBudget,
    StoreBootstrapReport, StoreBoundary, StoreDuplicateIdentity, StoreError, StoreHealth,
};
use atm_core::task_store::{TaskRecord, TaskStatus, TaskStore};
use atm_core::types::{AgentName, TaskId, TeamName};
use rusqlite::{Connection, OptionalExtension, Transaction};

const SCHEMA_VERSION: i64 = 1;

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS messages (
    message_key TEXT PRIMARY KEY,
    team_name TEXT NOT NULL,
    recipient_agent TEXT NOT NULL,
    sender_display TEXT NOT NULL,
    sender_canonical TEXT NULL,
    sender_team TEXT NULL,
    body TEXT NOT NULL,
    summary TEXT NULL,
    created_at TEXT NOT NULL,
    source_kind TEXT NOT NULL,
    legacy_message_id TEXT NULL UNIQUE,
    atm_message_id TEXT NULL UNIQUE,
    raw_metadata_json TEXT NULL
);

CREATE INDEX IF NOT EXISTS idx_messages_team_recipient_created
    ON messages(team_name, recipient_agent, created_at);

CREATE TABLE IF NOT EXISTS inbox_ingest (
    team_name TEXT NOT NULL,
    recipient_agent TEXT NOT NULL,
    source_path TEXT NOT NULL,
    source_fingerprint TEXT NOT NULL,
    message_key TEXT NOT NULL,
    imported_at TEXT NOT NULL,
    PRIMARY KEY (team_name, recipient_agent, source_fingerprint),
    FOREIGN KEY (message_key) REFERENCES messages(message_key) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS ack_state (
    message_key TEXT PRIMARY KEY,
    pending_ack_at TEXT NULL,
    acknowledged_at TEXT NULL,
    ack_reply_message_key TEXT NULL,
    ack_reply_team TEXT NULL,
    ack_reply_agent TEXT NULL,
    FOREIGN KEY (message_key) REFERENCES messages(message_key) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS tasks (
    task_id TEXT PRIMARY KEY,
    message_key TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    acknowledged_at TEXT NULL,
    metadata_json TEXT NULL,
    FOREIGN KEY (message_key) REFERENCES messages(message_key) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_tasks_message_key ON tasks(message_key);

CREATE TABLE IF NOT EXISTS team_roster (
    team_name TEXT NOT NULL,
    agent_name TEXT NOT NULL,
    role TEXT NOT NULL,
    transport_kind TEXT NOT NULL,
    host_name TEXT NOT NULL,
    recipient_pane_id TEXT NULL,
    pid INTEGER NULL,
    metadata_json TEXT NULL,
    PRIMARY KEY (team_name, agent_name)
);

CREATE TABLE IF NOT EXISTS message_visibility (
    message_key TEXT PRIMARY KEY,
    read_at TEXT NULL,
    cleared_at TEXT NULL,
    FOREIGN KEY (message_key) REFERENCES messages(message_key) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS pending_exports (
    message_key TEXT PRIMARY KEY,
    export_target_team TEXT NOT NULL,
    export_target_agent TEXT NOT NULL,
    recipient_pane_id TEXT NULL,
    attempt_count INTEGER NOT NULL,
    next_attempt_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    FOREIGN KEY (message_key) REFERENCES messages(message_key) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_pending_exports_next_attempt
    ON pending_exports(next_attempt_at);
"#;

#[derive(Debug)]
pub struct RusqliteStore {
    database_path: PathBuf,
    connection: Mutex<Connection>,
    busy_timeout_ms: BusyTimeoutMs,
    handle_budget: SqliteHandleBudget,
}

impl RusqliteStore {
    pub fn open_for_team_home(home_dir: &Path, team_name: &TeamName) -> Result<Self, StoreError> {
        let database_path =
            mail_db_path_from_home(home_dir, team_name.as_str()).map_err(|error| {
                StoreError::open("failed to resolve team mail.db path").with_source(error)
            })?;
        Self::open_path(database_path)
    }

    pub fn open_path(database_path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::open_path_with_options(
            database_path.as_ref(),
            BusyTimeoutMs::DEFAULT,
            SqliteHandleBudget::DEFAULT,
        )
    }

    fn open_path_with_options(
        database_path: &Path,
        busy_timeout_ms: BusyTimeoutMs,
        handle_budget: SqliteHandleBudget,
    ) -> Result<Self, StoreError> {
        if let Some(parent) = database_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                StoreError::open(format!(
                    "failed to create SQLite store directory {}",
                    parent.display()
                ))
                .with_source(error)
            })?;
        }

        let mut connection = Connection::open(database_path).map_err(|error| {
            StoreError::open(format!(
                "failed to open SQLite store {}",
                database_path.display()
            ))
            .with_source(error)
        })?;

        configure_connection(&connection, busy_timeout_ms)?;
        bootstrap_schema(&mut connection)?;

        Ok(Self {
            database_path: database_path.to_path_buf(),
            connection: Mutex::new(connection),
            busy_timeout_ms,
            handle_budget,
        })
    }

    fn lock_connection(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.connection
            .lock()
            .map_err(|_| StoreError::transaction("SQLite store mutex poisoned"))
    }

    fn with_transaction<T>(
        &self,
        action: impl FnOnce(&Transaction<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction().map_err(|error| {
            StoreError::transaction("failed to start SQLite transaction").with_source(error)
        })?;
        let result = action(&transaction)?;
        transaction.commit().map_err(|error| {
            StoreError::transaction("failed to commit SQLite transaction").with_source(error)
        })?;
        Ok(result)
    }
}

impl StoreBoundary for RusqliteStore {
    fn bootstrap_report(&self) -> Result<StoreBootstrapReport, StoreError> {
        let connection = self.lock_connection()?;
        Ok(StoreBootstrapReport {
            database_path: self.database_path.clone(),
            schema_version: query_user_version(&connection)?,
            wal_enabled: query_journal_mode(&connection)?.eq_ignore_ascii_case("wal"),
            foreign_keys_enabled: query_foreign_keys(&connection)?,
            busy_timeout_ms: self.busy_timeout_ms,
            handle_budget: self.handle_budget,
        })
    }

    fn health(&self) -> Result<StoreHealth, StoreError> {
        let connection = self.lock_connection()?;
        Ok(StoreHealth {
            database_path: self.database_path.clone(),
            ready: true,
            schema_version: query_user_version(&connection)?,
        })
    }
}

impl MailStore for RusqliteStore {
    fn insert_message(
        &self,
        message: &StoredMessageRecord,
    ) -> Result<InsertOutcome<StoredMessageRecord>, StoreError> {
        let connection = self.lock_connection()?;
        match insert_message_row(&connection, message) {
            Ok(()) => Ok(InsertOutcome::Inserted(message.clone())),
            Err(error) => match classify_message_duplicate(&error, message) {
                Some(identity) => Ok(InsertOutcome::Duplicate(identity)),
                None => Err(classify_store_error(error, "failed to insert message row")),
            },
        }
    }

    fn insert_message_batch(&self, messages: &[StoredMessageRecord]) -> Result<(), StoreError> {
        self.with_transaction(|transaction| {
            for message in messages {
                insert_message_row(transaction, message).map_err(|error| {
                    classify_store_error(error, "failed to insert message batch row")
                })?;
            }
            Ok(())
        })
    }

    fn load_message(
        &self,
        message_key: &MessageKey,
    ) -> Result<Option<StoredMessageRecord>, StoreError> {
        let connection = self.lock_connection()?;
        let raw = connection
            .query_row(
                &format!("{MESSAGE_SELECT_COLUMNS} WHERE message_key = ?1"),
                [message_key.as_str()],
                |row| {
                    Ok(RawMessageRow {
                        message_key: row.get(0)?,
                        team_name: row.get(1)?,
                        recipient_agent: row.get(2)?,
                        sender_display: row.get(3)?,
                        sender_canonical: row.get(4)?,
                        sender_team: row.get(5)?,
                        body: row.get(6)?,
                        summary: row.get(7)?,
                        created_at: row.get(8)?,
                        source_kind: row.get(9)?,
                        legacy_message_id: row.get(10)?,
                        atm_message_id: row.get(11)?,
                        raw_metadata_json: row.get(12)?,
                    })
                },
            )
            .optional()
            .map_err(|error| classify_store_error(error, "failed to load message by key"))?;
        raw.map(convert_message_row).transpose()
    }

    fn load_message_by_legacy_id(
        &self,
        legacy_message_id: &LegacyMessageId,
    ) -> Result<Option<StoredMessageRecord>, StoreError> {
        let connection = self.lock_connection()?;
        let raw = connection
            .query_row(
                &format!("{MESSAGE_SELECT_COLUMNS} WHERE legacy_message_id = ?1"),
                [legacy_message_id.to_string()],
                |row| {
                    Ok(RawMessageRow {
                        message_key: row.get(0)?,
                        team_name: row.get(1)?,
                        recipient_agent: row.get(2)?,
                        sender_display: row.get(3)?,
                        sender_canonical: row.get(4)?,
                        sender_team: row.get(5)?,
                        body: row.get(6)?,
                        summary: row.get(7)?,
                        created_at: row.get(8)?,
                        source_kind: row.get(9)?,
                        legacy_message_id: row.get(10)?,
                        atm_message_id: row.get(11)?,
                        raw_metadata_json: row.get(12)?,
                    })
                },
            )
            .optional()
            .map_err(|error| classify_store_error(error, "failed to load message by legacy id"))?;
        raw.map(convert_message_row).transpose()
    }

    fn load_message_by_atm_id(
        &self,
        atm_message_id: &AtmMessageId,
    ) -> Result<Option<StoredMessageRecord>, StoreError> {
        let connection = self.lock_connection()?;
        let raw = connection
            .query_row(
                &format!("{MESSAGE_SELECT_COLUMNS} WHERE atm_message_id = ?1"),
                [atm_message_id.to_string()],
                |row| {
                    Ok(RawMessageRow {
                        message_key: row.get(0)?,
                        team_name: row.get(1)?,
                        recipient_agent: row.get(2)?,
                        sender_display: row.get(3)?,
                        sender_canonical: row.get(4)?,
                        sender_team: row.get(5)?,
                        body: row.get(6)?,
                        summary: row.get(7)?,
                        created_at: row.get(8)?,
                        source_kind: row.get(9)?,
                        legacy_message_id: row.get(10)?,
                        atm_message_id: row.get(11)?,
                        raw_metadata_json: row.get(12)?,
                    })
                },
            )
            .optional()
            .map_err(|error| classify_store_error(error, "failed to load message by atm id"))?;
        raw.map(convert_message_row).transpose()
    }

    fn upsert_ack_state(&self, ack_state: &AckStateRecord) -> Result<AckStateRecord, StoreError> {
        let connection = self.lock_connection()?;
        connection
            .execute(
                r#"
                INSERT INTO ack_state (
                    message_key,
                    pending_ack_at,
                    acknowledged_at,
                    ack_reply_message_key,
                    ack_reply_team,
                    ack_reply_agent
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ON CONFLICT(message_key) DO UPDATE SET
                    pending_ack_at = excluded.pending_ack_at,
                    acknowledged_at = excluded.acknowledged_at,
                    ack_reply_message_key = excluded.ack_reply_message_key,
                    ack_reply_team = excluded.ack_reply_team,
                    ack_reply_agent = excluded.ack_reply_agent
                "#,
                (
                    ack_state.message_key.as_str(),
                    ack_state.pending_ack_at.as_ref().map(ToString::to_string),
                    ack_state.acknowledged_at.as_ref().map(ToString::to_string),
                    ack_state
                        .ack_reply_message_key
                        .as_ref()
                        .map(|value| value.to_string()),
                    ack_state.ack_reply_team.as_ref().map(ToString::to_string),
                    ack_state.ack_reply_agent.as_ref().map(ToString::to_string),
                ),
            )
            .map_err(|error| classify_store_error(error, "failed to upsert ack state"))?;
        Ok(ack_state.clone())
    }

    fn load_ack_state(
        &self,
        message_key: &MessageKey,
    ) -> Result<Option<AckStateRecord>, StoreError> {
        let connection = self.lock_connection()?;
        let raw = connection
            .query_row(
                "SELECT message_key, pending_ack_at, acknowledged_at, ack_reply_message_key, ack_reply_team, ack_reply_agent FROM ack_state WHERE message_key = ?1",
                [message_key.as_str()],
                |row| {
                    Ok(RawAckStateRow {
                        message_key: row.get(0)?,
                        pending_ack_at: row.get(1)?,
                        acknowledged_at: row.get(2)?,
                        ack_reply_message_key: row.get(3)?,
                        ack_reply_team: row.get(4)?,
                        ack_reply_agent: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(|error| classify_store_error(error, "failed to load ack state"))?;
        raw.map(convert_ack_state_row).transpose()
    }

    fn upsert_visibility(
        &self,
        visibility: &VisibilityStateRecord,
    ) -> Result<VisibilityStateRecord, StoreError> {
        let connection = self.lock_connection()?;
        connection
            .execute(
                r#"
                INSERT INTO message_visibility (message_key, read_at, cleared_at)
                VALUES (?1, ?2, ?3)
                ON CONFLICT(message_key) DO UPDATE SET
                    read_at = excluded.read_at,
                    cleared_at = excluded.cleared_at
                "#,
                (
                    visibility.message_key.as_str(),
                    visibility.read_at.as_ref().map(ToString::to_string),
                    visibility.cleared_at.as_ref().map(ToString::to_string),
                ),
            )
            .map_err(|error| classify_store_error(error, "failed to upsert visibility state"))?;
        Ok(visibility.clone())
    }

    fn load_visibility(
        &self,
        message_key: &MessageKey,
    ) -> Result<Option<VisibilityStateRecord>, StoreError> {
        let connection = self.lock_connection()?;
        let raw = connection
            .query_row(
                "SELECT message_key, read_at, cleared_at FROM message_visibility WHERE message_key = ?1",
                [message_key.as_str()],
                |row| {
                    Ok(RawVisibilityRow {
                        message_key: row.get(0)?,
                        read_at: row.get(1)?,
                        cleared_at: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(|error| classify_store_error(error, "failed to load visibility state"))?;
        raw.map(convert_visibility_row).transpose()
    }

    fn record_ingest(
        &self,
        ingest_record: &IngestRecord,
    ) -> Result<InsertOutcome<IngestRecord>, StoreError> {
        let connection = self.lock_connection()?;
        match connection.execute(
            r#"
            INSERT INTO inbox_ingest (
                team_name,
                recipient_agent,
                source_path,
                source_fingerprint,
                message_key,
                imported_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            (
                ingest_record.team_name.as_str(),
                ingest_record.recipient_agent.as_str(),
                ingest_record.source_path.display().to_string(),
                ingest_record.source_fingerprint.as_str(),
                ingest_record.message_key.as_str(),
                ingest_record.imported_at.to_string(),
            ),
        ) {
            Ok(_) => Ok(InsertOutcome::Inserted(ingest_record.clone())),
            Err(error) => match classify_ingest_duplicate(&error, ingest_record) {
                Some(identity) => Ok(InsertOutcome::Duplicate(identity)),
                None => Err(classify_store_error(error, "failed to record ingest row")),
            },
        }
    }

    fn load_ingest(
        &self,
        team_name: &TeamName,
        recipient_agent: &AgentName,
        source_fingerprint: &SourceFingerprint,
    ) -> Result<Option<IngestRecord>, StoreError> {
        let connection = self.lock_connection()?;
        let raw = connection
            .query_row(
                "SELECT team_name, recipient_agent, source_path, source_fingerprint, message_key, imported_at FROM inbox_ingest WHERE team_name = ?1 AND recipient_agent = ?2 AND source_fingerprint = ?3",
                (team_name.as_str(), recipient_agent.as_str(), source_fingerprint.as_str()),
                |row| {
                    Ok(RawIngestRow {
                        team_name: row.get(0)?,
                        recipient_agent: row.get(1)?,
                        source_path: row.get(2)?,
                        source_fingerprint: row.get(3)?,
                        message_key: row.get(4)?,
                        imported_at: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(|error| classify_store_error(error, "failed to load ingest row"))?;
        raw.map(convert_ingest_row).transpose()
    }

    fn record_pending_export(&self, export: &PendingExportRecord) -> Result<(), StoreError> {
        let connection = self.lock_connection()?;
        connection
            .execute(
                r#"
                INSERT INTO pending_exports (
                    message_key,
                    export_target_team,
                    export_target_agent,
                    recipient_pane_id,
                    attempt_count,
                    next_attempt_at,
                    expires_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ON CONFLICT(message_key) DO UPDATE SET
                    export_target_team = excluded.export_target_team,
                    export_target_agent = excluded.export_target_agent,
                    recipient_pane_id = excluded.recipient_pane_id,
                    attempt_count = excluded.attempt_count,
                    next_attempt_at = excluded.next_attempt_at,
                    expires_at = excluded.expires_at
                "#,
                (
                    export.message_key.as_str(),
                    export.export_target_team.as_str(),
                    export.export_target_agent.as_str(),
                    export.recipient_pane_id.as_ref().map(ToString::to_string),
                    i64::from(export.attempt_count),
                    export.next_attempt_at.to_string(),
                    export.expires_at.to_string(),
                ),
            )
            .map_err(|error| classify_store_error(error, "failed to record pending export"))?;
        Ok(())
    }

    fn remove_pending_export(&self, message_key: &MessageKey) -> Result<(), StoreError> {
        let connection = self.lock_connection()?;
        connection
            .execute(
                "DELETE FROM pending_exports WHERE message_key = ?1",
                [message_key.as_str()],
            )
            .map_err(|error| classify_store_error(error, "failed to remove pending export"))?;
        Ok(())
    }
}

impl TaskStore for RusqliteStore {
    fn upsert_task(&self, task: &TaskRecord) -> Result<InsertOutcome<TaskRecord>, StoreError> {
        let connection = self.lock_connection()?;
        connection
            .execute(
                r#"
                INSERT INTO tasks (
                    task_id,
                    message_key,
                    status,
                    created_at,
                    acknowledged_at,
                    metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ON CONFLICT(task_id) DO UPDATE SET
                    message_key = excluded.message_key,
                    status = excluded.status,
                    created_at = excluded.created_at,
                    acknowledged_at = excluded.acknowledged_at,
                    metadata_json = excluded.metadata_json
                "#,
                (
                    task.task_id.as_str(),
                    task.message_key.as_str(),
                    task.status.as_str(),
                    task.created_at.to_string(),
                    task.acknowledged_at.as_ref().map(ToString::to_string),
                    task.metadata_json.as_deref(),
                ),
            )
            .map_err(|error| classify_store_error(error, "failed to upsert task row"))?;
        Ok(InsertOutcome::Inserted(task.clone()))
    }

    fn load_task(&self, task_id: &TaskId) -> Result<Option<TaskRecord>, StoreError> {
        let connection = self.lock_connection()?;
        let raw = connection
            .query_row(
                "SELECT task_id, message_key, status, created_at, acknowledged_at, metadata_json FROM tasks WHERE task_id = ?1",
                [task_id.as_str()],
                |row| {
                    Ok(RawTaskRow {
                        task_id: row.get(0)?,
                        message_key: row.get(1)?,
                        status: row.get(2)?,
                        created_at: row.get(3)?,
                        acknowledged_at: row.get(4)?,
                        metadata_json: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(|error| classify_store_error(error, "failed to load task row"))?;
        raw.map(convert_task_row).transpose()
    }
}

impl RosterStore for RusqliteStore {
    fn replace_roster(
        &self,
        team_name: &TeamName,
        members: &[RosterMemberRecord],
    ) -> Result<(), StoreError> {
        self.with_transaction(|transaction| {
            transaction
                .execute(
                    "DELETE FROM team_roster WHERE team_name = ?1",
                    [team_name.as_str()],
                )
                .map_err(|error| classify_store_error(error, "failed to clear existing roster"))?;
            for member in members {
                insert_roster_member_row(transaction, member).map_err(|error| {
                    classify_store_error(error, "failed to insert replacement roster member")
                })?;
            }
            Ok(())
        })
    }

    fn upsert_roster_member(
        &self,
        member: &RosterMemberRecord,
    ) -> Result<InsertOutcome<RosterMemberRecord>, StoreError> {
        let connection = self.lock_connection()?;
        connection
            .execute(
                r#"
                INSERT INTO team_roster (
                    team_name,
                    agent_name,
                    role,
                    transport_kind,
                    host_name,
                    recipient_pane_id,
                    pid,
                    metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(team_name, agent_name) DO UPDATE SET
                    role = excluded.role,
                    transport_kind = excluded.transport_kind,
                    host_name = excluded.host_name,
                    recipient_pane_id = excluded.recipient_pane_id,
                    pid = excluded.pid,
                    metadata_json = excluded.metadata_json
                "#,
                roster_member_params(member),
            )
            .map_err(|error| classify_store_error(error, "failed to upsert roster member"))?;
        Ok(InsertOutcome::Inserted(member.clone()))
    }

    fn load_roster(&self, team_name: &TeamName) -> Result<Vec<RosterMemberRecord>, StoreError> {
        let connection = self.lock_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT team_name, agent_name, role, transport_kind, host_name, recipient_pane_id, pid, metadata_json FROM team_roster WHERE team_name = ?1 ORDER BY agent_name",
            )
            .map_err(|error| classify_store_error(error, "failed to prepare roster load query"))?;
        let rows = statement
            .query_map([team_name.as_str()], |row| {
                Ok(RawRosterRow {
                    team_name: row.get(0)?,
                    agent_name: row.get(1)?,
                    role: row.get(2)?,
                    transport_kind: row.get(3)?,
                    host_name: row.get(4)?,
                    recipient_pane_id: row.get(5)?,
                    pid: row.get(6)?,
                    metadata_json: row.get(7)?,
                })
            })
            .map_err(|error| classify_store_error(error, "failed to query roster rows"))?;

        let mut members = Vec::new();
        for row in rows {
            let raw =
                row.map_err(|error| classify_store_error(error, "failed to read roster row"))?;
            members.push(convert_roster_row(raw)?);
        }
        Ok(members)
    }

    fn update_member_pid(
        &self,
        team_name: &TeamName,
        agent_name: &AgentName,
        update: PidUpdate,
    ) -> Result<Option<RosterMemberRecord>, StoreError> {
        let connection = self.lock_connection()?;
        connection
            .execute(
                "UPDATE team_roster SET pid = ?1 WHERE team_name = ?2 AND agent_name = ?3",
                (update.pid.get(), team_name.as_str(), agent_name.as_str()),
            )
            .map_err(|error| classify_store_error(error, "failed to update roster PID"))?;

        connection
            .query_row(
                "SELECT team_name, agent_name, role, transport_kind, host_name, recipient_pane_id, pid, metadata_json FROM team_roster WHERE team_name = ?1 AND agent_name = ?2",
                (team_name.as_str(), agent_name.as_str()),
                |row| {
                    Ok(RawRosterRow {
                        team_name: row.get(0)?,
                        agent_name: row.get(1)?,
                        role: row.get(2)?,
                        transport_kind: row.get(3)?,
                        host_name: row.get(4)?,
                        recipient_pane_id: row.get(5)?,
                        pid: row.get(6)?,
                        metadata_json: row.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(|error| classify_store_error(error, "failed to reload roster member after pid update"))?
            .map(convert_roster_row)
            .transpose()
    }
}

const MESSAGE_SELECT_COLUMNS: &str = "SELECT message_key, team_name, recipient_agent, sender_display, sender_canonical, sender_team, body, summary, created_at, source_kind, legacy_message_id, atm_message_id, raw_metadata_json FROM messages";

#[derive(Debug)]
struct RawMessageRow {
    message_key: String,
    team_name: String,
    recipient_agent: String,
    sender_display: String,
    sender_canonical: Option<String>,
    sender_team: Option<String>,
    body: String,
    summary: Option<String>,
    created_at: String,
    source_kind: String,
    legacy_message_id: Option<String>,
    atm_message_id: Option<String>,
    raw_metadata_json: Option<String>,
}

#[derive(Debug)]
struct RawAckStateRow {
    message_key: String,
    pending_ack_at: Option<String>,
    acknowledged_at: Option<String>,
    ack_reply_message_key: Option<String>,
    ack_reply_team: Option<String>,
    ack_reply_agent: Option<String>,
}

#[derive(Debug)]
struct RawVisibilityRow {
    message_key: String,
    read_at: Option<String>,
    cleared_at: Option<String>,
}

#[derive(Debug)]
struct RawIngestRow {
    team_name: String,
    recipient_agent: String,
    source_path: String,
    source_fingerprint: String,
    message_key: String,
    imported_at: String,
}

#[derive(Debug)]
struct RawTaskRow {
    task_id: String,
    message_key: String,
    status: String,
    created_at: String,
    acknowledged_at: Option<String>,
    metadata_json: Option<String>,
}

#[derive(Debug)]
struct RawRosterRow {
    team_name: String,
    agent_name: String,
    role: String,
    transport_kind: String,
    host_name: String,
    recipient_pane_id: Option<String>,
    pid: Option<i64>,
    metadata_json: Option<String>,
}

fn configure_connection(
    connection: &Connection,
    busy_timeout_ms: BusyTimeoutMs,
) -> Result<(), StoreError> {
    connection
        .busy_timeout(Duration::from_millis(u64::from(busy_timeout_ms.get())))
        .map_err(|error| {
            StoreError::bootstrap("failed to set SQLite busy timeout").with_source(error)
        })?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(|error| {
            StoreError::bootstrap("failed to enable SQLite WAL mode").with_source(error)
        })?;
    connection
        .pragma_update(None, "foreign_keys", 1)
        .map_err(|error| {
            StoreError::bootstrap("failed to enable SQLite foreign_keys").with_source(error)
        })?;
    Ok(())
}

fn bootstrap_schema(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection.transaction().map_err(|error| {
        StoreError::bootstrap("failed to start schema bootstrap transaction").with_source(error)
    })?;
    transaction.execute_batch(SCHEMA_SQL).map_err(|error| {
        StoreError::bootstrap("failed to apply SQLite schema bootstrap").with_source(error)
    })?;
    transaction
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|error| {
            StoreError::migration("failed to persist SQLite schema version").with_source(error)
        })?;
    transaction.commit().map_err(|error| {
        StoreError::bootstrap("failed to commit SQLite schema bootstrap").with_source(error)
    })?;
    Ok(())
}

fn query_user_version(connection: &Connection) -> Result<i64, StoreError> {
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| {
            StoreError::query("failed to query SQLite user_version").with_source(error)
        })
}

fn query_journal_mode(connection: &Connection) -> Result<String, StoreError> {
    connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .map_err(|error| {
            StoreError::query("failed to query SQLite journal_mode").with_source(error)
        })
}

fn query_foreign_keys(connection: &Connection) -> Result<bool, StoreError> {
    let enabled: i64 = connection
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .map_err(|error| {
            StoreError::query("failed to query SQLite foreign_keys").with_source(error)
        })?;
    Ok(enabled == 1)
}

fn insert_message_row(
    connection: &Connection,
    message: &StoredMessageRecord,
) -> rusqlite::Result<()> {
    connection.execute(
        r#"
        INSERT INTO messages (
            message_key,
            team_name,
            recipient_agent,
            sender_display,
            sender_canonical,
            sender_team,
            body,
            summary,
            created_at,
            source_kind,
            legacy_message_id,
            atm_message_id,
            raw_metadata_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        "#,
        (
            message.message_key.as_str(),
            message.team_name.as_str(),
            message.recipient_agent.as_str(),
            message.sender_display.as_str(),
            message.sender_canonical.as_ref().map(ToString::to_string),
            message.sender_team.as_ref().map(ToString::to_string),
            message.body.as_str(),
            message.summary.as_deref(),
            message.created_at.to_string(),
            message.source_kind.as_str(),
            message.legacy_message_id.as_ref().map(ToString::to_string),
            message.atm_message_id.as_ref().map(ToString::to_string),
            message.raw_metadata_json.as_deref(),
        ),
    )?;
    Ok(())
}

fn insert_roster_member_row(
    connection: &Connection,
    member: &RosterMemberRecord,
) -> rusqlite::Result<()> {
    connection.execute(
        r#"
        INSERT INTO team_roster (
            team_name,
            agent_name,
            role,
            transport_kind,
            host_name,
            recipient_pane_id,
            pid,
            metadata_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        roster_member_params(member),
    )?;
    Ok(())
}

fn roster_member_params(
    member: &RosterMemberRecord,
) -> (
    &str,
    &str,
    &str,
    &str,
    &str,
    Option<String>,
    Option<i64>,
    Option<&str>,
) {
    (
        member.team_name.as_str(),
        member.agent_name.as_str(),
        member.role.as_str(),
        member.transport_kind.as_str(),
        member.host_name.as_str(),
        member.recipient_pane_id.as_ref().map(ToString::to_string),
        member.pid.map(ProcessId::get),
        member.metadata_json.as_deref(),
    )
}

fn classify_store_error(error: rusqlite::Error, context: &str) -> StoreError {
    if let rusqlite::Error::SqliteFailure(code, message) = &error {
        if code.code == rusqlite::ErrorCode::DatabaseBusy
            || code.code == rusqlite::ErrorCode::DatabaseLocked
        {
            return StoreError::busy(format!(
                "{context}: {}",
                message.as_deref().unwrap_or("database busy")
            ))
            .with_source(error);
        }
        if code.code == rusqlite::ErrorCode::ConstraintViolation
            || message
                .as_deref()
                .unwrap_or_default()
                .contains("constraint failed")
        {
            return StoreError::constraint(format!(
                "{context}: {}",
                message.as_deref().unwrap_or("constraint violation")
            ))
            .with_source(error);
        }
    }
    StoreError::query(format!("{context}: {error}")).with_source(error)
}

fn classify_message_duplicate(
    error: &rusqlite::Error,
    message: &StoredMessageRecord,
) -> Option<StoreDuplicateIdentity> {
    let detail = sqlite_error_detail(error)?;
    if detail.contains("messages.message_key") {
        return Some(StoreDuplicateIdentity::MessageKey(
            message.message_key.clone(),
        ));
    }
    if detail.contains("messages.legacy_message_id") {
        return message
            .legacy_message_id
            .map(StoreDuplicateIdentity::LegacyMessageId);
    }
    if detail.contains("messages.atm_message_id") {
        return message
            .atm_message_id
            .map(StoreDuplicateIdentity::AtmMessageId);
    }
    None
}

fn classify_ingest_duplicate(
    error: &rusqlite::Error,
    ingest: &IngestRecord,
) -> Option<StoreDuplicateIdentity> {
    let detail = sqlite_error_detail(error)?;
    if detail.contains("inbox_ingest.team_name")
        || detail.contains("inbox_ingest.recipient_agent")
        || detail.contains("inbox_ingest.source_fingerprint")
    {
        return Some(StoreDuplicateIdentity::IngestFingerprint {
            team_name: ingest.team_name.clone(),
            recipient_agent: ingest.recipient_agent.clone(),
            source_fingerprint: ingest.source_fingerprint.clone(),
        });
    }
    None
}

fn sqlite_error_detail(error: &rusqlite::Error) -> Option<&str> {
    match error {
        rusqlite::Error::SqliteFailure(_, Some(message)) => Some(message.as_str()),
        _ => None,
    }
}

fn convert_message_row(raw: RawMessageRow) -> Result<StoredMessageRecord, StoreError> {
    Ok(StoredMessageRecord {
        message_key: parse_required(raw.message_key, "message_key")?,
        team_name: parse_required(raw.team_name, "team_name")?,
        recipient_agent: parse_required(raw.recipient_agent, "recipient_agent")?,
        sender_display: raw.sender_display,
        sender_canonical: parse_optional(raw.sender_canonical, "sender_canonical")?,
        sender_team: parse_optional(raw.sender_team, "sender_team")?,
        body: raw.body,
        summary: raw.summary,
        created_at: parse_required(raw.created_at, "created_at")?,
        source_kind: parse_message_source_kind(raw.source_kind)?,
        legacy_message_id: parse_optional(raw.legacy_message_id, "legacy_message_id")?,
        atm_message_id: parse_optional(raw.atm_message_id, "atm_message_id")?,
        raw_metadata_json: raw.raw_metadata_json,
    })
}

fn convert_ack_state_row(raw: RawAckStateRow) -> Result<AckStateRecord, StoreError> {
    Ok(AckStateRecord {
        message_key: parse_required(raw.message_key, "message_key")?,
        pending_ack_at: parse_optional(raw.pending_ack_at, "pending_ack_at")?,
        acknowledged_at: parse_optional(raw.acknowledged_at, "acknowledged_at")?,
        ack_reply_message_key: parse_optional(raw.ack_reply_message_key, "ack_reply_message_key")?,
        ack_reply_team: parse_optional(raw.ack_reply_team, "ack_reply_team")?,
        ack_reply_agent: parse_optional(raw.ack_reply_agent, "ack_reply_agent")?,
    })
}

fn convert_visibility_row(raw: RawVisibilityRow) -> Result<VisibilityStateRecord, StoreError> {
    Ok(VisibilityStateRecord {
        message_key: parse_required(raw.message_key, "message_key")?,
        read_at: parse_optional(raw.read_at, "read_at")?,
        cleared_at: parse_optional(raw.cleared_at, "cleared_at")?,
    })
}

fn convert_ingest_row(raw: RawIngestRow) -> Result<IngestRecord, StoreError> {
    Ok(IngestRecord {
        team_name: parse_required(raw.team_name, "team_name")?,
        recipient_agent: parse_required(raw.recipient_agent, "recipient_agent")?,
        source_path: PathBuf::from(raw.source_path),
        source_fingerprint: parse_required(raw.source_fingerprint, "source_fingerprint")?,
        message_key: parse_required(raw.message_key, "message_key")?,
        imported_at: parse_required(raw.imported_at, "imported_at")?,
    })
}

fn convert_task_row(raw: RawTaskRow) -> Result<TaskRecord, StoreError> {
    Ok(TaskRecord {
        task_id: parse_required(raw.task_id, "task_id")?,
        message_key: parse_required(raw.message_key, "message_key")?,
        status: parse_task_status(raw.status)?,
        created_at: parse_required(raw.created_at, "created_at")?,
        acknowledged_at: parse_optional(raw.acknowledged_at, "acknowledged_at")?,
        metadata_json: raw.metadata_json,
    })
}

fn convert_roster_row(raw: RawRosterRow) -> Result<RosterMemberRecord, StoreError> {
    Ok(RosterMemberRecord {
        team_name: parse_required(raw.team_name, "team_name")?,
        agent_name: parse_required(raw.agent_name, "agent_name")?,
        role: raw.role,
        transport_kind: raw.transport_kind,
        host_name: parse_required(raw.host_name, "host_name")?,
        recipient_pane_id: parse_optional(raw.recipient_pane_id, "recipient_pane_id")?,
        pid: raw
            .pid
            .map(|value| ProcessId::new(value).map_err(|error| invalid_store_data("pid", error)))
            .transpose()?,
        metadata_json: raw.metadata_json,
    })
}

fn parse_message_source_kind(value: String) -> Result<MessageSourceKind, StoreError> {
    match value.as_str() {
        "atm" => Ok(MessageSourceKind::Atm),
        "legacy" => Ok(MessageSourceKind::Legacy),
        "external" => Ok(MessageSourceKind::External),
        _ => Err(invalid_store_data("source_kind", "unsupported source kind")),
    }
}

fn parse_task_status(value: String) -> Result<TaskStatus, StoreError> {
    match value.as_str() {
        "pending_ack" => Ok(TaskStatus::PendingAck),
        "acknowledged" => Ok(TaskStatus::Acknowledged),
        _ => Err(invalid_store_data("task_status", "unsupported task status")),
    }
}

fn parse_required<T>(value: String, field: &str) -> Result<T, StoreError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value
        .parse::<T>()
        .map_err(|error| invalid_store_data(field, error))
}

fn parse_optional<T>(value: Option<String>, field: &str) -> Result<Option<T>, StoreError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value.map(|value| parse_required(value, field)).transpose()
}

fn invalid_store_data(field: &str, error: impl std::fmt::Display) -> StoreError {
    StoreError::query(format!("invalid store data for {field}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn team() -> TeamName {
        "atm-dev".parse().expect("team")
    }

    fn agent(name: &str) -> AgentName {
        name.parse().expect("agent")
    }

    fn message_at(index: u8) -> StoredMessageRecord {
        let legacy_message_id: LegacyMessageId =
            format!("00000000-0000-4000-8000-0000000000{index:02}")
                .parse()
                .expect("legacy id");
        let atm_message_id: AtmMessageId = format!("01ARZ3NDEKTSV4RRFFQ69G5F{index:02}")
            .parse()
            .expect("atm id");
        StoredMessageRecord {
            message_key: MessageKey::from_atm_message_id(atm_message_id),
            team_name: team(),
            recipient_agent: agent("team-lead"),
            sender_display: "arch-ctm".to_string(),
            sender_canonical: Some(agent("arch-ctm")),
            sender_team: Some(team()),
            body: format!("body-{index}"),
            summary: Some(format!("summary-{index}")),
            created_at: "2026-05-02T20:00:00Z".parse().expect("timestamp"),
            source_kind: MessageSourceKind::Atm,
            legacy_message_id: Some(legacy_message_id),
            atm_message_id: Some(atm_message_id),
            raw_metadata_json: Some("{\"atm\":true}".to_string()),
        }
    }

    fn ingest_record(message_key: &MessageKey) -> IngestRecord {
        IngestRecord {
            team_name: team(),
            recipient_agent: agent("team-lead"),
            source_path: PathBuf::from("/tmp/team-lead.json"),
            source_fingerprint: "sha256-team-lead-001".parse().expect("fingerprint"),
            message_key: message_key.clone(),
            imported_at: "2026-05-02T20:00:05Z".parse().expect("timestamp"),
        }
    }

    fn roster_member(name: &str, pane_id: Option<&str>, pid: Option<i64>) -> RosterMemberRecord {
        RosterMemberRecord {
            team_name: team(),
            agent_name: agent(name),
            role: "member".to_string(),
            transport_kind: "claude".to_string(),
            host_name: "local-host".parse().expect("host"),
            recipient_pane_id: pane_id.map(|value| value.parse().expect("pane")),
            pid: pid.map(|value| ProcessId::new(value).expect("pid")),
            metadata_json: Some("{\"provider\":\"claude\"}".to_string()),
        }
    }

    #[test]
    fn opens_under_team_state_mail_db() {
        let tempdir = TempDir::new().expect("tempdir");
        let store = RusqliteStore::open_for_team_home(tempdir.path(), &team()).expect("open store");
        assert_eq!(
            store.database_path,
            tempdir
                .path()
                .join(".claude")
                .join("teams")
                .join("atm-dev")
                .join(".atm-state")
                .join("mail.db")
        );
    }

    #[test]
    fn bootstrap_is_idempotent_and_reports_wal() {
        let tempdir = TempDir::new().expect("tempdir");
        let db_path = tempdir.path().join("mail.db");

        let first = RusqliteStore::open_path(&db_path).expect("first bootstrap");
        let second = RusqliteStore::open_path(&db_path).expect("second bootstrap");

        let first_report = first.bootstrap_report().expect("first report");
        let second_report = second.bootstrap_report().expect("second report");

        assert_eq!(first_report.schema_version, SCHEMA_VERSION);
        assert_eq!(second_report.schema_version, SCHEMA_VERSION);
        assert!(first_report.wal_enabled);
        assert!(first_report.foreign_keys_enabled);
    }

    #[test]
    fn insert_message_enforces_unique_identities() {
        let tempdir = TempDir::new().expect("tempdir");
        let store = RusqliteStore::open_path(tempdir.path().join("mail.db")).expect("open store");

        let first = message_at(1);
        let second_same_key_atm = AtmMessageId::new();
        let second_same_key = StoredMessageRecord {
            body: "updated".to_string(),
            legacy_message_id: Some(
                "00000000-0000-4000-8000-000000000099"
                    .parse()
                    .expect("legacy id"),
            ),
            atm_message_id: Some(second_same_key_atm),
            message_key: first.message_key.clone(),
            ..first.clone()
        };
        let second_atm_message_id = AtmMessageId::new();
        let second_same_legacy = StoredMessageRecord {
            message_key: MessageKey::from_atm_message_id(second_atm_message_id),
            atm_message_id: Some(second_atm_message_id),
            ..first.clone()
        };

        match store.insert_message(&first).expect("insert first") {
            InsertOutcome::Inserted(_) => {}
            InsertOutcome::Duplicate(_) => panic!("first insert unexpectedly duplicate"),
        }

        match store
            .insert_message(&second_same_key)
            .expect("duplicate key result")
        {
            InsertOutcome::Duplicate(StoreDuplicateIdentity::MessageKey(key)) => {
                assert_eq!(key, first.message_key)
            }
            other => panic!("unexpected duplicate outcome: {other:?}"),
        }

        match store
            .insert_message(&second_same_legacy)
            .expect("duplicate legacy result")
        {
            InsertOutcome::Duplicate(StoreDuplicateIdentity::LegacyMessageId(id)) => {
                assert_eq!(id, first.legacy_message_id.expect("legacy id"))
            }
            other => panic!("unexpected duplicate outcome: {other:?}"),
        }
    }

    #[test]
    fn insert_message_batch_rolls_back_on_mid_operation_failure() {
        let tempdir = TempDir::new().expect("tempdir");
        let store = RusqliteStore::open_path(tempdir.path().join("mail.db")).expect("open store");

        let first = message_at(1);
        let second = StoredMessageRecord {
            message_key: MessageKey::from_atm_message_id(
                "01ARZ3NDEKTSV4RRFFQ69G5FBB".parse().expect("atm id"),
            ),
            atm_message_id: Some("01ARZ3NDEKTSV4RRFFQ69G5FBB".parse().expect("atm id")),
            ..first.clone()
        };

        let error = store
            .insert_message_batch(&[first.clone(), second])
            .expect_err("duplicate batch should fail");
        assert_eq!(error.kind, atm_core::store::StoreErrorKind::Constraint);
        assert_eq!(
            error.code,
            atm_core::error_codes::AtmErrorCode::StoreConstraintViolation
        );
        assert!(
            store
                .load_message(&first.message_key)
                .expect("reload after failed batch")
                .is_none()
        );
    }

    #[test]
    fn create_read_and_update_store_rows() {
        let tempdir = TempDir::new().expect("tempdir");
        let store = RusqliteStore::open_path(tempdir.path().join("mail.db")).expect("open store");
        let message = message_at(1);

        store.insert_message(&message).expect("insert message");

        let loaded = store
            .load_message(&message.message_key)
            .expect("load message")
            .expect("stored message");
        assert_eq!(loaded.message_key, message.message_key);
        assert_eq!(
            store
                .load_message_by_legacy_id(&message.legacy_message_id.expect("legacy id"))
                .expect("load by legacy")
                .expect("legacy row")
                .message_key,
            message.message_key
        );
        assert_eq!(
            store
                .load_message_by_atm_id(&message.atm_message_id.expect("atm id"))
                .expect("load by atm")
                .expect("atm row")
                .message_key,
            message.message_key
        );

        let ack_state = AckStateRecord {
            message_key: message.message_key.clone(),
            pending_ack_at: Some("2026-05-02T20:00:10Z".parse().expect("timestamp")),
            acknowledged_at: Some("2026-05-02T20:00:20Z".parse().expect("timestamp")),
            ack_reply_message_key: Some("ext:ack-reply-1".parse().expect("message key")),
            ack_reply_team: Some(team()),
            ack_reply_agent: Some(agent("team-lead")),
        };
        store.upsert_ack_state(&ack_state).expect("upsert ack");
        assert_eq!(
            store
                .load_ack_state(&message.message_key)
                .expect("load ack")
                .expect("ack row"),
            ack_state
        );

        let visibility = VisibilityStateRecord {
            message_key: message.message_key.clone(),
            read_at: Some("2026-05-02T20:00:30Z".parse().expect("timestamp")),
            cleared_at: None,
        };
        store
            .upsert_visibility(&visibility)
            .expect("upsert visibility");
        assert_eq!(
            store
                .load_visibility(&message.message_key)
                .expect("load visibility")
                .expect("visibility row"),
            visibility
        );

        let ingest = ingest_record(&message.message_key);
        match store.record_ingest(&ingest).expect("record ingest") {
            InsertOutcome::Inserted(_) => {}
            InsertOutcome::Duplicate(_) => panic!("first ingest unexpectedly duplicate"),
        }
        assert_eq!(
            store
                .load_ingest(
                    &ingest.team_name,
                    &ingest.recipient_agent,
                    &ingest.source_fingerprint
                )
                .expect("load ingest")
                .expect("ingest row"),
            ingest
        );

        let task = TaskRecord {
            task_id: "TASK-1".parse().expect("task id"),
            message_key: message.message_key.clone(),
            status: TaskStatus::PendingAck,
            created_at: "2026-05-02T20:00:40Z".parse().expect("timestamp"),
            acknowledged_at: None,
            metadata_json: Some("{\"priority\":\"high\"}".to_string()),
        };
        store.upsert_task(&task).expect("upsert task");
        assert_eq!(
            store
                .load_task(&task.task_id)
                .expect("load task")
                .expect("task row"),
            task
        );
    }

    #[test]
    fn roster_replace_update_and_pid_round_trip() {
        let tempdir = TempDir::new().expect("tempdir");
        let store = RusqliteStore::open_path(tempdir.path().join("mail.db")).expect("open store");

        let arch = roster_member("arch-ctm", Some("%1"), Some(1001));
        let quality = roster_member("quality-mgr", None, None);
        store
            .replace_roster(&team(), &[arch.clone(), quality.clone()])
            .expect("replace roster");
        let loaded = store.load_roster(&team()).expect("load roster");
        assert_eq!(loaded, vec![arch.clone(), quality.clone()]);

        let updated_quality = RosterMemberRecord {
            recipient_pane_id: Some("%2".parse().expect("pane")),
            ..quality.clone()
        };
        store
            .upsert_roster_member(&updated_quality)
            .expect("upsert roster member");
        let pid_updated = store
            .update_member_pid(
                &team(),
                &updated_quality.agent_name,
                PidUpdate {
                    pid: ProcessId::new(2024).expect("pid"),
                },
            )
            .expect("update pid")
            .expect("updated member");

        assert_eq!(pid_updated.agent_name, updated_quality.agent_name);
        assert_eq!(
            pid_updated.recipient_pane_id,
            updated_quality.recipient_pane_id
        );
        assert_eq!(pid_updated.pid, Some(ProcessId::new(2024).expect("pid")));
    }

    #[test]
    fn team_roster_schema_keeps_recipient_pane_nullable() {
        let tempdir = TempDir::new().expect("tempdir");
        let store = RusqliteStore::open_path(tempdir.path().join("mail.db")).expect("open store");
        let connection = store.lock_connection().expect("lock");
        let mut statement = connection
            .prepare("PRAGMA table_info(team_roster)")
            .expect("pragma table_info");
        let mut rows = statement.query([]).expect("query table_info");

        let mut saw_recipient_pane = false;
        while let Some(row) = rows.next().expect("next row") {
            let name: String = row.get(1).expect("column name");
            if name == "recipient_pane_id" {
                let not_null: i64 = row.get(3).expect("not null flag");
                saw_recipient_pane = true;
                assert_eq!(not_null, 0);
            }
        }
        assert!(saw_recipient_pane);
    }

    #[test]
    fn store_errors_stay_discriminated() {
        let tempdir = TempDir::new().expect("tempdir");
        let error =
            RusqliteStore::open_path(tempdir.path()).expect_err("directory path should fail");
        assert_eq!(error.kind, atm_core::store::StoreErrorKind::Open);
        assert_eq!(
            error.code,
            atm_core::error_codes::AtmErrorCode::StoreOpenFailed
        );
    }
}
