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
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(test)]
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

// Search projection writes touch several B-trees and FTS segments inside the
// sole durable writer transaction.  The SQLite default is only 2 MiB, which
// churns cache pages under a concurrent admission burst.  This setting applies
// only to the one writer connection and bounds its page cache at 32 MiB.
const WRITER_CACHE_KIB: i64 = 32 * 1024;
#[cfg(test)]
static NEXT_IN_MEMORY_DB_ID: AtomicU64 = AtomicU64::new(1);
#[cfg(test)]
static OPENED_CONNECTIONS: LazyLock<Mutex<std::collections::HashMap<String, usize>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// Canonical `mail_messages` table DDL, parameterized by its `CREATE` clause.
///
/// The fresh-database schema in [`DB_MIGRATIONS`] and the legacy
/// `message_text` rebuild both materialize their table from this single
/// definition, so the two shapes cannot drift apart. The column order here is
/// also the order a fresh database ends up with after the additive
/// `ALTER TABLE` migrations in `ensure_mail_message_columns` and
/// `template_catalog_schema::ensure_schema` have run.
macro_rules! mail_messages_table_ddl {
    ($create:literal) => {
        concat!(
            $create,
            r#" (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    message_key TEXT NOT NULL,
    envelope_json TEXT NOT NULL,
    from_agent TEXT NOT NULL,
    source_chat_id TEXT NULL,
    destination_chat_id TEXT NULL,
    message_text TEXT NULL,
    template_sha TEXT NULL,
    vars_json TEXT NULL,
    category TEXT NULL,
    content_format TEXT NULL,
    tags_json TEXT NOT NULL DEFAULT '[]',
    summary TEXT NULL,
    message_at TEXT NOT NULL,
    message_id TEXT NULL,
    parent_message_id TEXT NULL,
    thread_mode TEXT NULL CHECK(thread_mode IS NULL OR thread_mode IN ('add-details', 'supersede')),
    recorded_at TEXT,
    workflow_scope_kind TEXT NULL,
    workflow_scope_id TEXT NULL,
    workflow_state TEXT NULL,
    workflow_stage TEXT NULL,
    workflow_transition TEXT NULL,
    workflow_iteration TEXT NULL,
    applied_template_tags_json TEXT NULL,
    effective_tags_json TEXT NULL,
    CHECK(message_key GLOB 'atm:*' OR message_key GLOB 'ext:*'),
    PRIMARY KEY (team, agent, message_key)
);
"#
        )
    };
}

/// Every index owned by `mail_messages`.
///
/// The rebuild drops the table (and therefore its indexes), so it replays this
/// exact definition to guarantee a migrated database is index-identical to a
/// fresh one.
macro_rules! mail_messages_index_ddl {
    () => {
        r#"
CREATE UNIQUE INDEX IF NOT EXISTS uq_mail_messages_single_successor
    ON mail_messages(team, agent, parent_message_id)
    WHERE parent_message_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS uq_mail_messages_message_id
    ON mail_messages(team, agent, message_id)
    WHERE message_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_mail_messages_mailbox
    ON mail_messages(team, agent);

-- Post-commit received-hook dispatch reloads an admitted record by its
-- immutable message key.  The mailbox primary key begins with team and agent,
-- so it cannot serve that global-key lookup without scanning a growing table.
CREATE INDEX IF NOT EXISTS idx_mail_messages_message_key
    ON mail_messages(message_key);
"#
    };
}

/// Staging table name used by the `message_text` nullability rebuild.
const MAIL_MESSAGES_REBUILD_TABLE: &str = "mail_messages_nullable_rebuild";

/// `CREATE TABLE` for the rebuild staging table, byte-identical in shape to
/// the fresh `mail_messages` definition.
const MAIL_MESSAGES_REBUILD_TABLE_DDL: &str =
    mail_messages_table_ddl!("CREATE TABLE mail_messages_nullable_rebuild");

/// Index DDL replayed after the rebuild swaps the tables.
const MAIL_MESSAGES_INDEX_DDL: &str = mail_messages_index_ddl!();

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
            'delivery_task',
            'delivery_task_ack',
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
        let mut connection = self.open_connection()?;
        operation(&mut connection)
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

    fn open_connection(&self) -> Result<Connection, AtmError> {
        open_connection_for_target(self.target.as_ref())
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
    ensure_mail_messages_message_text_nullable(connection, target)?;
    crate::search_schema::ensure_schema(connection, target)?;
    ensure_team_roster_columns(connection, target)?;
    ensure_team_roster_harness_values(connection, target)?;
    ensure_team_nudge_template_override_columns(connection, target)?;
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

/// Relaxes a legacy `mail_messages.message_text NOT NULL` column to nullable.
///
/// Databases created before decomposed template admission shipped still carry
/// `message_text TEXT NOT NULL`. SQLite cannot relax a `NOT NULL` constraint
/// in place, and the additive `ensure_column` migrations only add missing
/// columns, so `atm send --template` fails on those databases with a
/// constraint violation when the decomposed writer nulls the column out.
///
/// The probe is self-detecting rather than version-stamped: it reads the live
/// column flag and returns without writing anything when the table is missing
/// or the column is already nullable, so correct databases pay one pragma
/// query per startup.
///
/// # Errors
/// Returns an error when the probe or any rebuild stage fails. Every failure
/// rolls the rebuild transaction back, so the legacy table is left untouched
/// and a later startup retries.
fn ensure_mail_messages_message_text_nullable(
    connection: &mut Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    use rusqlite::OptionalExtension;

    let legacy_not_null = connection
        .query_row(
            r#"SELECT "notnull" FROM pragma_table_info('mail_messages') WHERE name = 'message_text';"#,
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to probe mail_messages.message_text nullability",
                error,
            )
        })?;
    if legacy_not_null != Some(1) {
        return Ok(());
    }

    // `PRAGMA foreign_keys` is a no-op inside a transaction, so it must be
    // toggled here: `mail_message_states` has a foreign key onto
    // `mail_messages`, and the drop/rename below would otherwise cascade the
    // state rows away. This follows SQLite's documented ALTER TABLE procedure.
    connection
        .pragma_update(None, "foreign_keys", "OFF")
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to disable foreign keys for the mail_messages rebuild",
                error,
            )
        })?;
    let rebuild = rebuild_mail_messages_with_nullable_message_text(connection, target);
    let restored = connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to restore foreign keys after the mail_messages rebuild",
                error,
            )
        });
    rebuild?;
    restored
}

/// Performs the SQLite table rebuild that relaxes `message_text` to nullable.
///
/// Callers must have disabled `PRAGMA foreign_keys` and must restore it
/// afterwards; see [`ensure_mail_messages_message_text_nullable`].
fn rebuild_mail_messages_with_nullable_message_text(
    connection: &mut Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let started = Instant::now();
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to start the mail_messages message_text rebuild",
                error,
            )
        })?;

    let legacy_columns = table_column_names(&transaction, target, "mail_messages")?;

    // The catalog view selects from `mail_messages`; SQLite validates view
    // bodies during `ALTER TABLE ... RENAME`, so it is dropped up front and
    // recreated verbatim from its owning module once the swap is done.
    transaction
        .execute_batch("DROP VIEW IF EXISTS decomposed_messages;")
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to drop the decomposed_messages view for the mail_messages rebuild",
                error,
            )
        })?;

    transaction
        .execute_batch(MAIL_MESSAGES_REBUILD_TABLE_DDL)
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to create the rebuilt mail_messages table",
                error,
            )
        })?;
    let rebuilt_columns = table_column_names(&transaction, target, MAIL_MESSAGES_REBUILD_TABLE)?;

    let unknown = difference(&legacy_columns, &rebuilt_columns);
    if !unknown.is_empty() {
        return Err(AtmError::mailbox_write(format!(
            "refusing to rebuild mail_messages on {}: the legacy table carries column(s) {} that the current schema does not define",
            target.display(),
            unknown.join(", ")
        )));
    }
    let absent = difference(&rebuilt_columns, &legacy_columns);
    if !absent.is_empty() {
        return Err(AtmError::mailbox_write(format!(
            "refusing to rebuild mail_messages on {}: the legacy table is missing column(s) {} that the additive migrations should have added",
            target.display(),
            absent.join(", ")
        )));
    }

    let column_list = rebuilt_columns
        .iter()
        .map(|column| format!("\"{column}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let copied = transaction
        .execute(
            &format!(
                "INSERT INTO {MAIL_MESSAGES_REBUILD_TABLE} ({column_list}) SELECT {column_list} FROM mail_messages;"
            ),
            [],
        )
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to copy rows into the rebuilt mail_messages table",
                error,
            )
        })?;

    transaction
        .execute_batch(&format!(
            "DROP TABLE mail_messages;
             ALTER TABLE {MAIL_MESSAGES_REBUILD_TABLE} RENAME TO mail_messages;"
        ))
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to swap in the rebuilt mail_messages table",
                error,
            )
        })?;

    transaction
        .execute_batch(MAIL_MESSAGES_INDEX_DDL)
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to recreate mail_messages indexes after the rebuild",
                error,
            )
        })?;
    crate::template_catalog_schema::ensure_schema(&transaction, target)?;

    let violations = transaction
        .query_row(
            "SELECT COUNT(1) FROM pragma_foreign_key_check;",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to run the foreign-key check after the mail_messages rebuild",
                error,
            )
        })?;
    if violations > 0 {
        return Err(AtmError::mailbox_write(format!(
            "mail_messages rebuild on {} left {violations} foreign key violation(s); rolling back",
            target.display()
        )));
    }

    transaction.commit().map_err(|error| {
        sqlite_error(
            target,
            "failed to commit the mail_messages message_text rebuild",
            error,
        )
    })?;
    tracing::info!(
        event = "sqlite.mail_messages.message_text_rebuild",
        db.namespace = %target.display(),
        db.rows_copied = copied,
        db.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        "rebuilt mail_messages so message_text is nullable",
    );
    Ok(())
}

/// Returns the entries of `left` that do not appear in `right`.
fn difference(left: &[String], right: &[String]) -> Vec<String> {
    left.iter()
        .filter(|column| !right.contains(column))
        .cloned()
        .collect()
}

/// Reads the declared column names of `table` in their declaration order.
fn table_column_names(
    connection: &Connection,
    target: &SharedDbTarget,
    table: &str,
) -> Result<Vec<String>, AtmError> {
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
    columns
        .map(|entry| {
            entry.map_err(|error| {
                sqlite_error(
                    target,
                    format!("failed to read sqlite column metadata for {table}"),
                    error,
                )
            })
        })
        .collect()
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

fn ensure_team_roster_harness_values(
    connection: &mut Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    let table_sql = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'team_roster';",
            [],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to inspect team_roster harness constraint",
                error,
            )
        })?;
    if table_sql.contains("'hermes'") && table_sql.contains("'python-graft'") {
        return Ok(());
    }

    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to start team_roster harness migration",
                error,
            )
        })?;
    transaction
        .execute_batch(
            "DROP INDEX IF EXISTS idx_team_roster_team_name;
             ALTER TABLE team_roster RENAME TO team_roster_legacy_harness;
             CREATE TABLE team_roster (
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
             INSERT INTO team_roster(
                 team_name, agent_name, member_kind, harness, agent_type, model,
                 metadata_json, source, recipient_pane_id, updated_at
             )
             SELECT
                 team_name, agent_name, member_kind, harness, agent_type, model,
                 metadata_json, source, recipient_pane_id, updated_at
             FROM team_roster_legacy_harness;
             DROP TABLE team_roster_legacy_harness;
             CREATE INDEX idx_team_roster_team_name ON team_roster(team_name);",
        )
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to migrate team_roster harness constraint",
                error,
            )
        })?;
    transaction.commit().map_err(|error| {
        sqlite_error(
            target,
            "failed to commit team_roster harness migration",
            error,
        )
    })
}

fn ensure_team_nudge_template_override_columns(
    connection: &Connection,
    target: &SharedDbTarget,
) -> Result<(), AtmError> {
    ensure_column(
        connection,
        target,
        "team_nudge_template_overrides",
        "mode",
        "ALTER TABLE team_nudge_template_overrides ADD COLUMN mode TEXT NOT NULL DEFAULT 'override';",
    )?;
    connection
        .execute(
            "UPDATE team_nudge_template_overrides
             SET mode = 'disabled'
             WHERE mode = 'override' AND template_body = '';",
            [],
        )
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to normalize legacy empty nudge-template override rows",
                error,
            )
        })?;
    Ok(())
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
    fn ensure_team_roster_harness_values_migrates_legacy_check_constraint() {
        let target = SharedDbTarget::InMemory {
            uri: format!(
                "file:atm-storage-rusqlite-roster-harness-test-{}?mode=memory&cache=shared",
                NEXT_IN_MEMORY_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        };
        let mut connection = open_connection_for_target(&target).expect("open connection");
        connection
            .execute_batch(
                "CREATE TABLE team_roster (
                    team_name TEXT NOT NULL,
                    agent_name TEXT NOT NULL,
                    member_kind TEXT NOT NULL CHECK(member_kind IN ('permanent', 'ephemeral')),
                    harness TEXT NOT NULL CHECK(harness IN ('claude-code', 'codex-cli', 'gemini-cli', 'opencode')),
                    agent_type TEXT NOT NULL DEFAULT '',
                    model TEXT NOT NULL DEFAULT '',
                    metadata_json TEXT NOT NULL DEFAULT '{}',
                    source TEXT,
                    recipient_pane_id TEXT NULL,
                    updated_at TEXT NOT NULL,
                    PRIMARY KEY (team_name, agent_name)
                );
                CREATE INDEX idx_team_roster_team_name ON team_roster(team_name);
                INSERT INTO team_roster(
                    team_name, agent_name, member_kind, harness, updated_at
                ) VALUES ('hermes', 'skillrx', 'permanent', 'claude-code', 'now');",
            )
            .expect("create legacy roster table");

        ensure_team_roster_harness_values(&mut connection, &target).expect("migrate roster");
        connection
            .execute(
                "INSERT INTO team_roster(
                    team_name, agent_name, member_kind, harness, updated_at
                ) VALUES ('hermes', 'python-worker', 'permanent', 'python-graft', 'now');",
                [],
            )
            .expect("new harness should satisfy migrated check constraint");
        let harnesses: Vec<String> = connection
            .prepare(
                "SELECT harness FROM team_roster
                 WHERE team_name = 'hermes' ORDER BY agent_name;",
            )
            .expect("prepare harness query")
            .query_map([], |row| row.get(0))
            .expect("query harnesses")
            .collect::<Result<_, _>>()
            .expect("decode harnesses");
        assert_eq!(harnesses, vec!["python-graft", "claude-code"]);
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

        ensure_team_nudge_template_override_columns(&connection, &target)
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

    /// `mail_messages` exactly as databases created before the nullable
    /// `message_text` change carry it, including the foreign-key dependent
    /// state table and every index owned by the mailbox table.
    const LEGACY_MAIL_SCHEMA_DDL: &str = r#"
CREATE TABLE mail_messages (
    team TEXT NOT NULL, agent TEXT NOT NULL, message_key TEXT NOT NULL,
    envelope_json TEXT NOT NULL, from_agent TEXT NOT NULL,
    source_chat_id TEXT NULL, destination_chat_id TEXT NULL,
    message_text TEXT NOT NULL, summary TEXT NULL, message_at TEXT NOT NULL,
    message_id TEXT NULL, parent_message_id TEXT NULL,
    thread_mode TEXT NULL CHECK(thread_mode IS NULL OR thread_mode IN ('add-details', 'supersede')),
    recorded_at TEXT,
    CHECK(message_key GLOB 'atm:*' OR message_key GLOB 'ext:*'),
    PRIMARY KEY (team, agent, message_key)
);

CREATE TABLE mail_message_states (
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    message_key TEXT NOT NULL,
    read INTEGER NOT NULL DEFAULT 0 CHECK(read IN (0, 1)),
    pending_ack_at TEXT NULL,
    acknowledged_at TEXT NULL,
    expires_at TEXT NULL,
    deleted_at TEXT NULL,
    updated_at TEXT NULL,
    PRIMARY KEY (team, agent, message_key),
    FOREIGN KEY (team, agent, message_key)
        REFERENCES mail_messages(team, agent, message_key)
        ON DELETE CASCADE
);

CREATE UNIQUE INDEX uq_mail_messages_single_successor
    ON mail_messages(team, agent, parent_message_id)
    WHERE parent_message_id IS NOT NULL;

CREATE UNIQUE INDEX uq_mail_messages_message_id
    ON mail_messages(team, agent, message_id)
    WHERE message_id IS NOT NULL;

CREATE INDEX idx_mail_messages_mailbox
    ON mail_messages(team, agent);

CREATE INDEX idx_mail_messages_message_key
    ON mail_messages(message_key);
"#;

    const LEGACY_MAIL_ROWS_DDL: &str = r#"
INSERT INTO mail_messages(
    team, agent, message_key, envelope_json, from_agent, source_chat_id,
    destination_chat_id, message_text, summary, message_at, message_id,
    parent_message_id, thread_mode, recorded_at
) VALUES
 ('team-a', 'agent-a', 'atm:one', '{"one":1}', 'sender-one', 'src-1', 'dst-1',
  'body one', 'summary one', '2026-07-13T00:00:00Z', 'msg-1', NULL, NULL,
  '2026-07-13T00:00:01Z'),
 ('team-a', 'agent-a', 'atm:two', '{"two":2}', 'sender-two', NULL, NULL,
  'body two', NULL, '2026-07-13T00:00:02Z', 'msg-2', 'msg-1', 'supersede',
  '2026-07-13T00:00:03Z'),
 ('team-b', 'agent-b', 'ext:three', '{"three":3}', 'sender-three', NULL, NULL,
  'body three', 'summary three', '2026-07-13T00:00:04Z', NULL, NULL, NULL, NULL);

INSERT INTO mail_message_states(
    team, agent, message_key, read, acknowledged_at, updated_at
) VALUES ('team-a', 'agent-a', 'atm:two', 1, '2026-07-13T00:01:00Z', '2026-07-13T00:01:00Z');
"#;

    /// Full legacy row projection used to prove the rebuild is lossless.
    ///
    /// Every legacy column is TEXT, so one nullable-string vector per row keeps
    /// the comparison byte-for-byte without a bespoke row struct.
    type LegacyRow = Vec<Option<String>>;

    fn in_memory_target(label: &str) -> SharedDbTarget {
        SharedDbTarget::InMemory {
            uri: format!(
                "file:atm-storage-rusqlite-{label}-{}?mode=memory&cache=shared",
                NEXT_IN_MEMORY_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        }
    }

    fn legacy_mail_messages_connection(label: &str) -> (SharedDbTarget, Connection) {
        let target = in_memory_target(label);
        let connection = open_connection_for_target(&target).expect("open connection");
        connection
            .execute_batch(LEGACY_MAIL_SCHEMA_DDL)
            .expect("create legacy mail schema");
        (target, connection)
    }

    fn fresh_schema_connection(label: &str) -> (SharedDbTarget, Connection) {
        let target = in_memory_target(label);
        let mut connection = open_connection_for_target(&target).expect("open connection");
        ensure_schema(&mut connection, &target).expect("fresh schema");
        (target, connection)
    }

    fn message_text_notnull(connection: &Connection) -> i64 {
        connection
            .query_row(
                r#"SELECT "notnull" FROM pragma_table_info('mail_messages') WHERE name = 'message_text';"#,
                [],
                |row| row.get(0),
            )
            .expect("probe message_text nullability")
    }

    fn foreign_keys_enabled(connection: &Connection) -> i64 {
        connection
            .query_row("PRAGMA foreign_keys;", [], |row| row.get(0))
            .expect("read foreign_keys pragma")
    }

    fn foreign_key_violations(connection: &Connection) -> i64 {
        connection
            .query_row(
                "SELECT COUNT(1) FROM pragma_foreign_key_check;",
                [],
                |row| row.get(0),
            )
            .expect("run foreign key check")
    }

    fn schema_objects(connection: &Connection, object_type: &str) -> Vec<(String, String)> {
        connection
            .prepare(
                "SELECT name, sql FROM sqlite_master
                 WHERE type = ?1 AND sql IS NOT NULL AND name LIKE '%mail_messages%'
                 ORDER BY name;",
            )
            .expect("prepare sqlite_master query")
            .query_map([object_type], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .expect("query sqlite_master")
            .collect::<Result<Vec<_>, _>>()
            .expect("decode sqlite_master rows")
    }

    fn decomposed_view_sql(connection: &Connection) -> String {
        connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'view' AND name = 'decomposed_messages';",
                [],
                |row| row.get(0),
            )
            .expect("read decomposed_messages view sql")
    }

    fn mail_messages_rootpage(connection: &Connection) -> i64 {
        connection
            .query_row(
                "SELECT rootpage FROM sqlite_master WHERE type = 'table' AND name = 'mail_messages';",
                [],
                |row| row.get(0),
            )
            .expect("read mail_messages rootpage")
    }

    fn legacy_rows(connection: &Connection) -> Vec<LegacyRow> {
        connection
            .prepare(
                "SELECT team, agent, message_key, envelope_json, from_agent,
                        source_chat_id, destination_chat_id, message_text, summary,
                        message_at, message_id, parent_message_id, thread_mode,
                        recorded_at
                 FROM mail_messages ORDER BY team, agent, message_key;",
            )
            .expect("prepare legacy row query")
            .query_map([], |row| {
                (0..14)
                    .map(|index| row.get(index))
                    .collect::<Result<_, _>>()
            })
            .expect("query legacy rows")
            .collect::<Result<Vec<_>, _>>()
            .expect("decode legacy rows")
    }

    #[test]
    fn legacy_not_null_message_text_is_rebuilt_without_losing_rows_or_schema_objects() {
        let (target, mut connection) = legacy_mail_messages_connection("message-text-rebuild");
        connection
            .execute_batch(LEGACY_MAIL_ROWS_DDL)
            .expect("seed legacy rows");
        assert_eq!(message_text_notnull(&connection), 1);
        let before = legacy_rows(&connection);

        ensure_schema(&mut connection, &target).expect("migrate legacy database");

        assert_eq!(
            message_text_notnull(&connection),
            0,
            "the rebuild must relax message_text to nullable"
        );
        assert_eq!(
            legacy_rows(&connection),
            before,
            "every legacy column value must survive the rebuild unchanged"
        );
        let state: (i64, Option<String>) = connection
            .query_row(
                "SELECT read, acknowledged_at FROM mail_message_states
                 WHERE team = 'team-a' AND agent = 'agent-a' AND message_key = 'atm:two';",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("dependent state row must survive the foreign-key detour");
        assert_eq!(state, (1, Some("2026-07-13T00:01:00Z".to_owned())));
        assert_eq!(foreign_key_violations(&connection), 0);
        assert_eq!(
            foreign_keys_enabled(&connection),
            1,
            "foreign key enforcement must be restored after the rebuild"
        );

        let (_fresh_target, fresh) = fresh_schema_connection("message-text-fresh");
        assert_eq!(
            schema_objects(&connection, "index"),
            schema_objects(&fresh, "index"),
            "a migrated database must be index-identical to a fresh one"
        );
        assert_eq!(
            decomposed_view_sql(&connection),
            decomposed_view_sql(&fresh),
            "the catalog view must be recreated exactly as a fresh database defines it"
        );
    }

    #[test]
    fn rebuilt_mail_messages_accepts_the_decomposed_admission_null_update() {
        let (target, mut connection) = legacy_mail_messages_connection("message-text-null-update");
        connection
            .execute_batch(LEGACY_MAIL_ROWS_DDL)
            .expect("seed legacy rows");
        ensure_schema(&mut connection, &target).expect("migrate legacy database");

        // Mirrors the decomposed admission write in `writer::ops`, which is the
        // exact statement that failed on legacy databases.
        let changed = connection
            .execute(
                "UPDATE mail_messages
                 SET template_sha = 'sha-1', vars_json = '{}', category = 'assignment',
                     tags_json = '[]', content_format = 'markdown', message_text = NULL
                 WHERE message_key = 'atm:one' AND template_sha IS NULL",
                [],
            )
            .expect("decomposed admission must be able to null message_text");
        assert_eq!(changed, 1);
        let stored: Option<String> = connection
            .query_row(
                "SELECT message_text FROM mail_messages WHERE message_key = 'atm:one';",
                [],
                |row| row.get(0),
            )
            .expect("read migrated row");
        assert_eq!(stored, None);
    }

    #[test]
    fn message_text_rebuild_is_skipped_once_the_column_is_already_nullable() {
        let (target, mut connection) = legacy_mail_messages_connection("message-text-idempotent");
        connection
            .execute_batch(LEGACY_MAIL_ROWS_DDL)
            .expect("seed legacy rows");
        ensure_schema(&mut connection, &target).expect("first migration");
        let migrated_rootpage = mail_messages_rootpage(&connection);

        ensure_schema(&mut connection, &target).expect("second migration");
        assert_eq!(
            mail_messages_rootpage(&connection),
            migrated_rootpage,
            "a migrated database must not be rebuilt a second time"
        );

        let (fresh_target, mut fresh) = fresh_schema_connection("message-text-fresh-idempotent");
        let fresh_rootpage = mail_messages_rootpage(&fresh);
        ensure_schema(&mut fresh, &fresh_target).expect("re-run fresh schema");
        assert_eq!(
            mail_messages_rootpage(&fresh),
            fresh_rootpage,
            "a fresh database must never be rebuilt"
        );
    }

    #[test]
    fn message_text_rebuild_preserves_the_message_id_uniqueness_guarantee() {
        let (target, mut connection) = legacy_mail_messages_connection("message-text-unique");
        connection
            .execute_batch(LEGACY_MAIL_ROWS_DDL)
            .expect("seed legacy rows");
        let duplicate = "INSERT INTO mail_messages(
                team, agent, message_key, envelope_json, from_agent, message_text,
                message_at, message_id
             ) VALUES ('team-a', 'agent-a', 'atm:duplicate', '{}', 'sender', 'body',
                       '2026-07-13T00:00:05Z', 'msg-1');";
        connection
            .execute_batch(duplicate)
            .expect_err("legacy unique index must reject a duplicate message_id");

        ensure_schema(&mut connection, &target).expect("migrate legacy database");

        connection
            .execute_batch(duplicate)
            .expect_err("the recreated unique index must still reject a duplicate message_id");
    }

    #[test]
    fn message_text_rebuild_handles_an_empty_legacy_table() {
        let (target, mut connection) = legacy_mail_messages_connection("message-text-empty");
        ensure_schema(&mut connection, &target).expect("migrate empty legacy database");

        assert_eq!(message_text_notnull(&connection), 0);
        assert_eq!(
            connection
                .query_row("SELECT COUNT(1) FROM mail_messages;", [], |row| row
                    .get::<_, i64>(0))
                .expect("count migrated rows"),
            0
        );
        assert_eq!(foreign_keys_enabled(&connection), 1);
    }

    #[test]
    fn message_text_rebuild_lands_the_fresh_column_list_in_the_fresh_order() {
        let (target, mut connection) = legacy_mail_messages_connection("message-text-columns");
        connection
            .execute_batch(LEGACY_MAIL_ROWS_DDL)
            .expect("seed legacy rows");
        assert!(
            !table_column_names(&connection, &target, "mail_messages")
                .expect("legacy columns")
                .contains(&"template_sha".to_owned()),
            "the legacy fixture must predate the additive template columns"
        );

        ensure_schema(&mut connection, &target).expect("migrate legacy database");

        let (fresh_target, fresh) = fresh_schema_connection("message-text-columns-fresh");
        assert_eq!(
            table_column_names(&connection, &target, "mail_messages").expect("migrated columns"),
            table_column_names(&fresh, &fresh_target, "mail_messages").expect("fresh columns"),
            "a migrated table must expose the fresh column list in the fresh order"
        );
    }

    #[test]
    fn a_failed_message_text_rebuild_rolls_back_and_restores_foreign_keys() {
        let (target, mut connection) = legacy_mail_messages_connection("message-text-rollback");
        connection
            .execute_batch(LEGACY_MAIL_ROWS_DDL)
            .expect("seed legacy rows");
        let before = legacy_rows(&connection);
        // Occupying the staging name makes the rebuild's `CREATE TABLE` fail
        // after the view drop, which is the riskiest point to roll back from.
        connection
            .execute_batch("CREATE TABLE mail_messages_nullable_rebuild (probe TEXT);")
            .expect("occupy the rebuild staging table name");

        let failure = ensure_schema(&mut connection, &target)
            .expect_err("a blocked rebuild must surface as an error");
        assert!(
            failure
                .message()
                .contains("failed to create the rebuilt mail_messages table"),
            "the failing stage must be identifiable: {}",
            failure.message()
        );

        assert_eq!(
            message_text_notnull(&connection),
            1,
            "the legacy table must be left untouched so a later startup retries"
        );
        assert_eq!(legacy_rows(&connection), before);
        assert_eq!(foreign_keys_enabled(&connection), 1);
        assert!(
            connection
                .query_row(
                    "SELECT COUNT(1) FROM sqlite_master WHERE type = 'view' AND name = 'decomposed_messages';",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("count views")
                > 0,
            "the rollback must restore the dropped catalog view"
        );
    }

    #[test]
    fn a_legacy_table_with_an_unknown_column_is_rejected_instead_of_losing_data() {
        let (target, mut connection) = legacy_mail_messages_connection("message-text-extra-column");
        connection
            .execute_batch("ALTER TABLE mail_messages ADD COLUMN operator_annotation TEXT NULL;")
            .expect("add an unexpected legacy column");
        connection
            .execute_batch(LEGACY_MAIL_ROWS_DDL)
            .expect("seed legacy rows");
        let before = legacy_rows(&connection);

        let failure = ensure_schema(&mut connection, &target)
            .expect_err("an unknown legacy column must block the rebuild");
        assert!(
            failure.message().contains("operator_annotation"),
            "the error must name the column that would be dropped: {}",
            failure.message()
        );

        assert_eq!(message_text_notnull(&connection), 1);
        assert_eq!(legacy_rows(&connection), before);
        assert_eq!(foreign_keys_enabled(&connection), 1);
        assert!(
            table_column_names(&connection, &target, "mail_messages")
                .expect("columns")
                .contains(&"operator_annotation".to_owned())
        );
    }
}
