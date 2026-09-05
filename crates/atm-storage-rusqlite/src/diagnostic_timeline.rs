//! SQLite implementation of AW.2's lower-priority diagnostic timeline.
//!
//! Timeline writes deliberately reuse the durable state's one writer worker;
//! opening an independent writer connection here would let best-effort
//! telemetry contend with mailbox durability.

#[cfg(test)]
use std::path::Path;
use std::sync::Arc;

use atm_storage::{AtmError, DiagnosticEvent, DiagnosticQuery, DiagnosticTimelineStore};
use rusqlite::params;

#[cfg(test)]
use crate::observability::PassiveSqliteObservability;
use crate::shared_db::SharedDb;
use crate::writer::{DIAGNOSTIC_BATCH_MAX, DiagnosticBatchOffer};

pub const DIAGNOSTIC_DETAIL_MAX_BYTES: usize = 1024;
pub const DIAGNOSTIC_MAX_ROWS: usize = 20_000;
pub const DIAGNOSTIC_MAX_AGE_DAYS: i64 = 7;
pub const DIAGNOSTIC_PRUNE_BATCH: usize = 1000;
pub const DIAGNOSTIC_PRUNE_CHECK_EVERY: usize = 500;

#[derive(Clone)]
pub struct SqliteDiagnosticTimeline {
    db: Arc<SharedDb>,
}

impl std::fmt::Debug for SqliteDiagnosticTimeline {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SqliteDiagnosticTimeline")
            .field("target", &self.db.target().display())
            .finish_non_exhaustive()
    }
}

impl SqliteDiagnosticTimeline {
    /// Opens a stand-alone timeline using the same one-writer topology as the
    /// storage backend. Production callers should use the backend accessor so
    /// the timeline shares that backend's writer worker.
    #[cfg(test)]
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self, AtmError> {
        let db = Arc::new(SharedDb::open_with_observability(
            path,
            Arc::new(PassiveSqliteObservability),
        )?);
        Ok(Self { db })
    }

    pub(crate) fn from_shared_db(db: Arc<SharedDb>) -> Self {
        Self { db }
    }

    pub fn persistence_stats(&self) -> Arc<crate::writer::DiagnosticTimelinePersistenceStats> {
        self.db.writer.diagnostic_stats()
    }

    fn offer_batch(&self, events: Vec<DiagnosticEvent>) -> Result<(), AtmError> {
        match self.db.writer.try_record_diagnostics(events) {
            DiagnosticBatchOffer::Accepted => Ok(()),
            DiagnosticBatchOffer::QueueFull => Err(AtmError::daemon_unavailable(
                "diagnostic timeline queue is full; batch dropped",
            )),
            DiagnosticBatchOffer::WriterClosed => Err(AtmError::daemon_unavailable(
                "diagnostic timeline writer is unavailable; batch dropped",
            )),
            DiagnosticBatchOffer::InvalidBatch => Err(AtmError::mailbox_write(format!(
                "diagnostic timeline batch must contain 1..={DIAGNOSTIC_BATCH_MAX} events",
            ))),
        }
    }
}

impl DiagnosticTimelineStore for SqliteDiagnosticTimeline {
    fn record_batch(&self, events: &[DiagnosticEvent]) -> Result<(), AtmError> {
        let events = events
            .iter()
            .cloned()
            .map(|mut event| {
                event.detail = event.detail.as_deref().map(truncate_detail);
                event
            })
            .collect();
        self.offer_batch(events)
    }

    fn query(&self, query: &DiagnosticQuery) -> Result<Vec<DiagnosticEvent>, AtmError> {
        self.db.with_connection(|connection| {
            let limit = query.limit.unwrap_or(100).min(1_000) as i64;
            let mut statement = connection.prepare("SELECT ts_unix_ms, level, component, code, correlation_id, origin, message, detail FROM diagnostic_events WHERE (?1 IS NULL OR ts_unix_ms >= ?1) AND (?2 IS NULL OR ts_unix_ms <= ?2) AND (?3 IS NULL OR level >= ?3) AND (?4 IS NULL OR component LIKE (?4 || '%')) ORDER BY ts_unix_ms DESC LIMIT ?5").map_err(|error| AtmError::mailbox_read(error.to_string()))?;
            statement
                .query_map(
                    params![
                        query.since,
                        query.until,
                        query.level_at_least,
                        query.component_prefix,
                        limit
                    ],
                    |row| {
                        Ok(DiagnosticEvent {
                            ts_unix_ms: row.get(0)?,
                            level: row.get(1)?,
                            component: row.get(2)?,
                            code: row.get(3)?,
                            correlation_id: row.get(4)?,
                            origin: row.get(5)?,
                            message: row.get(6)?,
                            detail: row.get(7)?,
                        })
                    },
                )
                .map_err(|error| AtmError::mailbox_read(error.to_string()))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| AtmError::mailbox_read(error.to_string()))
        })
    }

    fn prune(&self, now_unix_ms: i64) -> Result<u64, AtmError> {
        self.db.writer.prune_diagnostics(now_unix_ms)
    }
}

pub(crate) fn truncate_detail(detail: &str) -> String {
    if detail.len() <= DIAGNOSTIC_DETAIL_MAX_BYTES {
        return detail.to_owned();
    }
    let marker = "…";
    let budget = DIAGNOSTIC_DETAIL_MAX_BYTES - marker.len();
    let boundary = detail
        .char_indices()
        .take_while(|(index, _)| *index <= budget)
        .map(|(index, _)| index)
        .last()
        .unwrap_or_default();
    format!("{}{marker}", &detail[..boundary])
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{
        DIAGNOSTIC_DETAIL_MAX_BYTES, DIAGNOSTIC_MAX_ROWS, SqliteDiagnosticTimeline, truncate_detail,
    };
    use atm_storage::{AtmError, DiagnosticEvent, DiagnosticQuery, DiagnosticTimelineStore};
    use rusqlite::params;
    use tempfile::tempdir;

    #[test]
    fn detail_truncation_is_utf8_safe_and_bounded() {
        let value = "é".repeat(DIAGNOSTIC_DETAIL_MAX_BYTES);
        let truncated = truncate_detail(&value);
        assert!(truncated.len() <= DIAGNOSTIC_DETAIL_MAX_BYTES);
        assert!(truncated.ends_with('…'));
    }

    #[test]
    fn fresh_database_migrates_and_writer_lane_persists_a_diagnostic() {
        let directory = tempdir().expect("temporary database directory");
        let timeline = SqliteDiagnosticTimeline::open(directory.path().join("mail.db"))
            .expect("timeline opens and migrates");
        timeline
            .record_batch(&[DiagnosticEvent {
                ts_unix_ms: 42,
                level: "warn".to_owned(),
                component: "timeline-test".to_owned(),
                code: Some("ATM_TEST".to_owned()),
                correlation_id: None,
                origin: "tracing".to_owned(),
                message: "bounded diagnostic".to_owned(),
                detail: None,
            }])
            .expect("non-blocking diagnostic offer");
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let rows = timeline
                .query(&DiagnosticQuery::default())
                .expect("timeline query");
            if rows
                .iter()
                .any(|row| row.code.as_deref() == Some("ATM_TEST"))
            {
                break;
            }
            assert!(Instant::now() < deadline, "diagnostic writer did not drain");
            std::thread::yield_now();
        }
    }

    #[test]
    fn ac4_prune_reduces_a_25k_fixture_to_the_documented_row_bound() {
        const FIXTURE_ROWS: usize = 25_000;
        let directory = tempdir().expect("temporary database directory");
        let timeline = SqliteDiagnosticTimeline::open(directory.path().join("mail.db"))
            .expect("timeline opens and migrates");
        timeline
            .db
            .with_connection(|connection| {
                for index in 0..FIXTURE_ROWS {
                    connection
                        .execute(
                            "INSERT INTO diagnostic_events (ts_unix_ms, level, component, origin, message) VALUES (?1, 'info', 'fixture', 'test', 'fixture')",
                            params![index as i64],
                        )
                        .map_err(|error| AtmError::mailbox_write(error.to_string()))?;
                }
                Ok(())
            })
            .expect("seed diagnostic fixture");

        let deleted = timeline.prune(FIXTURE_ROWS as i64).expect("prune fixture");
        let retained: i64 = timeline
            .db
            .with_connection(|connection| {
                connection
                    .query_row("SELECT COUNT(*) FROM diagnostic_events", [], |row| {
                        row.get(0)
                    })
                    .map_err(|error| AtmError::mailbox_read(error.to_string()))
            })
            .expect("count retained diagnostics");

        assert_eq!(deleted, (FIXTURE_ROWS - DIAGNOSTIC_MAX_ROWS) as u64);
        assert_eq!(retained, DIAGNOSTIC_MAX_ROWS as i64);
    }
}
