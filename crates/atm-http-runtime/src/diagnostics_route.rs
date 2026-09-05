//! Bounded, read-only HTTP projection of the retained diagnostic timeline.

use std::sync::Arc;
use std::time::Duration;

use atm_core::observability_counters::{
    DIAGNOSTIC_QUERY_MAX_LIMIT, DiagnosticTimelineRecord, DiagnosticTimelineResponse,
};
use atm_runtime::{DiagnosticCursor, DiagnosticTimelineStore};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

const DEFAULT_LIMIT: usize = 50;

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
}

/// Adds the authenticated local diagnostics query route.
pub(crate) fn diagnostics_router(
    store: Option<Arc<dyn DiagnosticTimelineStore>>,
    query_deadline: Duration,
) -> Router {
    Router::new()
        .route("/v1/diagnostics", get(query_diagnostics))
        .with_state(DiagnosticsState {
            store,
            query_deadline,
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
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
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
    let query_future = tokio::task::spawn_blocking(move || store.query(&storage_query));
    let mut events = tokio::time::timeout(state.query_deadline, query_future)
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
        .map_err(|error| diagnostics_error(StatusCode::SERVICE_UNAVAILABLE, error.to_string()))?;
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
    use std::time::Duration;

    use atm_core::error::AtmError;
    use atm_core::observability_counters::{
        DIAGNOSTIC_QUERY_MAX_LIMIT, DiagnosticTimelineResponse,
    };
    use atm_runtime::{
        DiagnosticCursor, DiagnosticEvent, DiagnosticQuery, DiagnosticTimelineStore,
    };
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::diagnostics_router;

    #[derive(Debug, Default)]
    struct FixtureStore {
        rows: Vec<DiagnosticEvent>,
        query_delay: Option<Duration>,
    }

    impl DiagnosticTimelineStore for FixtureStore {
        fn record_batch(&self, _events: &[DiagnosticEvent]) -> Result<(), AtmError> {
            unimplemented!("the diagnostics route never writes")
        }

        fn query(&self, query: &DiagnosticQuery) -> Result<Vec<DiagnosticEvent>, AtmError> {
            if let Some(delay) = self.query_delay {
                std::thread::sleep(delay);
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
        let router = diagnostics_router(Some(store), Duration::from_secs(5));
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
        let router = diagnostics_router(Some(store), Duration::from_secs(5));
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
            std::sync::Arc::new(FixtureStore {
                rows,
                query_delay: None,
            });
        let router = diagnostics_router(Some(store), Duration::from_secs(5));

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
        let router = diagnostics_router(Some(store), Duration::from_secs(5));
        let (status, _) = get(router, "/v1/diagnostics?cursor=not-a-cursor").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn a_query_that_exceeds_its_deadline_is_reported_as_unavailable() {
        let store: std::sync::Arc<dyn DiagnosticTimelineStore> =
            std::sync::Arc::new(FixtureStore {
                rows: vec![fixture_event(1, 1)],
                query_delay: Some(Duration::from_millis(200)),
            });
        let router = diagnostics_router(Some(store), Duration::from_millis(20));
        let (status, _) = get(router, "/v1/diagnostics").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn cursor_pagination_is_stable_across_rows_sharing_one_timestamp() {
        // Same millisecond timestamp for every row exercises the `id`
        // tie-break exclusively.
        let rows: Vec<_> = (0..5).map(|id| fixture_event(id, 1)).collect();
        let store: std::sync::Arc<dyn DiagnosticTimelineStore> =
            std::sync::Arc::new(FixtureStore {
                rows,
                query_delay: None,
            });
        let router = diagnostics_router(Some(store), Duration::from_secs(5));

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
