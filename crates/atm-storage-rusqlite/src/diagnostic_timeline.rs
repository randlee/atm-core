//! SQLite implementation of AW.2's lower-priority diagnostic timeline.
//!
//! Timeline writes deliberately reuse the durable state's one writer worker;
//! opening an independent writer connection here would let best-effort
//! telemetry contend with mailbox durability.

#[cfg(test)]
use std::path::Path;
use std::sync::Arc;

use atm_storage::{
    AtmError, DIAGNOSTIC_QUERY_DEFAULT_LIMIT, DIAGNOSTIC_QUERY_MAX_LIMIT, DiagnosticEvent,
    DiagnosticQuery, DiagnosticRecordError, DiagnosticTimelineStore,
};
use rusqlite::params;

#[cfg(test)]
use crate::observability::PassiveSqliteObservability;
use crate::shared_db::SharedDb;
use crate::writer::DiagnosticBatchOffer;

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

    fn offer_batch(&self, events: Vec<DiagnosticEvent>) -> Result<(), DiagnosticRecordError> {
        match self.db.writer.try_record_diagnostics(events) {
            DiagnosticBatchOffer::Accepted => Ok(()),
            DiagnosticBatchOffer::QueueFull => Err(DiagnosticRecordError::QueueFull),
            DiagnosticBatchOffer::WriterClosed => Err(DiagnosticRecordError::WriterClosed),
            DiagnosticBatchOffer::InvalidBatch => Err(DiagnosticRecordError::InvalidBatch),
        }
    }
}

impl DiagnosticTimelineStore for SqliteDiagnosticTimeline {
    fn record_batch(&self, events: &[DiagnosticEvent]) -> Result<(), DiagnosticRecordError> {
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
            // The route layer requests up to `DIAGNOSTIC_QUERY_MAX_LIMIT + 1`
            // rows (one extra "peek" row) to detect truncation without a
            // second query; clamp the ceiling here to match so a caller can
            // never bypass the shared cap by asking for more directly.
            let limit = query
                .limit
                .unwrap_or(DIAGNOSTIC_QUERY_DEFAULT_LIMIT)
                .min(DIAGNOSTIC_QUERY_MAX_LIMIT + 1) as i64;
            let (cursor_ts, cursor_id) = query
                .cursor
                .map(|cursor| (Some(cursor.ts_unix_ms), Some(cursor.id)))
                .unwrap_or((None, None));
            let mut statement = connection
                .prepare(
                    "SELECT id, ts_unix_ms, level, component, code, correlation_id, origin, \
                     message, detail FROM diagnostic_events \
                     WHERE (?1 IS NULL OR ts_unix_ms >= ?1) \
                     AND (?2 IS NULL OR ts_unix_ms <= ?2) \
                     AND (?3 IS NULL OR CASE lower(level) \
                         WHEN 'trace' THEN 0 WHEN 'debug' THEN 1 WHEN 'info' THEN 2 \
                         WHEN 'warn' THEN 3 WHEN 'error' THEN 4 ELSE -1 END >= ?3) \
                     AND (?4 IS NULL OR component LIKE (?4 || '%') ESCAPE '\\') \
                     AND (?5 IS NULL OR ts_unix_ms < ?5 OR (ts_unix_ms = ?5 AND id < ?6)) \
                     ORDER BY ts_unix_ms DESC, id DESC LIMIT ?7",
                )
                .map_err(|error| AtmError::mailbox_read(error.to_string()))?;
            statement
                .query_map(
                    params![
                        query.since,
                        query.until,
                        query.level_at_least,
                        query.component_prefix,
                        cursor_ts,
                        cursor_id,
                        limit
                    ],
                    |row| {
                        Ok(DiagnosticEvent {
                            id: row.get(0)?,
                            ts_unix_ms: row.get(1)?,
                            level: row.get(2)?,
                            component: row.get(3)?,
                            code: row.get(4)?,
                            correlation_id: row.get(5)?,
                            origin: row.get(6)?,
                            message: row.get(7)?,
                            detail: row.get(8)?,
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
                id: 0,
            }])
            .expect("non-blocking diagnostic offer");
        // The diagnostic writer lane is a single FIFO channel: `prune()`
        // sends a `Prune{reply}` message on that same channel and blocks on
        // its reply, so once it returns, every message queued ahead of it
        // (including our `record_batch` offer above) has already been
        // applied. That gives a deterministic synchronization point with no
        // clock or poll loop; `now_unix_ms = 0` prunes nothing (the cutoff is
        // seven days before the epoch), so the seeded row survives.
        timeline.prune(0).expect("prune reply drains the FIFO lane");
        let rows = timeline
            .query(&DiagnosticQuery::default())
            .expect("timeline query");
        assert!(
            rows.iter()
                .any(|row| row.code.as_deref() == Some("ATM_TEST")),
            "diagnostic writer did not persist the batch before the prune reply: {rows:?}"
        );
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

        let mut deleted = 0;
        while deleted < (FIXTURE_ROWS - DIAGNOSTIC_MAX_ROWS) as u64 {
            let pass = timeline.prune(FIXTURE_ROWS as i64).expect("prune fixture");
            assert!(pass > 0, "each retention pass must make progress");
            assert!(
                pass <= crate::DIAGNOSTIC_PRUNE_BATCH as u64,
                "one writer-lane maintenance pass must remain bounded"
            );
            deleted += pass;
        }
        let (retained, oldest_ts, newest_ts): (i64, i64, i64) = timeline
            .db
            .with_connection(|connection| {
                connection
                    .query_row(
                        "SELECT COUNT(*), MIN(ts_unix_ms), MAX(ts_unix_ms) FROM diagnostic_events",
                        [],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .map_err(|error| AtmError::mailbox_read(error.to_string()))
            })
            .expect("count retained diagnostics");

        assert_eq!(deleted, (FIXTURE_ROWS - DIAGNOSTIC_MAX_ROWS) as u64);
        assert_eq!(retained, DIAGNOSTIC_MAX_ROWS as i64);
        assert_eq!(oldest_ts, (FIXTURE_ROWS - DIAGNOSTIC_MAX_ROWS) as i64);
        assert_eq!(newest_ts, (FIXTURE_ROWS - 1) as i64);
    }

    #[test]
    fn query_clamps_a_requested_limit_above_the_shared_max_plus_peek_row() {
        let directory = tempdir().expect("temporary database directory");
        let timeline = SqliteDiagnosticTimeline::open(directory.path().join("mail.db"))
            .expect("timeline opens and migrates");
        timeline
            .db
            .with_connection(|connection| {
                for index in 0..10 {
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

        let rows = timeline
            .query(&DiagnosticQuery {
                limit: Some(50_000),
                ..DiagnosticQuery::default()
            })
            .expect("query with an out-of-range limit");
        assert_eq!(rows.len(), 10, "clamp must never drop in-range rows");
    }

    #[test]
    fn cursor_pagination_visits_every_row_exactly_once_in_stable_order() {
        use atm_storage::DiagnosticCursor;

        const FIXTURE_ROWS: i64 = 25;
        const PAGE_SIZE: usize = 4;
        let directory = tempdir().expect("temporary database directory");
        let timeline = SqliteDiagnosticTimeline::open(directory.path().join("mail.db"))
            .expect("timeline opens and migrates");
        timeline
            .db
            .with_connection(|connection| {
                for index in 0..FIXTURE_ROWS {
                    // Two rows deliberately share one timestamp to exercise
                    // the `(ts_unix_ms, id)` tie-break.
                    let ts = index / 2;
                    connection
                        .execute(
                            "INSERT INTO diagnostic_events (ts_unix_ms, level, component, origin, message) VALUES (?1, 'info', 'fixture', 'test', 'fixture')",
                            params![ts],
                        )
                        .map_err(|error| AtmError::mailbox_write(error.to_string()))?;
                }
                Ok(())
            })
            .expect("seed diagnostic fixture");

        let mut seen_ids = Vec::new();
        let mut cursor: Option<DiagnosticCursor> = None;
        loop {
            let rows = timeline
                .query(&DiagnosticQuery {
                    limit: Some(PAGE_SIZE),
                    cursor,
                    ..DiagnosticQuery::default()
                })
                .expect("paginated query");
            if rows.is_empty() {
                break;
            }
            assert!(
                rows.len() <= PAGE_SIZE,
                "a page must never exceed its requested size"
            );
            seen_ids.extend(rows.iter().map(|row| row.id));
            let last = rows.last().expect("non-empty page has a last row");
            cursor = Some(DiagnosticCursor {
                ts_unix_ms: last.ts_unix_ms,
                id: last.id,
            });
            if rows.len() < PAGE_SIZE {
                break;
            }
        }

        seen_ids.sort_unstable();
        seen_ids.dedup();
        assert_eq!(
            seen_ids.len(),
            FIXTURE_ROWS as usize,
            "keyset pagination must visit every row exactly once, including same-timestamp ties"
        );
    }
}
