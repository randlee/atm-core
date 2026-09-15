//! Bounded, read-only HTTP projection of the retained diagnostic timeline.

use std::sync::Arc;
use std::time::Duration;

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use atm_core::observability_counters::{
    DIAGNOSTIC_QUERY_DEFAULT_LIMIT, DIAGNOSTIC_QUERY_MAX_LIMIT, DiagnosticTimelineRecord,
    DiagnosticTimelineResponse,
};
use atm_runtime::{DiagnosticCursor, DiagnosticTimelineStore};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

/// Re-exported so callers that must document or test the effective default
/// page size use the same identifier as the storage-layer default
/// (`atm_storage::diagnostics::DIAGNOSTIC_QUERY_DEFAULT_LIMIT`) instead of a
/// second, potentially divergent constant (AW-READY-O7 item 2).
pub const DEFAULT_DIAGNOSTICS_LIMIT: usize = DIAGNOSTIC_QUERY_DEFAULT_LIMIT;

/// Re-exported so callers that must document or test the effective cap use
/// the same identifier as the storage-layer clamp (`atm_storage::diagnostics
/// ::DIAGNOSTIC_QUERY_MAX_LIMIT`) instead of a second, potentially
/// divergent constant.
pub const MAX_DIAGNOSTICS_LIMIT: usize = DIAGNOSTIC_QUERY_MAX_LIMIT;

#[derive(Clone)]
struct DiagnosticsState {
    store: Option<Arc<dyn DiagnosticTimelineStore>>,
    /// Bounded deadline for the whole query, including the `spawn_blocking`
    /// hop to the SQLite connection pool. A read that cannot complete inside
    /// this budget is reported as unavailable rather than left to run
    /// unbounded against a large retained table.
    query_deadline: Duration,
    /// Bounds blocking diagnostic workers by their real lifetime. Unlike the
    /// HTTP admission permit, this remains held after a timed-out request
    /// returns until its non-cancellable spawn_blocking query finishes.
    query_workers: Arc<tokio::sync::Semaphore>,
    #[cfg(test)]
    worker_completion: Option<Arc<WorkerCompletion>>,
}

/// Test-only observation point for the worker-admission lifecycle.
///
/// The completion count advances only after the blocking closure explicitly
/// releases its owned semaphore permit. This makes a test waiting on the
/// count a proof that a subsequent request can acquire admission.
#[cfg(test)]
#[derive(Debug, Default)]
struct WorkerCompletion {
    completed: tokio::sync::Notify,
    completed_count: AtomicUsize,
}

#[cfg(test)]
impl WorkerCompletion {
    fn complete(&self) {
        self.completed_count.fetch_add(1, Ordering::SeqCst);
        self.completed.notify_waiters();
    }

    async fn wait_for_completed(&self, target: usize) {
        let completion = async {
            loop {
                let notified = self.completed.notified();
                if self.completed_count.load(Ordering::SeqCst) >= target {
                    return;
                }
                notified.await;
            }
        };
        tokio::time::timeout(Duration::from_secs(1), completion)
            .await
            .expect("worker completion must arrive within the test's hard bound");
    }
}

/// Adds the authenticated local diagnostics query route.
pub(crate) fn diagnostics_router(
    store: Option<Arc<dyn DiagnosticTimelineStore>>,
    query_deadline: Duration,
    max_in_flight_queries: usize,
) -> Router {
    Router::new()
        .route("/v1/diagnostics", get(query_diagnostics))
        .with_state(DiagnosticsState {
            store,
            query_deadline,
            query_workers: Arc::new(tokio::sync::Semaphore::new(max_in_flight_queries)),
            #[cfg(test)]
            worker_completion: None,
        })
}

#[cfg(test)]
fn diagnostics_router_with_worker_completion(
    store: Option<Arc<dyn DiagnosticTimelineStore>>,
    query_deadline: Duration,
    max_in_flight_queries: usize,
    worker_completion: Arc<WorkerCompletion>,
) -> Router {
    Router::new()
        .route("/v1/diagnostics", get(query_diagnostics))
        .with_state(DiagnosticsState {
            store,
            query_deadline,
            query_workers: Arc::new(tokio::sync::Semaphore::new(max_in_flight_queries)),
            worker_completion: Some(worker_completion),
        })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiagnosticsQuery {
    since: Option<i64>,
    until: Option<i64>,
    level: Option<String>,
    component: Option<String>,
    limit: Option<usize>,
    cursor: Option<String>,
}

async fn query_diagnostics(
    State(state): State<DiagnosticsState>,
    Query(query): Query<DiagnosticsQuery>,
) -> Result<Json<DiagnosticTimelineResponse>, axum::response::Response> {
    let limit = query.limit.unwrap_or(DEFAULT_DIAGNOSTICS_LIMIT);
    if limit == 0 {
        return Err(diagnostics_error(
            StatusCode::BAD_REQUEST,
            "limit must be greater than zero",
        ));
    }
    if limit > MAX_DIAGNOSTICS_LIMIT {
        return Err(diagnostics_error(
            StatusCode::BAD_REQUEST,
            format!("limit must be at most {MAX_DIAGNOSTICS_LIMIT}"),
        ));
    }
    let cursor = query
        .cursor
        .as_deref()
        .map(DiagnosticCursor::decode)
        .transpose()
        .map_err(|_| diagnostics_error(StatusCode::BAD_REQUEST, "cursor is invalid"))?;
    let query_deadline = state.query_deadline;
    let query_workers = Arc::clone(&state.query_workers);
    #[cfg(test)]
    let worker_completion = state.worker_completion.clone();
    let store = state.store.ok_or_else(|| {
        diagnostics_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "diagnostic timeline was not installed at daemon startup",
        )
    })?;
    // Request one extra "peek" row beyond the caller's limit so truncation
    // can be reported precisely without a second round trip.
    let storage_query = atm_runtime::DiagnosticQuery {
        since: query.since,
        until: query.until,
        level_at_least: query.level,
        component_prefix: query.component,
        limit: Some(limit + 1),
        cursor,
    };
    let mut events = run_bounded_diagnostics_query(
        query_deadline,
        query_workers,
        store,
        storage_query,
        #[cfg(test)]
        worker_completion,
    )
    .await?;
    let truncated = events.len() > limit;
    events.truncate(limit);
    let next_cursor = truncated.then(|| {
        let last = events
            .last()
            .expect("truncated implies at least `limit` events, and limit > 0");
        DiagnosticCursor {
            ts_unix_ms: last.ts_unix_ms,
            id: last.id,
        }
        .encode()
    });
    Ok(Json(DiagnosticTimelineResponse {
        records: events.into_iter().map(diagnostic_record).collect(),
        truncated,
        next_cursor,
    }))
}

/// Admit, execute, and bound one diagnostics query against the storage
/// backend: acquire a worker permit, run the query on the blocking pool, and
/// enforce the caller's overall deadline across that whole hop.
async fn run_bounded_diagnostics_query(
    query_deadline: Duration,
    query_workers: Arc<tokio::sync::Semaphore>,
    store: Arc<dyn DiagnosticTimelineStore>,
    storage_query: atm_runtime::DiagnosticQuery,
    #[cfg(test)] worker_completion: Option<Arc<WorkerCompletion>>,
) -> Result<Vec<atm_runtime::DiagnosticEvent>, axum::response::Response> {
    let worker_permit = query_workers.try_acquire_owned().map_err(|_| {
        diagnostics_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "diagnostic timeline query admission is saturated",
        )
    })?;
    let query_future = tokio::task::spawn_blocking(move || {
        // Keep this permit in the blocking closure so a deadline response
        // cannot release capacity while the synchronous SQLite query lives.
        let result = store.query(&storage_query);
        drop(worker_permit);
        #[cfg(test)]
        if let Some(worker_completion) = worker_completion {
            worker_completion.complete();
        }
        result
    });
    tokio::time::timeout(query_deadline, query_future)
        .await
        .map_err(|_| {
            diagnostics_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "diagnostic timeline query exceeded its bounded deadline",
            )
        })?
        .map_err(|_| {
            diagnostics_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "diagnostic timeline query worker stopped",
            )
        })?
        .map_err(|error| diagnostics_error(StatusCode::SERVICE_UNAVAILABLE, error.to_string()))
}

fn diagnostic_record(event: atm_runtime::DiagnosticEvent) -> DiagnosticTimelineRecord {
    DiagnosticTimelineRecord {
        ts_unix_ms: event.ts_unix_ms,
        level: event.level,
        component: event.component,
        code: event.code,
        correlation_id: event.correlation_id,
        origin: event.origin,
        message: event.message,
        detail: event.detail,
    }
}

fn diagnostics_error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(serde_json::json!({ "error": message.into() }))).into_response()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use atm_core::error::AtmError;
    use atm_core::observability_counters::{
        DIAGNOSTIC_QUERY_MAX_LIMIT, DiagnosticTimelineResponse,
    };
    use atm_runtime::{
        DiagnosticCursor, DiagnosticEvent, DiagnosticQuery, DiagnosticRecordError,
        DiagnosticTimelineStore,
    };
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::{WorkerCompletion, diagnostics_router, diagnostics_router_with_worker_completion};

    /// Synchronizes a fixture query with its test so a bounded-deadline test
    /// can advance paused tokio time deterministically instead of racing a
    /// real wall-clock sleep against the configured deadline (AW-READY-O7
    /// item 3: no sleeps-and-hope, explicit synchronisation only).
    ///
    /// [`FixtureStore::query`] runs inside `tokio::task::spawn_blocking`, on
    /// a dedicated OS thread independent of the paused async-runtime clock,
    /// so parking that thread on this explicitly bounded condition gate does
    /// not stall the
    /// test's executor.
    struct QueryGate {
        entered: tokio::sync::Notify,
        entered_count: AtomicUsize,
        state: std::sync::Mutex<QueryGateState>,
        release: std::sync::Condvar,
    }

    #[derive(Debug, Default)]
    struct QueryGateState {
        released: bool,
    }

    impl std::fmt::Debug for QueryGate {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("QueryGate").finish_non_exhaustive()
        }
    }

    impl QueryGate {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                entered: tokio::sync::Notify::new(),
                entered_count: AtomicUsize::new(0),
                state: std::sync::Mutex::new(QueryGateState::default()),
                release: std::sync::Condvar::new(),
            })
        }

        /// Called from the dedicated `spawn_blocking` thread: signals the
        /// test that the store call has genuinely started, then parks this
        /// thread (never the async executor) until the test releases it.
        fn enter_and_wait(&self) {
            self.entered_count.fetch_add(1, Ordering::SeqCst);
            self.entered.notify_waiters();
            let state = self.state.lock().expect("query gate state lock");
            let (state, timeout) = self
                .release
                .wait_timeout_while(state, Duration::from_secs(1), |state| !state.released)
                .expect("query gate state is not poisoned");
            assert!(state.released, "query gate was released");
            assert!(
                !timeout.timed_out(),
                "query gate release must arrive within the test's hard bound"
            );
        }

        async fn wait_for_entered(&self, target: usize) {
            loop {
                let notified = self.entered.notified();
                if self.entered_count.load(Ordering::SeqCst) >= target {
                    return;
                }
                notified.await;
            }
        }

        fn entered_count(&self) -> usize {
            self.entered_count.load(Ordering::SeqCst)
        }

        fn release(&self) {
            let mut state = self.state.lock().expect("query gate state lock");
            state.released = true;
            self.release.notify_all();
        }
    }

    #[derive(Debug, Default)]
    struct FixtureStore {
        rows: Vec<DiagnosticEvent>,
        gate: Option<Arc<QueryGate>>,
    }

    impl DiagnosticTimelineStore for FixtureStore {
        fn record_batch(&self, _events: &[DiagnosticEvent]) -> Result<(), DiagnosticRecordError> {
            unimplemented!("the diagnostics route never writes")
        }

        fn query(&self, query: &DiagnosticQuery) -> Result<Vec<DiagnosticEvent>, AtmError> {
            if let Some(gate) = &self.gate {
                gate.enter_and_wait();
            }
            let cursor = query.cursor;
            let mut rows: Vec<_> = self
                .rows
                .iter()
                .filter(|row| match cursor {
                    Some(cursor) => {
                        row.ts_unix_ms < cursor.ts_unix_ms
                            || (row.ts_unix_ms == cursor.ts_unix_ms && row.id < cursor.id)
                    }
                    None => true,
                })
                .cloned()
                .collect();
            rows.sort_by(|a, b| (b.ts_unix_ms, b.id).cmp(&(a.ts_unix_ms, a.id)));
            rows.truncate(query.limit.unwrap_or(usize::MAX));
            Ok(rows)
        }

        fn prune(&self, _now_unix_ms: i64) -> Result<u64, AtmError> {
            unimplemented!("the diagnostics route never prunes")
        }
    }

    fn fixture_event(id: i64, ts_unix_ms: i64) -> DiagnosticEvent {
        DiagnosticEvent {
            ts_unix_ms,
            level: "info".to_owned(),
            component: "fixture".to_owned(),
            code: None,
            correlation_id: None,
            origin: "test".to_owned(),
            message: format!("event {id}"),
            detail: None,
            id,
        }
    }

    async fn get(router: axum::Router, path: &str) -> (StatusCode, Vec<u8>) {
        let response = router
            .oneshot(Request::get(path).body(Body::empty()).expect("request"))
            .await
            .expect("response");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        (status, bytes.to_vec())
    }

    #[tokio::test]
    async fn rejects_a_limit_above_the_shared_max_cap() {
        let store: std::sync::Arc<dyn DiagnosticTimelineStore> =
            std::sync::Arc::new(FixtureStore::default());
        let router = diagnostics_router(Some(store), Duration::from_secs(5), 8);
        let (status, _) = get(
            router,
            &format!("/v1/diagnostics?limit={}", DIAGNOSTIC_QUERY_MAX_LIMIT + 1),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn accepts_a_limit_exactly_at_the_shared_max_cap() {
        let store: std::sync::Arc<dyn DiagnosticTimelineStore> =
            std::sync::Arc::new(FixtureStore::default());
        let router = diagnostics_router(Some(store), Duration::from_secs(5), 8);
        let (status, _) = get(
            router,
            &format!("/v1/diagnostics?limit={DIAGNOSTIC_QUERY_MAX_LIMIT}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn truncated_flag_and_cursor_page_through_every_row_once() {
        let rows: Vec<_> = (0..7).map(|id| fixture_event(id, 100 - id)).collect();
        let store: std::sync::Arc<dyn DiagnosticTimelineStore> =
            std::sync::Arc::new(FixtureStore { rows, gate: None });
        let router = diagnostics_router(Some(store), Duration::from_secs(5), 8);

        let (status, body) = get(router.clone(), "/v1/diagnostics?limit=3").await;
        assert_eq!(status, StatusCode::OK);
        let page1: DiagnosticTimelineResponse =
            serde_json::from_slice(&body).expect("first page JSON");
        assert_eq!(page1.records.len(), 3);
        assert!(page1.truncated, "more rows remain after the first page");
        let cursor1 = page1.next_cursor.expect("truncated page carries a cursor");

        let (status, body) = get(
            router.clone(),
            &format!("/v1/diagnostics?limit=3&cursor={cursor1}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let page2: DiagnosticTimelineResponse =
            serde_json::from_slice(&body).expect("second page JSON");
        assert_eq!(page2.records.len(), 3);
        assert!(page2.truncated, "one row remains after the second page");
        let cursor2 = page2.next_cursor.expect("truncated page carries a cursor");

        let (status, body) =
            get(router, &format!("/v1/diagnostics?limit=3&cursor={cursor2}")).await;
        assert_eq!(status, StatusCode::OK);
        let page3: DiagnosticTimelineResponse =
            serde_json::from_slice(&body).expect("third page JSON");
        assert_eq!(page3.records.len(), 1);
        assert!(
            !page3.truncated,
            "the last page must not claim further truncation"
        );
        assert!(page3.next_cursor.is_none());

        let mut all_messages: Vec<_> = page1
            .records
            .iter()
            .chain(&page2.records)
            .chain(&page3.records)
            .map(|record| record.message.clone())
            .collect();
        all_messages.sort();
        all_messages.dedup();
        assert_eq!(
            all_messages.len(),
            7,
            "pagination must visit every row once"
        );
    }

    #[tokio::test]
    async fn rejects_a_malformed_cursor() {
        let store: std::sync::Arc<dyn DiagnosticTimelineStore> =
            std::sync::Arc::new(FixtureStore::default());
        let router = diagnostics_router(Some(store), Duration::from_secs(5), 8);
        let (status, _) = get(router, "/v1/diagnostics?cursor=not-a-cursor").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn a_query_that_exceeds_its_deadline_is_reported_as_unavailable() {
        let gate = QueryGate::new();
        let deadline = Duration::from_millis(20);
        let store: Arc<dyn DiagnosticTimelineStore> = Arc::new(FixtureStore {
            rows: vec![fixture_event(1, 1)],
            gate: Some(Arc::clone(&gate)),
        });
        let worker_completion = Arc::new(WorkerCompletion::default());
        let router = diagnostics_router_with_worker_completion(
            Some(store),
            deadline,
            8,
            Arc::clone(&worker_completion),
        );

        let request = tokio::spawn(get(router, "/v1/diagnostics"));

        // Deterministic: wait until the fixture query has genuinely started
        // (on its own `spawn_blocking` thread) before advancing the paused
        // clock, instead of racing a real sleep against the deadline.
        gate.wait_for_entered(1).await;
        tokio::time::advance(deadline + Duration::from_millis(1)).await;

        let (status, _) = request
            .await
            .expect("request task completes once the bounded deadline elapses");
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

        // Let the parked fixture query thread return so it does not leak
        // past the end of the test.
        gate.release();
        worker_completion.wait_for_completed(1).await;
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn timed_out_queries_keep_worker_admission_until_the_workers_finish() {
        let gate = QueryGate::new();
        let deadline = Duration::from_millis(20);
        let store: Arc<dyn DiagnosticTimelineStore> = Arc::new(FixtureStore {
            rows: vec![fixture_event(1, 1)],
            gate: Some(Arc::clone(&gate)),
        });
        // This directly models the production max-connections budget: two
        // timed-out requests may leave two workers running, but never a
        // third once their HTTP futures have returned.
        let worker_completion = Arc::new(WorkerCompletion::default());
        let router = diagnostics_router_with_worker_completion(
            Some(store),
            deadline,
            2,
            Arc::clone(&worker_completion),
        );

        let first = tokio::spawn(get(router.clone(), "/v1/diagnostics"));
        gate.wait_for_entered(1).await;
        tokio::time::advance(deadline + Duration::from_millis(1)).await;
        assert_eq!(
            first.await.expect("first request completes").0,
            StatusCode::SERVICE_UNAVAILABLE
        );

        let second = tokio::spawn(get(router.clone(), "/v1/diagnostics"));
        gate.wait_for_entered(2).await;
        tokio::time::advance(deadline + Duration::from_millis(1)).await;
        assert_eq!(
            second.await.expect("second request completes").0,
            StatusCode::SERVICE_UNAVAILABLE
        );

        let (status, _) = get(router.clone(), "/v1/diagnostics").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            gate.entered_count(),
            2,
            "the third request must not spawn a worker after two timed-out workers retain admission"
        );

        gate.release();
        worker_completion.wait_for_completed(2).await;

        let (status, _) = get(router, "/v1/diagnostics").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "worker admission returns only after the blocking workers finish"
        );
    }

    #[tokio::test]
    async fn cursor_pagination_is_stable_across_rows_sharing_one_timestamp() {
        // Same millisecond timestamp for every row exercises the `id`
        // tie-break exclusively.
        let rows: Vec<_> = (0..5).map(|id| fixture_event(id, 1)).collect();
        let store: std::sync::Arc<dyn DiagnosticTimelineStore> =
            std::sync::Arc::new(FixtureStore { rows, gate: None });
        let router = diagnostics_router(Some(store), Duration::from_secs(5), 8);

        let (status, body) = get(router.clone(), "/v1/diagnostics?limit=2").await;
        assert_eq!(status, StatusCode::OK);
        let page1: DiagnosticTimelineResponse = serde_json::from_slice(&body).expect("page one");
        let cursor = page1
            .next_cursor
            .expect("truncated first page has a cursor");

        let (status, body) =
            get(router, &format!("/v1/diagnostics?limit=10&cursor={cursor}")).await;
        assert_eq!(status, StatusCode::OK);
        let page2: DiagnosticTimelineResponse = serde_json::from_slice(&body).expect("page two");
        assert_eq!(page2.records.len(), 3, "remaining same-timestamp rows");

        let mut seen: Vec<_> = page1
            .records
            .iter()
            .chain(&page2.records)
            .map(|record| record.message.clone())
            .collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 5);
    }

    #[test]
    fn decode_rejects_a_cursor_that_was_never_encoded() {
        assert!(DiagnosticCursor::decode("../../etc/passwd").is_err());
    }
}
