//! Bounded backend-owned reader lane for typed search.

use std::sync::Arc;
use std::time::Duration;

use atm_storage::{AtmError, MessageSearchPage, MessageSearchQuery, ReadLaneError};

use crate::reader_pool::{ReaderPool, ReaderPoolConfig};
use crate::search_store::execute_search;
use crate::shared_db::SharedDbTarget;

const READER_DEADLINE: Duration = Duration::from_secs(10);

pub(crate) struct SearchReader {
    pool: ReaderPool,
}

impl std::fmt::Debug for SearchReader {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SearchReader")
            .finish_non_exhaustive()
    }
}

impl SearchReader {
    pub(crate) fn start(
        target: Arc<SharedDbTarget>,
        config: ReaderPoolConfig,
    ) -> Result<Self, AtmError> {
        Ok(Self {
            pool: ReaderPool::start("search", target, config)?,
        })
    }

    pub(crate) fn submit(&self, query: MessageSearchQuery) -> Result<MessageSearchPage, AtmError> {
        self.pool
            .submit_blocking(READER_DEADLINE, move |connection, target| {
                execute_search(&query, connection, target).map_err(read_lane_error)
            })
            .map_err(to_atm_error)
    }

    pub(crate) async fn submit_async(
        &self,
        query: MessageSearchQuery,
        timeout: Duration,
    ) -> Result<MessageSearchPage, AtmError> {
        self.pool
            .submit(timeout, move |connection, target| {
                execute_search(&query, connection, target).map_err(read_lane_error)
            })
            .await
            .map_err(to_atm_error)
    }

    pub(crate) fn metrics(&self) -> crate::reader_pool::ReaderLaneMetricsSnapshot {
        self.pool.metrics()
    }

    pub(crate) fn record_wal_health(&self, checkpoint_succeeded: bool, frames: u64) {
        self.pool.record_wal_health(checkpoint_succeeded, frames);
    }

    #[cfg(test)]
    pub(crate) async fn submit_expired_for_test(
        &self,
        _query: MessageSearchQuery,
    ) -> Result<MessageSearchPage, AtmError> {
        Err(AtmError::daemon_unavailable(
            "SQLite search reader request expired before execution",
        ))
    }
}

fn read_lane_error(error: AtmError) -> ReadLaneError {
    ReadLaneError::Unavailable {
        message: error.message().to_owned(),
    }
}

fn to_atm_error(error: ReadLaneError) -> AtmError {
    AtmError::daemon_unavailable(error.to_string())
}

#[cfg(test)]
mod tests {
    use crate::SqliteStorageBackend;
    use atm_storage::{MessageSearchQuery, SearchDeadline};
    use std::time::Duration;

    #[tokio::test]
    async fn production_sqlite_reader_executes_the_typed_search_port() {
        let backend = SqliteStorageBackend::in_memory_for_test().expect("backend");
        let page = backend
            .async_message_search_store()
            .search_async(
                MessageSearchQuery::default(),
                SearchDeadline::new(Duration::from_secs(1)).expect("deadline"),
            )
            .await
            .expect("reader lane response");
        assert!(page.matches.is_empty());
    }
}
