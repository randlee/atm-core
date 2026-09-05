//! Reader-lane assembly and defensive connection construction.
//!
//! This is deliberately separate from `shared_db`: the durable writer surface
//! must not grow with reader-lane composition details.

use crate::control_path_pool::ControlPathConnections;
use crate::mailbox_reader::{MailboxReaderMetrics, start_mailbox_reader_from_pool};
#[cfg(test)]
use crate::observability::NullSqliteObservability;
use crate::observability::SqliteObservability;
use crate::reader_pool::{ReaderLanesConfig, ReaderPool};
use crate::search_reader::SearchReader;
#[cfg(test)]
use crate::shared_db::record_opened_connection;
use crate::shared_db::{SharedDbTarget, configure_connection, sqlite_error, sqlite_open_error};
use crate::task_ledger_reader::TaskLedgerReader;
use crate::writer::SqliteWriter;
use atm_storage::{AsyncMailboxReader, AsyncTaskLedgerReader, AtmError};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(test)]
static NEXT_IN_MEMORY_DB_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub(crate) struct SharedDb {
    pub(crate) target: Arc<SharedDbTarget>,
    pub(crate) writer: Arc<SqliteWriter>,
    pub(crate) control_path: Arc<ControlPathConnections>,
    pub(crate) search_reader: Arc<SearchReader>,
    pub(crate) mailbox_reader: Arc<dyn AsyncMailboxReader + Send + Sync>,
    pub(crate) task_ledger_reader: Arc<dyn AsyncTaskLedgerReader + Send + Sync>,
    pub(crate) mailbox_reader_metrics: MailboxReaderMetrics,
    pub(crate) observability: Arc<dyn SqliteObservability>,
}

impl SharedDb {
    pub(crate) fn target(&self) -> &SharedDbTarget {
        self.target.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn open_in_memory_for_test() -> Result<Self, AtmError> {
        Self::open_in_memory_with_observability(Arc::new(NullSqliteObservability))
    }

    #[cfg(test)]
    pub(crate) fn open_in_memory_with_observability(
        observability: Arc<dyn SqliteObservability>,
    ) -> Result<Self, AtmError> {
        Self::open_in_memory_with_reader_lanes(observability, ReaderLanesConfig::default())
    }

    #[cfg(test)]
    pub(crate) fn open_in_memory_with_reader_lanes(
        observability: Arc<dyn SqliteObservability>,
        reader_lanes: ReaderLanesConfig,
    ) -> Result<Self, AtmError> {
        reader_lanes.validate()?;
        let target = Arc::new(SharedDbTarget::InMemory {
            uri: format!(
                "file:atm-storage-rusqlite-{}?mode=memory&cache=shared",
                NEXT_IN_MEMORY_DB_ID.fetch_add(1, Ordering::Relaxed)
            ),
        });
        Self::assemble(target, observability, reader_lanes)
    }

    #[allow(
        dead_code,
        reason = "Production construction delegates through ReaderLanesConfig; this direct observability constructor remains a backend test seam."
    )]
    pub(crate) fn open_with_observability(
        path: impl AsRef<Path>,
        observability: Arc<dyn SqliteObservability>,
    ) -> Result<Self, AtmError> {
        Self::open_with_reader_lanes(path, observability, ReaderLanesConfig::default())
    }

    pub(crate) fn open_with_reader_lanes(
        path: impl AsRef<Path>,
        observability: Arc<dyn SqliteObservability>,
        reader_lanes: ReaderLanesConfig,
    ) -> Result<Self, AtmError> {
        reader_lanes.validate()?;
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                AtmError::mailbox_write(format!(
                    "failed to create sqlite parent directory {}: {error}",
                    parent.display()
                ))
            })?;
        }
        Self::assemble(
            Arc::new(SharedDbTarget::Path(path)),
            observability,
            reader_lanes,
        )
    }

    fn assemble(
        target: Arc<SharedDbTarget>,
        observability: Arc<dyn SqliteObservability>,
        reader_lanes: ReaderLanesConfig,
    ) -> Result<Self, AtmError> {
        let writer = Arc::new(SqliteWriter::start(
            Arc::clone(&target),
            Arc::clone(&observability),
        )?);
        let search_reader = Arc::new(SearchReader::start(
            Arc::clone(&target),
            reader_lanes.search,
        )?);
        let mailbox_pool = ReaderPool::start("mailbox", Arc::clone(&target), reader_lanes.mailbox)?;
        let (mailbox_reader, mailbox_reader_metrics) =
            start_mailbox_reader_from_pool(mailbox_pool.clone());
        let task_ledger_reader = Arc::new(TaskLedgerReader::from_pool(mailbox_pool));
        tracing::debug!(
            writer_handles = 1,
            path = %target.display(),
            "sqlite boundary assembly opened"
        );
        Ok(Self {
            control_path: Arc::new(ControlPathConnections::new(Arc::clone(&target))),
            target,
            writer,
            search_reader,
            mailbox_reader,
            task_ledger_reader,
            mailbox_reader_metrics,
            observability,
        })
    }
}

/// Opens a connection which is physically read-only for durable databases and
/// configured defensively for the bounded reader lanes.
pub(crate) fn open_read_connection_for_target(
    target: &SharedDbTarget,
) -> Result<Connection, AtmError> {
    let mut connection = match target {
        SharedDbTarget::Path(path) => Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| sqlite_open_error(target, error))?,
        #[cfg(test)]
        SharedDbTarget::InMemory { uri } => Connection::open_with_flags(
            uri,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| sqlite_open_error(target, error))?,
    };
    configure_connection(&mut connection, target)?;
    connection
        .execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF; PRAGMA defensive=ON;")
        .map_err(|error| {
            sqlite_error(
                target,
                "failed to configure defensive read-only sqlite connection",
                error,
            )
        })?;
    #[cfg(test)]
    record_opened_connection(target);
    Ok(connection)
}
