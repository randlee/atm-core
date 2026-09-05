use crate::mail_messages_schema::{mail_messages_index_ddl, mail_messages_table_ddl};
pub(crate) use crate::shared_db_reader_lanes::SharedDb;
use crate::writer::{WriteOp, WriteOpResult, validate_upsert_message_request};
use atm_storage::AsyncMailboxReader;
use atm_storage::TemplateMessageAdmission;
use atm_storage::contract::{
    AcknowledgementCommit, AcknowledgementReplyBuilder, AcknowledgementSource, Message,
};
use atm_storage::error::AtmError;
use atm_storage::schema::ThreadMode;
#[cfg(test)]
use rusqlite::OpenFlags;
use rusqlite::{Connection, Error as RusqliteError, TransactionBehavior};
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(test)]
use std::sync::LazyLock;
#[cfg(test)]
use std::sync::Mutex;
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

// Search projection writes touch several B-trees and FTS segments inside the
// sole durable writer transaction.  The SQLite default is only 2 MiB, which
// churns cache pages under a concurrent admission burst.  This setting applies
// only to the one writer connection and bounds its page cache at 32 MiB.
const WRITER_CACHE_KIB: i64 = 32 * 1024;

#[cfg(test)]
pub(crate) static NEXT_IN_MEMORY_DB_ID: AtomicU64 = AtomicU64::new(1);
#[cfg(test)]
static OPENED_CONNECTIONS: LazyLock<Mutex<std::collections::HashMap<String, usize>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

pub(crate) const DB_MIGRATIONS: &str = concat!(
    mail_messages_table_ddl!("CREATE TABLE IF NOT EXISTS mail_messages"),
    r#"
CREATE TABLE IF NOT EXISTS mail_message_states (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    message_key TEXT NOT NULL,
    read INTEGER NOT NULL DEFAULT 0 CHECK(read IN (0, 1)),
    pending_ack_at TEXT NULL,
    acknowledged_at TEXT NULL,
    expires_at TEXT NULL,
    deleted_at TEXT NULL,
    updated_at TEXT NULL,
    nudge_pending_at TEXT NULL,
    nudge_attempts INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (team, agent, message_key),
    FOREIGN KEY (team, agent, message_key)
        REFERENCES mail_messages(team, agent, message_key)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS mail_seen_watermarks (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    watermark TEXT NOT NULL,
    PRIMARY KEY (team, agent)
);

CREATE TABLE IF NOT EXISTS team_roster (
    team_name TEXT NOT NULL,
    agent_name TEXT NOT NULL,
    member_kind TEXT NOT NULL CHECK(member_kind IN ('permanent', 'ephemeral')),
    harness TEXT NOT NULL CHECK(harness IN ('claude-code', 'codex-cli', 'gemini-cli', 'opencode', 'hermes', 'python-graft')),
    agent_type TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL DEFAULT '',
    metadata_json TEXT NOT NULL DEFAULT '{}',
    source TEXT,
    recipient_pane_id TEXT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (team_name, agent_name)
);

CREATE TABLE IF NOT EXISTS team_nudge_template_overrides (
    team_name TEXT NOT NULL,
    template_kind TEXT NOT NULL
        CHECK(template_kind IN (
            'delivery',
            'delivery_ack',
            'queue',
            'queue_ack',
            'task',
            'acknowledge',
            'acknowledge_task'
        )),
    mode TEXT NOT NULL DEFAULT 'override'
        CHECK(mode IN ('override', 'disabled')),
    template_body TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (team_name, template_kind)
);

CREATE TABLE IF NOT EXISTS peer_https_interfaces (
    bind_addr TEXT NOT NULL PRIMARY KEY,
    advertise_host TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK(enabled IN (0, 1))
);

CREATE TABLE IF NOT EXISTS peer_local_certificate (
    singleton INTEGER NOT NULL PRIMARY KEY CHECK(singleton = 1),
    fingerprint TEXT NOT NULL,
    private_key_ref TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS peer_trusted_peers (
    host TEXT NOT NULL PRIMARY KEY,
    fingerprint TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK(enabled IN (0, 1)),
    https_port INTEGER NOT NULL DEFAULT 43101 CHECK(https_port BETWEEN 1 AND 65535)
);

CREATE INDEX IF NOT EXISTS idx_mail_message_states_mailbox
    ON mail_message_states(team, agent);

CREATE INDEX IF NOT EXISTS idx_team_roster_team_name
    ON team_roster(team_name);

CREATE INDEX IF NOT EXISTS idx_team_nudge_template_overrides_team_name
    ON team_nudge_template_overrides(team_name);

CREATE INDEX IF NOT EXISTS idx_peer_https_interfaces_enabled
    ON peer_https_interfaces(enabled);

CREATE INDEX IF NOT EXISTS idx_peer_trusted_peers_enabled
    ON peer_trusted_peers(enabled);

-- AK.2 retires the worker-only reconciliation policy.  `IF EXISTS` makes the
-- migration safe for both historical databases and fresh installs.
DROP TABLE IF EXISTS peer_sync_policies;
"#,
    mail_messages_index_ddl!(),
);
// `team_roster` is the single canonical durable roster truth. Runtime pid
// continuity is transient daemon-owned state and must not be persisted here.

pub(crate) type SqliteConnection = Connection;

#[derive(Debug, Clone)]
pub(crate) enum SharedDbTarget {
    Path(PathBuf),
    #[cfg(test)]
    InMemory {
        uri: String,
    },
}

impl SharedDbTarget {
    pub(crate) fn display(&self) -> String {
        match self {
            Self::Path(path) => path.display().to_string(),
            #[cfg(test)]
            Self::InMemory { uri } => uri.clone(),
        }
    }
}

/// Test-only instrumentation at the concrete SQLite open sites. Entries are
/// keyed by the target so parallel tests cannot contaminate each other's
/// connection-budget evidence.
#[cfg(test)]
pub(crate) fn reset_opened_connection_count(target: &SharedDbTarget) {
    OPENED_CONNECTIONS
        .lock()
        .expect("opened connection counter lock")
        .insert(target.display(), 0);
}

#[cfg(test)]
pub(crate) fn record_opened_connection(target: &SharedDbTarget) {
    let mut counters = OPENED_CONNECTIONS
        .lock()
        .expect("opened connection counter lock");
    if let Some(count) = counters.get_mut(&target.display()) {
        *count += 1;
    }
}

#[cfg(test)]
pub(crate) fn opened_connection_count(target: &SharedDbTarget) -> usize {
    OPENED_CONNECTIONS
        .lock()
        .expect("opened connection counter lock")
        .get(&target.display())
        .copied()
        .unwrap_or_default()
}

impl SharedDb {
    /// Call only from backend-owned blocking code paths.
    ///
    /// Accepted risk: this is enforced as a crate-internal contract rather
    /// than a runtime assert because `SharedDb` is only called from owned
    /// blocking code paths inside `atm-storage-rusqlite`.
    pub(crate) fn with_connection<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T, AtmError>,
    ) -> Result<T, AtmError> {
        debug_assert_blocking_only("SharedDb::with_connection");
        let mut lease = crate::control_path_pool::checkout(&self.control_path)?;
        let outcome = operation(lease.connection());
        if outcome.is_ok() {
            lease.park();
        }
        outcome
    }

    /// Call only from backend-owned blocking code paths.
    ///
    /// Accepted risk: this is enforced as a crate-internal contract rather
    /// than a runtime assert because `SharedDb` is only called from owned
    /// blocking code paths inside `atm-storage-rusqlite`.
    pub(crate) fn with_transaction<T>(
        &self,
        operation: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T, AtmError>,
    ) -> Result<T, AtmError> {
        debug_assert_blocking_only("SharedDb::with_transaction");
        self.with_connection(|connection| {
            // Acquire the SQLite writer lock up front so concurrent write paths
            // wait under the configured busy_timeout instead of failing during
            // deferred lock escalation on slower Windows schedulers.
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| {
                    sqlite_error(
                        self.target.as_ref(),
                        "failed to open sqlite immediate transaction",
                        error,
                    )
                })?;
            let value = operation(&transaction)?;
            transaction.commit().map_err(|error| {
                sqlite_error(
                    self.target.as_ref(),
                    "failed to commit sqlite transaction",
                    error,
                )
            })?;
            Ok(value)
        })
    }

    pub(crate) fn submit_upsert_message(&self, record: Message) -> Result<bool, AtmError> {
        validate_upsert_message_request(&record)?;
        let result = self
            .writer
            .submit(WriteOp::UpsertMessage(Box::new(record)))?;
        match result {
            WriteOpResult::UpsertMessage { inserted, .. } => Ok(inserted),
            WriteOpResult::ReadDisplayStateApplied
            | WriteOpResult::UpsertMessages
            | WriteOpResult::Acknowledged(_)
            | WriteOpResult::TemplateRegistration(_)
            | WriteOpResult::DecomposedMessageAdmission(_)
            | WriteOpResult::TemplateMessageAdmission { .. } => Err(AtmError::daemon_unavailable(
                "sqlite writer returned the wrong result for message upsert",
            )),
        }
    }

    /// Submits a feature-owned writer operation without exposing the writer
    /// handle or its transaction lifecycle outside this state root.
    pub(crate) fn submit_writer_op(&self, operation: WriteOp) -> Result<WriteOpResult, AtmError> {
        self.writer.submit(operation)
    }

    pub(crate) fn submit_search(
        &self,
        query: atm_storage::MessageSearchQuery,
    ) -> Result<atm_storage::MessageSearchPage, AtmError> {
        self.search_reader.submit(query)
    }

    pub(crate) async fn submit_search_async(
        &self,
        query: atm_storage::MessageSearchQuery,
        deadline: std::time::Duration,
    ) -> Result<atm_storage::MessageSearchPage, AtmError> {
        self.search_reader.submit_async(query, deadline).await
    }

    #[cfg(test)]
    pub(crate) async fn submit_expired_search_for_test(
        &self,
        query: atm_storage::MessageSearchQuery,
    ) -> Result<atm_storage::MessageSearchPage, AtmError> {
        self.search_reader.submit_expired_for_test(query).await
    }

    pub(crate) async fn submit_upsert_message_async(
        &self,
        record: Message,
    ) -> Result<Option<Message>, AtmError> {
        validate_upsert_message_request(&record)?;
        match self
            .writer
            .submit_async(WriteOp::UpsertMessage(Box::new(record)))
            .await?
        {
            WriteOpResult::UpsertMessage { inserted: true, .. } => Ok(None),
            WriteOpResult::UpsertMessage {
                inserted: false,
                existing: Some(existing),
            } => Ok(Some(*existing)),
            WriteOpResult::UpsertMessage {
                inserted: false,
                existing: None,
            } => Err(AtmError::daemon_unavailable(
                "sqlite writer reported a duplicate without its retained record",
            )),
            WriteOpResult::ReadDisplayStateApplied
            | WriteOpResult::UpsertMessages
            | WriteOpResult::Acknowledged(_)
            | WriteOpResult::TemplateRegistration(_)
            | WriteOpResult::DecomposedMessageAdmission(_)
            | WriteOpResult::TemplateMessageAdmission { .. } => Err(AtmError::daemon_unavailable(
                "sqlite writer returned the wrong result for async message upsert",
            )),
        }
    }

    pub(crate) async fn submit_read_display_state_async(
        &self,
        mailbox: atm_storage::MailboxScope,
        message_ids: Vec<atm_storage::MessageKey>,
        seen_watermark: Option<atm_storage::IsoTimestamp>,
    ) -> Result<(), AtmError> {
        match self
            .writer
            .submit_async(WriteOp::ApplyReadDisplayState {
                mailbox,
                message_ids,
                seen_watermark,
            })
            .await?
        {
            WriteOpResult::ReadDisplayStateApplied => Ok(()),
            other => Err(AtmError::daemon_unavailable(format!(
                "sqlite writer returned the wrong result for async read display state: {other:?}"
            ))),
        }
    }

    pub(crate) async fn submit_template_message_admission_async(
        &self,
        admission: TemplateMessageAdmission,
    ) -> Result<Option<Message>, AtmError> {
        admission.validate()?;
        match self
            .writer
            .submit_async(WriteOp::AdmitTemplateMessage(Box::new(admission)))
            .await?
        {
            WriteOpResult::TemplateMessageAdmission { inserted: true, .. } => Ok(None),
            WriteOpResult::TemplateMessageAdmission {
                inserted: false,
                existing: Some(existing),
            } => Ok(Some(*existing)),
            WriteOpResult::TemplateMessageAdmission {
                inserted: false,
                existing: None,
            } => Err(AtmError::daemon_unavailable(
                "sqlite writer reported a duplicate template admission without its retained record",
            )),
            other => Err(AtmError::daemon_unavailable(format!(
                "sqlite writer returned the wrong result for async template message admission: {other:?}"
            ))),
        }
    }

    pub(crate) fn submit_upsert_messages_atomically(
        &self,
        records: Vec<Message>,
    ) -> Result<(), AtmError> {
        if records.is_empty() {
            return Ok(());
        }
        for record in &records {
            validate_upsert_message_request(record)?;
        }
        let result = self.writer.submit(WriteOp::UpsertMessages(records))?;
        match result {
            WriteOpResult::UpsertMessages => Ok(()),
            WriteOpResult::ReadDisplayStateApplied
            | WriteOpResult::UpsertMessage { .. }
            | WriteOpResult::Acknowledged(_)
            | WriteOpResult::TemplateRegistration(_)
            | WriteOpResult::DecomposedMessageAdmission(_)
            | WriteOpResult::TemplateMessageAdmission { .. } => Err(AtmError::daemon_unavailable(
                "sqlite writer returned the wrong result for atomic message commit",
            )),
        }
    }

    pub(crate) fn submit_acknowledgement(
        &self,
        source: AcknowledgementSource,
        builder: std::sync::Arc<dyn AcknowledgementReplyBuilder>,
    ) -> Result<AcknowledgementCommit, AtmError> {
        match self
            .writer
            .submit(WriteOp::Acknowledge { source, builder })?
        {
            WriteOpResult::Acknowledged(commit) => Ok(*commit),
            WriteOpResult::ReadDisplayStateApplied
            | WriteOpResult::UpsertMessage { .. }
            | WriteOpResult::UpsertMessages
            | WriteOpResult::TemplateRegistration(_)
            | WriteOpResult::DecomposedMessageAdmission(_)
            | WriteOpResult::TemplateMessageAdmission { .. } => Err(AtmError::daemon_unavailable(
                "sqlite writer returned the wrong result for acknowledgement admission",
            )),
        }
    }

    pub(crate) async fn submit_acknowledgement_async(
        &self,
        source: AcknowledgementSource,
        builder: std::sync::Arc<dyn AcknowledgementReplyBuilder>,
    ) -> Result<AcknowledgementCommit, AtmError> {
        match self
            .writer
            .submit_async(WriteOp::Acknowledge { source, builder })
            .await?
        {
            WriteOpResult::Acknowledged(commit) => Ok(*commit),
            WriteOpResult::ReadDisplayStateApplied
            | WriteOpResult::UpsertMessage { .. }
            | WriteOpResult::UpsertMessages
            | WriteOpResult::TemplateRegistration(_)
            | WriteOpResult::DecomposedMessageAdmission(_)
            | WriteOpResult::TemplateMessageAdmission { .. } => Err(AtmError::daemon_unavailable(
                "sqlite writer returned the wrong result for async acknowledgement admission",
            )),
        }
    }

    pub(crate) fn mailbox_reader(&self) -> Arc<dyn AsyncMailboxReader + Send + Sync> {
        Arc::clone(&self.mailbox_reader)
    }

    pub(crate) fn error(&self, message: impl Into<String>, source: RusqliteError) -> AtmError {
        sqlite_error(self.target.as_ref(), message, source)
    }

    pub(crate) fn target_path(&self) -> Option<&PathBuf> {
        match self.target.as_ref() {
            SharedDbTarget::Path(path) => Some(path),
            #[cfg(test)]
            SharedDbTarget::InMemory { .. } => None,
        }
    }
}

impl std::fmt::Debug for SharedDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedDb")
            .field("target", &self.target.display())
            .finish()
    }
}

fn debug_assert_blocking_only(method: &str) {
    #[cfg(not(debug_assertions))]
    let _ = method;
    #[cfg(debug_assertions)]
    {
        let current_thread = std::thread::current();
        let thread_name = current_thread.name().unwrap_or("<unnamed>");
        debug_assert!(
            !thread_name.starts_with("tokio-runtime-worker"),
            "{method} must run from a blocking code path; enter spawn_blocking before borrowing sqlite state"
        );
    }
}

pub(crate) fn configure_connection(
    connection: &mut Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    // Bound SQLite lock waits so contention returns an actionable ATM timeout
    // instead of hanging an adapter thread indefinitely. This is derived
    // from `atm_storage::request_budget::SERVER_REQUEST_BUDGET` so a
    // writer-lock wait can never by itself consume the entire per-request
    // budget the Tokio/Axum server enforces around this call; a busy
    // timeout at or above that budget would mean a lock wait could never
    // succeed inside it.
    connection
        .busy_timeout(atm_storage::request_budget::SQLITE_BUSY_TIMEOUT)
        .map_err(|error| sqlite_error(target, "failed to configure sqlite busy timeout", error))?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|error| sqlite_error(target, "failed to enable sqlite foreign keys", error))?;
    Ok(())
}

/// WAL is durable database state, not per-connection request setup. Configure
/// it once when the sole writer owns startup; readers only need their local
/// timeout and foreign-key settings.
fn enable_write_ahead_log(
    connection: &mut Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    #[cfg(test)]
    let enable_wal = !matches!(target, SharedDbTarget::InMemory { .. });
    #[cfg(not(test))]
    let enable_wal = true;
    if enable_wal {
        // `PRAGMA journal_mode = WAL` returns the selected mode. Use a query
        // rather than Rusqlite's execute-only pragma helper, which correctly
        // rejects result-producing pragmas.
        let journal_mode = connection
            .query_row("PRAGMA journal_mode = WAL", [], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| {
                sqlite_error(target, "failed to enable sqlite wal journal mode", error)
            })?;
        if !journal_mode.eq_ignore_ascii_case("wal") {
            return Err(AtmError::mailbox_write(format!(
                "SQLite declined WAL journal mode for {} (reported {journal_mode:?})",
                target.display()
            )));
        }
    }
    Ok(())
}

pub(crate) fn open_connection_for_target(target: &SharedDbTarget) -> Result<Connection, AtmError> {
    let mut connection = match target {
        SharedDbTarget::Path(path) => {
            // Accepted risk: sqlite connection open is a process-init boundary
            // operation and may block on the host filesystem before runtime
            // request handling starts.
            Connection::open(path).map_err(|error| sqlite_open_error(target, error))?
        }
        #[cfg(test)]
        SharedDbTarget::InMemory { uri } => Connection::open_with_flags(
            uri,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )
        .map_err(|error| sqlite_open_error(target, error))?,
    };
    configure_connection(&mut connection, target)?;
    #[cfg(test)]
    record_opened_connection(target);
    Ok(connection)
}

/// Opens a connection which is physically read-only for durable databases and
/// configured defensively for the bounded reader lanes.
pub(crate) fn open_writer_connection_for_target(
    target: &SharedDbTarget,
) -> Result<Connection, AtmError> {
    let mut connection = open_connection_for_target(target)?;
    enable_write_ahead_log(&mut connection, target)?;
    connection
        .pragma_update(None, "cache_size", -WRITER_CACHE_KIB)
        .map_err(|error| sqlite_error(target, "failed to configure SQLite writer cache", error))?;
    Ok(connection)
}

pub(crate) fn ensure_schema(
    connection: &mut Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    ensure_mail_messages_message_id_compat(connection, target)?;
    connection
        .execute_batch(DB_MIGRATIONS)
        .map_err(|error| sqlite_error(target, "failed to initialize sqlite schema", error))?;
    ensure_mail_message_columns(connection, target)?;
    crate::template_catalog_schema::ensure_schema(connection, target)?;
    // Runs after every additive column migration so the rebuilt table can be
    // populated from a legacy table that already carries the current column
    // set, and before the search projections, which read `mail_messages`.
    crate::mail_messages_schema::ensure_mail_messages_message_text_nullable(connection, target)?;
    crate::search_schema::ensure_schema(connection, target)?;
    ensure_team_roster_columns(connection, target)?;
    crate::team_roster_schema::ensure_team_roster_harness_values(connection, target)?;
    crate::template_override_migration::ensure_team_nudge_template_override_columns(
        connection, target,
    )?;
    crate::template_override_migration::migrate_template_override_kinds_to_seven(
        connection, target,
    )?;
    ensure_mail_message_states_nudge_columns(connection, target)?;
    crate::graft_receiver_endpoint_schema::ensure_schema(connection, target)?;
    ensure_column(
        connection,
        target,
        "peer_trusted_peers",
        "https_port",
        "ALTER TABLE peer_trusted_peers ADD COLUMN https_port INTEGER NOT NULL DEFAULT 43101 CHECK(https_port BETWEEN 1 AND 65535);",
    )?;
    Ok(())
}

fn ensure_mail_message_columns(
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    ensure_column(
        connection,
        target,
        "mail_messages",
        "from_agent",
        "ALTER TABLE mail_messages ADD COLUMN from_agent TEXT NULL;",
    )?;
    ensure_column(
        connection,
        target,
        "mail_messages",
        "source_chat_id",
        "ALTER TABLE mail_messages ADD COLUMN source_chat_id TEXT NULL;",
    )?;
    ensure_column(
        connection,
        target,
        "mail_messages",
        "destination_chat_id",
        "ALTER TABLE mail_messages ADD COLUMN destination_chat_id TEXT NULL;",
    )?;
    ensure_column(
        connection,
        target,
        "mail_messages",
        "message_text",
        "ALTER TABLE mail_messages ADD COLUMN message_text TEXT NULL;",
    )?;
    ensure_column(
        connection,
        target,
        "mail_messages",
        "summary",
        "ALTER TABLE mail_messages ADD COLUMN summary TEXT NULL;",
    )?;
    ensure_column(
        connection,
        target,
        "mail_messages",
        "message_at",
        "ALTER TABLE mail_messages ADD COLUMN message_at TEXT NULL;",
    )?;
    ensure_column(
        connection,
        target,
        "mail_messages",
        "message_id",
        "ALTER TABLE mail_messages ADD COLUMN message_id TEXT NULL;",
    )
}

fn ensure_team_roster_columns(
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    ensure_column(
        connection,
        target,
        "team_roster",
        "member_kind",
        "ALTER TABLE team_roster ADD COLUMN member_kind TEXT NOT NULL DEFAULT 'permanent';",
    )?;
    ensure_column(
        connection,
        target,
        "team_roster",
        "harness",
        "ALTER TABLE team_roster ADD COLUMN harness TEXT NOT NULL DEFAULT 'claude-code';",
    )?;
    ensure_column(
        connection,
        target,
        "team_roster",
        "agent_type",
        "ALTER TABLE team_roster ADD COLUMN agent_type TEXT NOT NULL DEFAULT '';",
    )?;
    ensure_column(
        connection,
        target,
        "team_roster",
        "model",
        "ALTER TABLE team_roster ADD COLUMN model TEXT NOT NULL DEFAULT '';",
    )?;
    ensure_column(
        connection,
        target,
        "team_roster",
        "metadata_json",
        "ALTER TABLE team_roster ADD COLUMN metadata_json TEXT NOT NULL DEFAULT '{}';",
    )
}

fn ensure_mail_message_states_nudge_columns(
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    ensure_column(
        connection,
        target,
        "mail_message_states",
        "nudge_pending_at",
        "ALTER TABLE mail_message_states ADD COLUMN nudge_pending_at TEXT NULL;",
    )?;
    ensure_column(
        connection,
        target,
        "mail_message_states",
        "nudge_attempts",
        "ALTER TABLE mail_message_states ADD COLUMN nudge_attempts INTEGER NOT NULL DEFAULT 0;",
    )?;
    // Partial index: only rows currently awaiting a deferred nudge are
    // indexed, keeping claim_next_pending's ORDER BY message_key scan cheap
    // without paying index-maintenance cost on the (much larger) steady
    // state of already-delivered rows. Created here (post column-migration)
    // rather than in DB_MIGRATIONS because ensure_schema runs DB_MIGRATIONS
    // before the column-migration functions, and the index depends on the
    // nudge_pending_at column existing.
    connection
        .execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_mail_message_states_pending
                ON mail_message_states(team, agent, message_key)
                WHERE nudge_pending_at IS NOT NULL;",
        )
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to create pending-nudge partial index",
                error,
            )
        })
}

fn ensure_mail_messages_message_id_compat(
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    if !table_exists(connection, target, "mail_messages")? {
        return Ok(());
    }
    ensure_column(
        connection,
        target,
        "mail_messages",
        "message_id",
        "ALTER TABLE mail_messages ADD COLUMN message_id TEXT NULL;",
    )
}

pub(crate) fn ensure_column(
    connection: &Connection,
    target: &SharedDbTarget,
    table: &str,
    column: &str,
    ddl: &str,
) -> Result<(), AtmError> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table});"))
        .map_err(|error| {
            sqlite_error(
                target,
                format!("failed to inspect sqlite table {table}"),
                error,
            )
        })?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| {
            sqlite_error(
                target,
                format!("failed to enumerate sqlite columns for {table}"),
                error,
            )
        })?;
    let collected = columns
        .into_iter()
        .map(|entry| {
            entry.map_err(|error| {
                sqlite_error(
                    target,
                    format!("failed to read sqlite column metadata for {table}"),
                    error,
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if collected.into_iter().any(|value| value == column) {
        return Ok(());
    }
    connection.execute_batch(ddl).map_err(|error| {
        sqlite_error(
            target,
            format!("failed to migrate sqlite table {table}"),
            error,
        )
    })
}

fn table_exists(
    connection: &Connection,
    target: &SharedDbTarget,
    table: &str,
) -> Result<bool, AtmError> {
    connection
        .query_row(
            "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = ?1;",
            [table],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count > 0)
        .map_err(|error| {
            sqlite_error(
                target,
                format!("failed to inspect sqlite table existence for {table}"),
                error,
            )
        })
}

pub(crate) fn sqlite_open_error(target: &SharedDbTarget, source: RusqliteError) -> AtmError {
    sqlite_error(
        target,
        format!("failed to open sqlite database {}", target.display()),
        source,
    )
}

pub(crate) fn sqlite_error(
    target: &SharedDbTarget,
    message: impl Into<String>,
    source: RusqliteError,
) -> AtmError {
    let message = message.into();
    // Every arm keeps the stable code/message contract; the raw SQLite
    // failure rides along as the machine-preserved cause so a constraint
    // violation or schema mismatch is diagnosable from the surfaced error.
    let error =
        match &source {
            RusqliteError::SqliteFailure(error, _) => match error.code {
                rusqlite::ffi::ErrorCode::ConstraintViolation => AtmError::validation(message),
                rusqlite::ffi::ErrorCode::DatabaseBusy
                | rusqlite::ffi::ErrorCode::DatabaseLocked => match target {
                    SharedDbTarget::Path(path) => AtmError::mailbox_lock_timeout(path),
                    #[cfg(test)]
                    SharedDbTarget::InMemory { .. } => AtmError::mailbox_lock(format!(
                        "timed out waiting for sqlite database lock on {}",
                        target.display()
                    )),
                },
                rusqlite::ffi::ErrorCode::OperationInterrupted => match target {
                    SharedDbTarget::Path(path) => AtmError::mailbox_lock_timeout(path),
                    #[cfg(test)]
                    SharedDbTarget::InMemory { .. } => AtmError::mailbox_lock(
                        "sqlite query exceeded its caller-provided execution budget",
                    ),
                },
                rusqlite::ffi::ErrorCode::CannotOpen => AtmError::mailbox_write(message),
                rusqlite::ffi::ErrorCode::ReadOnly => AtmError::mailbox_write(message),
                rusqlite::ffi::ErrorCode::DatabaseCorrupt
                | rusqlite::ffi::ErrorCode::NotADatabase => AtmError::mailbox_read(message),
                rusqlite::ffi::ErrorCode::SystemIoFailure | rusqlite::ffi::ErrorCode::DiskFull => {
                    AtmError::mailbox_write(message)
                }
                _ => AtmError::mailbox_write(message),
            },
            _ => AtmError::mailbox_write(message),
        };
    error.with_cause(source)
}

fn json_error(message: impl Into<String>, source: serde_json::Error) -> AtmError {
    AtmError::validation(message).with_cause(source)
}

pub(crate) fn serialize_json<T: serde::Serialize>(
    value: &T,
    what: &str,
) -> Result<String, AtmError> {
    serde_json::to_string(value)
        .map_err(|error| json_error(format!("failed to encode {what}"), error))
}

pub(crate) fn deserialize_json<T: serde::de::DeserializeOwned>(
    value: &str,
    what: &str,
) -> Result<T, AtmError> {
    serde_json::from_str(value)
        .map_err(|error| json_error(format!("failed to decode {what}"), error))
}

pub(crate) fn sqlite_thread_mode(mode: Option<ThreadMode>) -> Option<&'static str> {
    match mode {
        Some(ThreadMode::AddDetails) => Some("add-details"),
        Some(ThreadMode::Supersede) => Some("supersede"),
        None => None,
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::observability::NullSqliteObservability;
    use crate::shared_db_reader_lanes::open_read_connection_for_target;
    use atm_storage::AtmErrorCode;

    /// Number of control-path borrows a single burst is modelled on. Any
    /// value comfortably above the connection bound proves reuse rather than
    /// an accidentally oversized cache.
    const CONTROL_PATH_BORROWS: usize = 32;

    fn count_probe_rows(db: &SharedDb) -> Result<i64, AtmError> {
        db.with_connection(|connection| {
            connection
                .query_row("SELECT COUNT(*) FROM mail_messages", [], |row| row.get(0))
                .map_err(|error| sqlite_error(db.target(), "failed to count probe rows", error))
        })
    }

    #[test]
    fn control_path_borrows_reuse_one_connection_instead_of_reopening_per_call() {
        let db = SharedDb::open_in_memory_for_test().expect("in-memory sqlite boundary");
        reset_opened_connection_count(db.target());

        for _ in 0..CONTROL_PATH_BORROWS {
            count_probe_rows(&db).expect("control-path read succeeds");
        }

        assert_eq!(
            opened_connection_count(db.target()),
            1,
            "sequential control-path borrows must reuse one connection; opening per call \
             multiplies descriptor demand by the in-flight admission count"
        );
    }

    #[test]
    fn concurrent_control_path_borrows_stay_within_the_connection_bound() {
        let db = SharedDb::open_in_memory_for_test().expect("in-memory sqlite boundary");
        // Warm the pool so the burst below measures reuse, not first-touch.
        count_probe_rows(&db).expect("control-path read succeeds");
        reset_opened_connection_count(db.target());

        let barrier = Arc::new(std::sync::Barrier::new(CONTROL_PATH_BORROWS));
        std::thread::scope(|scope| {
            for _ in 0..CONTROL_PATH_BORROWS {
                let db = db.clone();
                let barrier = Arc::clone(&barrier);
                scope.spawn(move || {
                    barrier.wait();
                    count_probe_rows(&db).expect("control-path read succeeds");
                });
            }
        });
        let burst_opens = opened_connection_count(db.target());
        assert!(
            burst_opens <= crate::control_path_pool::MAX_CONTROL_PATH_CONNECTIONS,
            "a control-path burst must never open more connections than the bound"
        );

        reset_opened_connection_count(db.target());
        for _ in 0..CONTROL_PATH_BORROWS {
            count_probe_rows(&db).expect("control-path read succeeds");
        }
        assert_eq!(
            opened_connection_count(db.target()),
            0,
            "after a burst the parked connections must absorb the next round without reopening"
        );
    }

    #[test]
    fn failed_control_path_borrows_are_not_parked_for_reuse() {
        let db = SharedDb::open_in_memory_for_test().expect("in-memory sqlite boundary");
        reset_opened_connection_count(db.target());

        let failure = db
            .with_connection(|_| Err::<(), _>(AtmError::mailbox_write("probe failure")))
            .expect_err("the probe operation fails");
        assert!(failure.message().starts_with("probe failure"));
        count_probe_rows(&db).expect("control-path read succeeds");

        assert_eq!(
            opened_connection_count(db.target()),
            2,
            "a connection whose operation failed is dropped rather than parked"
        );
    }

    #[test]
    fn sqlite_constraint_violations_keep_the_sqlite_cause() {
        let database = tempfile::NamedTempFile::new().expect("temporary database");
        let target = SharedDbTarget::Path(database.path().to_path_buf());
        let writer = open_connection_for_target(&target).expect("writer connection");
        writer
            .execute_batch("CREATE TABLE probe (value TEXT NOT NULL);")
            .expect("create fixture table");
        let failure = writer
            .execute("INSERT INTO probe (value) VALUES (NULL)", [])
            .expect_err("NOT NULL constraint must fail");
        let error = sqlite_error(&target, "failed to persist probe", failure);
        assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
        assert!(error.message().starts_with("failed to persist probe"));
        assert_eq!(
            error.cause(),
            Some("NOT NULL constraint failed: probe.value"),
            "the raw SQLite failure must survive as the machine-preserved cause"
        );
    }

    #[test]
    fn json_errors_keep_the_serde_cause() {
        let failure = serde_json::from_str::<serde_json::Value>("{").expect_err("invalid json");
        let error = json_error("failed to decode probe", failure);
        assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
        assert!(error.cause().is_some_and(|cause| cause.contains("EOF")));
    }

    #[test]
    fn defensive_reader_connection_rejects_writes() {
        let database = tempfile::NamedTempFile::new().expect("temporary database");
        let target = SharedDbTarget::Path(database.path().to_path_buf());
        let writer = open_connection_for_target(&target).expect("writer connection");
        writer
            .execute_batch("CREATE TABLE read_only_probe (value INTEGER);")
            .expect("create fixture table");
        let reader = open_read_connection_for_target(&target).expect("defensive reader");
        assert!(
            reader
                .execute("INSERT INTO read_only_probe (value) VALUES (1)", [])
                .is_err(),
            "reader lanes must reject writes through query_only/READ_ONLY"
        );
    }

    #[test]
    fn ensure_schema_drops_the_retired_peer_sync_policy_table() {
        let target = SharedDbTarget::InMemory {
            uri: format!(
                "file:atm-storage-rusqlite-peer-sync-retirement-{}?mode=memory&cache=shared",
                NEXT_IN_MEMORY_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        };
        let mut connection = open_connection_for_target(&target).expect("open connection");
        connection
            .execute_batch(
                "CREATE TABLE peer_sync_policies (
                    host TEXT NOT NULL PRIMARY KEY,
                    max_message_age_seconds INTEGER NOT NULL,
                    max_batch_messages INTEGER NOT NULL
                 );
                 CREATE INDEX idx_peer_sync_policies_host ON peer_sync_policies(host);",
            )
            .expect("create legacy peer sync configuration");

        ensure_schema(&mut connection, &target).expect("migrate schema");

        assert!(
            !table_exists(&connection, &target, "peer_sync_policies")
                .expect("inspect retired table"),
            "AK.2 must remove the obsolete worker policy from existing databases"
        );
    }

    #[test]
    fn ensure_schema_adds_the_message_key_lookup_index_for_post_commit_dispatch() {
        let target = SharedDbTarget::InMemory {
            uri: format!(
                "file:atm-storage-rusqlite-message-key-index-{}?mode=memory&cache=shared",
                NEXT_IN_MEMORY_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        };
        let mut connection = open_connection_for_target(&target).expect("open connection");
        ensure_schema(&mut connection, &target).expect("initialize schema");
        connection
            .execute_batch("DROP INDEX idx_mail_messages_message_key;")
            .expect("simulate database created before the lookup index");

        ensure_schema(&mut connection, &target).expect("upgrade existing schema");

        let plan: String = connection
            .query_row(
                "EXPLAIN QUERY PLAN
                 SELECT team, agent, envelope_json
                 FROM mail_messages
                 WHERE message_key = ?1;",
                ["atm:01J00000000000000000000000"],
                |row| row.get(3),
            )
            .expect("explain message-key lookup");
        assert!(
            plan.contains("idx_mail_messages_message_key"),
            "post-commit message lookup must remain indexed instead of scanning a growing mailbox: {plan}"
        );
    }
    #[test]
    fn configured_busy_timeout_never_reaches_the_server_request_budget() {
        let target = SharedDbTarget::InMemory {
            uri: format!(
                "file:atm-storage-rusqlite-busy-timeout-{}?mode=memory&cache=shared",
                NEXT_IN_MEMORY_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        };
        let connection = open_connection_for_target(&target).expect("open connection");

        let configured_ms: i64 = connection
            .query_row("PRAGMA busy_timeout;", [], |row| row.get(0))
            .expect("read configured sqlite busy_timeout");

        let expected_ms =
            i64::try_from(atm_storage::request_budget::SQLITE_BUSY_TIMEOUT.as_millis())
                .expect("busy timeout millis fit in i64");
        assert_eq!(
            configured_ms, expected_ms,
            "sqlite busy_timeout must stay derived from the shared request budget"
        );

        let budget_ms =
            i64::try_from(atm_storage::request_budget::SERVER_REQUEST_BUDGET.as_millis())
                .expect("server budget millis fit in i64");
        assert!(
            configured_ms < budget_ms,
            "sqlite busy_timeout ({configured_ms}ms) must stay below the server \
             request budget ({budget_ms}ms), or a writer-lock wait could never \
             succeed inside one request"
        );
    }

    use std::sync::Barrier;
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn stalled_writer_lock_yields_a_typed_lock_error_after_the_busy_timeout() {
        // Proves the SQLITE_BUSY_TIMEOUT < SERVER_REQUEST_BUDGET contract
        // holds under real writer-lock contention, not only as the
        // compile-time assertion in `atm_storage::request_budget`: a second
        // writer that has to wait out the configured busy_timeout must fail
        // with a typed lock error, so the daemon can return an actionable
        // response instead of the request silently overrunning on the
        // client. This deliberately waits on SQLite's own busy-lock retry
        // loop instead of a fixed sleep primitive; the elapsed wall time is
        // the product observable under test, not a synchronization device.
        //
        // This intentionally uses a file-backed target rather than the
        // `cache=shared` in-memory target used by other tests in this
        // module: SQLite's shared-cache mode reports same-process table
        // lock conflicts as `SQLITE_LOCKED`, and `busy_timeout`'s retry
        // handler only fires for `SQLITE_BUSY`, so a shared-cache
        // in-memory target would fail the second writer immediately
        // without ever exercising the busy-wait this test verifies.
        // Production storage (`SharedDbTarget::Path`) always opens plain,
        // non-shared-cache connections, so this matches production
        // locking semantics.
        let tempdir = tempfile::tempdir().expect("create temporary database directory");
        let db_path = tempdir.path().join("busy-timeout-contract.db");
        let db = Arc::new(
            SharedDb::open_with_observability(&db_path, Arc::new(NullSqliteObservability))
                .expect("open database"),
        );

        let (holder_ready_tx, holder_ready_rx) = mpsc::channel::<()>();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let holder_db = Arc::clone(&db);
        let holder = thread::spawn(move || {
            holder_db
                .with_connection(|connection| {
                    let transaction = connection
                        .transaction_with_behavior(TransactionBehavior::Immediate)
                        .expect("first writer acquires the immediate transaction lock");
                    holder_ready_tx
                        .send(())
                        .expect("signal that the writer lock is held");
                    release_rx
                        .recv()
                        .expect("wait for the test to release the writer lock");
                    transaction.commit().expect("release the writer lock");
                    Ok(())
                })
                .expect("holder transaction succeeds");
        });

        holder_ready_rx
            .recv()
            .expect("first writer must confirm it holds the lock before the second write starts");

        let started_at = std::time::Instant::now();
        let result = db.with_transaction(|_connection| Ok(()));
        let elapsed = started_at.elapsed();

        release_tx
            .send(())
            .expect("release the held writer lock so the holder thread can finish");
        holder.join().expect("holder thread panicked");

        let error = result.expect_err(
            "a second writer contending on an already-held immediate transaction must fail \
             once the configured busy_timeout elapses",
        );
        assert_eq!(
            error.code(),
            atm_storage::AtmErrorCode::MailboxLockTimeout,
            "a busy writer lock must surface as a typed mailbox-lock error, not an \
             opaque failure: {error:?}"
        );
        eprintln!("stalled writer lock typed-error elapsed: {elapsed:?}");
        assert!(
            elapsed >= atm_storage::request_budget::SQLITE_BUSY_TIMEOUT / 2,
            "a stalled writer must actually wait out most of the configured \
             busy_timeout ({:?}) before failing, not fail immediately: waited {elapsed:?}",
            atm_storage::request_budget::SQLITE_BUSY_TIMEOUT,
        );
    }

    #[test]
    fn writer_initialization_persists_wal_for_later_reader_connections() {
        let tempdir = tempfile::tempdir().expect("create temporary database directory");
        let path = tempdir.path().join("wal.db");
        let target = SharedDbTarget::Path(path.clone());
        {
            let writer =
                open_writer_connection_for_target(&target).expect("open writer connection");
            let writer_mode: String = writer
                .query_row("PRAGMA journal_mode;", [], |row| row.get(0))
                .expect("read writer journal mode");
            assert_eq!(writer_mode.to_ascii_lowercase(), "wal");
            let writer_cache_kib: i64 = writer
                .query_row("PRAGMA cache_size;", [], |row| row.get(0))
                .expect("read writer cache size");
            assert_eq!(writer_cache_kib, -WRITER_CACHE_KIB);

            let reader = open_connection_for_target(&target).expect("open reader connection");
            let reader_mode: String = reader
                .query_row("PRAGMA journal_mode;", [], |row| row.get(0))
                .expect("read persisted journal mode");
            assert_eq!(reader_mode.to_ascii_lowercase(), "wal");
        }
    }

    #[test]
    fn concurrent_read_handles_do_not_fail_fast_at_an_arbitrary_reader_cap() {
        let db = Arc::new(SharedDb::open_in_memory_for_test().expect("open database"));
        let start = Arc::new(Barrier::new(9));
        thread::scope(|scope| {
            let mut workers = Vec::new();
            for _ in 0..8 {
                let db = Arc::clone(&db);
                let start = Arc::clone(&start);
                workers.push(scope.spawn(move || {
                    start.wait();
                    db.with_connection(|connection| {
                        connection
                            .query_row("SELECT 1;", [], |row| row.get::<_, i64>(0))
                            .map_err(|error| db.error("concurrent reader failed", error))
                            .map(|_| ())
                    })
                }));
            }
            start.wait();
            for worker in workers {
                worker
                    .join()
                    .expect("reader thread panicked")
                    .expect("reader should succeed");
            }
        });
    }

    #[test]
    fn ensure_team_nudge_template_override_columns_migrates_legacy_empty_rows_to_disabled() {
        let target = SharedDbTarget::InMemory {
            uri: format!(
                "file:atm-storage-rusqlite-shared-db-test-{}?mode=memory&cache=shared",
                NEXT_IN_MEMORY_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        };
        let connection = open_connection_for_target(&target).expect("open connection");
        connection
            .execute_batch(
                "CREATE TABLE team_nudge_template_overrides (
                    team_name TEXT NOT NULL,
                    template_kind TEXT NOT NULL,
                    template_body TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    PRIMARY KEY (team_name, template_kind)
                );",
            )
            .expect("create pre-migration table");
        connection
            .execute(
                "INSERT INTO team_nudge_template_overrides(
                    team_name, template_kind, template_body, updated_at
                 ) VALUES (?1, ?2, ?3, ?4);",
                rusqlite::params![
                    "test-team",
                    "delivery_ack",
                    "",
                    atm_storage::types::IsoTimestamp::now().to_string()
                ],
            )
            .expect("insert legacy empty-body row");

        crate::template_override_migration::ensure_team_nudge_template_override_columns(
            &connection,
            &target,
        )
        .expect("migrate override table");

        let mode_exists = connection
            .prepare("PRAGMA table_info(team_nudge_template_overrides);")
            .expect("prepare pragma")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query pragma")
            .filter_map(Result::ok)
            .any(|column| column == "mode");
        assert!(mode_exists, "schema upgrade should add the mode column");

        let mode: String = connection
            .query_row(
                "SELECT mode FROM team_nudge_template_overrides
                 WHERE team_name = ?1 AND template_kind = ?2;",
                rusqlite::params!["test-team", "delivery_ack"],
                |row| row.get(0),
            )
            .expect("query migrated mode");
        assert_eq!(mode, "disabled");
    }

    #[test]
    fn ensure_schema_rebuilds_six_kind_override_table_and_is_idempotent() {
        let target = SharedDbTarget::InMemory {
            uri: format!(
                "file:atm-storage-rusqlite-shared-db-test-{}?mode=memory&cache=shared",
                NEXT_IN_MEMORY_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        };
        let mut connection = open_connection_for_target(&target).expect("open connection");
        connection
            .execute_batch(
                "CREATE TABLE team_nudge_template_overrides (
                    team_name TEXT NOT NULL,
                    template_kind TEXT NOT NULL CHECK(template_kind IN (
                        'delivery', 'delivery_ack', 'delivery_task', 'delivery_task_ack',
                        'acknowledge', 'acknowledge_task'
                    )),
                    template_body TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    PRIMARY KEY (team_name, template_kind)
                );
                INSERT INTO team_nudge_template_overrides
                    (team_name, template_kind, template_body, updated_at)
                VALUES
                    ('test-team', 'delivery_task', '<old-task/>', '2026-09-05T00:00:00Z'),
                    ('test-team', 'delivery_ack', '<delivery-ack/>', '2026-09-05T00:00:00Z');",
            )
            .expect("create six-kind table");

        ensure_schema(&mut connection, &target).expect("migrate six-kind table");

        let row_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM team_nudge_template_overrides WHERE team_name = 'test-team';",
                [],
                |row| row.get(0),
            )
            .expect("count migrated rows");
        assert_eq!(row_count, 1, "only the retired row should be dropped");
        let retained_kind: String = connection
            .query_row(
                "SELECT template_kind FROM team_nudge_template_overrides WHERE team_name = 'test-team';",
                [],
                |row| row.get(0),
            )
            .expect("read retained row");
        assert_eq!(retained_kind, "delivery_ack");
        connection
            .execute(
                "INSERT INTO team_nudge_template_overrides
                    (team_name, template_kind, mode, template_body, updated_at)
                 VALUES ('test-team', 'queue_ack', 'override', '<queue-ack/>', '2026-09-05T00:00:00Z');",
                [],
            )
            .expect("new queue kind should satisfy migrated check");
        let schema_sql: String = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'team_nudge_template_overrides';",
                [],
                |row| row.get(0),
            )
            .expect("read migrated schema");
        assert!(schema_sql.contains("'queue'"));

        let schema_before_second_open = schema_sql.clone();
        ensure_schema(&mut connection, &target).expect("second schema ensure");
        let schema_after_second_open: String = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'team_nudge_template_overrides';",
                [],
                |row| row.get(0),
            )
            .expect("read schema after second open");
        assert_eq!(schema_after_second_open, schema_before_second_open);
        let row_count_after_second_open: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM team_nudge_template_overrides WHERE team_name = 'test-team';",
                [],
                |row| row.get(0),
            )
            .expect("count rows after second open");
        assert_eq!(row_count_after_second_open, 2);
    }

    #[test]
    fn ensure_mail_message_states_nudge_columns_migrates_legacy_rows_and_creates_the_partial_index()
    {
        let target = SharedDbTarget::InMemory {
            uri: format!(
                "file:atm-storage-rusqlite-shared-db-test-{}?mode=memory&cache=shared",
                NEXT_IN_MEMORY_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        };
        let connection = open_connection_for_target(&target).expect("open connection");
        connection
            .execute_batch(
                "CREATE TABLE mail_message_states (
                    team TEXT NOT NULL,
                    agent TEXT NOT NULL,
                    message_key TEXT NOT NULL,
                    read INTEGER NOT NULL DEFAULT 0 CHECK(read IN (0, 1)),
                    pending_ack_at TEXT NULL,
                    acknowledged_at TEXT NULL,
                    expires_at TEXT NULL,
                    deleted_at TEXT NULL,
                    updated_at TEXT NULL,
                    PRIMARY KEY (team, agent, message_key)
                 );",
            )
            .expect("create pre-AQ1 mail_message_states table");
        connection
            .execute(
                "INSERT INTO mail_message_states(team, agent, message_key, read, updated_at)
                 VALUES (?1, ?2, ?3, 0, ?4);",
                rusqlite::params![
                    "test-team",
                    "test-agent",
                    "atm:01J00000000000000000000001",
                    atm_storage::types::IsoTimestamp::now().to_string()
                ],
            )
            .expect("insert legacy row");

        ensure_mail_message_states_nudge_columns(&connection, &target)
            .expect("migrate mail_message_states nudge columns");

        let columns: Vec<String> = connection
            .prepare("PRAGMA table_info(mail_message_states);")
            .expect("prepare pragma")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query pragma")
            .collect::<Result<_, _>>()
            .expect("decode pragma");
        assert!(
            columns.iter().any(|column| column == "nudge_pending_at"),
            "schema upgrade should add the nudge_pending_at column"
        );
        assert!(
            columns.iter().any(|column| column == "nudge_attempts"),
            "schema upgrade should add the nudge_attempts column"
        );

        let (read, nudge_pending_at, nudge_attempts): (i64, Option<String>, i64) = connection
            .query_row(
                "SELECT read, nudge_pending_at, nudge_attempts FROM mail_message_states
                 WHERE team = 'test-team' AND agent = 'test-agent'
                   AND message_key = 'atm:01J00000000000000000000001';",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("query migrated row");
        assert_eq!(
            read, 0,
            "the migration must preserve pre-existing column data"
        );
        assert!(nudge_pending_at.is_none());
        assert_eq!(nudge_attempts, 0);

        let index_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_mail_message_states_pending';",
                [],
                |row| row.get(0),
            )
            .expect("query sqlite_master");
        assert_eq!(
            index_count, 1,
            "the migration must create the pending-nudge partial index"
        );

        // Idempotent: running the migration again on an already-migrated
        // database must not error.
        ensure_mail_message_states_nudge_columns(&connection, &target)
            .expect("re-run migration idempotently");
    }
}
