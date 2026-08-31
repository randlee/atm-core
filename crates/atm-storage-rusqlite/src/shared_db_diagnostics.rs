//! SQLite diagnostic sampling kept separate from the durable writer surface.

use crate::observability::{SqliteObservabilityEvent, SqliteObservabilityOutcome};
use crate::reader_pool::ReaderLanesMetricsSnapshot;
#[cfg(test)]
use crate::shared_db::SharedDbTarget;
use crate::shared_db::{SharedDb, sqlite_error};
use atm_storage::AtmError;

impl SharedDb {
    pub(crate) fn reader_lane_metrics(&self) -> ReaderLanesMetricsSnapshot {
        ReaderLanesMetricsSnapshot {
            mailbox: self.mailbox_reader_metrics.snapshot(),
            search: self.search_reader.metrics(),
        }
    }

    pub(crate) fn checkpoint_wal(&self) -> Result<(), AtmError> {
        #[cfg(test)]
        if matches!(self.target.as_ref(), SharedDbTarget::InMemory { .. }) {
            return Ok(());
        }
        let result = self.with_connection(|connection| {
            connection
                .query_row("PRAGMA wal_checkpoint(PASSIVE);", [], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
                })
                .map_err(|error| {
                    sqlite_error(
                        self.target.as_ref(),
                        "failed to checkpoint sqlite wal during daemon shutdown",
                        error,
                    )
                })
        });
        match &result {
            Ok((busy, frames)) => {
                let succeeded = *busy == 0;
                let frames = u64::try_from(*frames).unwrap_or(u64::MAX);
                self.mailbox_reader_metrics
                    .record_wal_health(succeeded, frames);
                self.search_reader.record_wal_health(succeeded, frames);
            }
            Err(error) => self
                .observability
                .emit_or_warn(SqliteObservabilityEvent::new(
                    "wal_checkpoint",
                    SqliteObservabilityOutcome::Failed,
                    error.message().to_owned(),
                    Some(error.code()),
                )),
        }
        result.map(|_| ())
    }
}
