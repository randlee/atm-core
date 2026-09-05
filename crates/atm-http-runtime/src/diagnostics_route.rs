//! Bounded, read-only HTTP projection of the retained diagnostic timeline.

use std::sync::Arc;

use atm_core::observability_counters::{DiagnosticTimelineRecord, DiagnosticTimelineResponse};
use atm_runtime::DiagnosticTimelineStore;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

const DEFAULT_LIMIT: usize = 50;
pub const MAX_DIAGNOSTICS_LIMIT: usize = 5_000;

#[derive(Clone)]
struct DiagnosticsState {
    store: Option<Arc<dyn DiagnosticTimelineStore>>,
}

/// Adds the authenticated local diagnostics query route.
pub(crate) fn diagnostics_router(store: Option<Arc<dyn DiagnosticTimelineStore>>) -> Router {
    Router::new()
        .route("/v1/diagnostics", get(query_diagnostics))
        .with_state(DiagnosticsState { store })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiagnosticsQuery {
    since: Option<i64>,
    until: Option<i64>,
    level: Option<String>,
    component: Option<String>,
    limit: Option<usize>,
}

async fn query_diagnostics(
    State(state): State<DiagnosticsState>,
    Query(query): Query<DiagnosticsQuery>,
) -> Result<Json<DiagnosticTimelineResponse>, axum::response::Response> {
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    if limit > MAX_DIAGNOSTICS_LIMIT {
        return Err(diagnostics_error(
            StatusCode::BAD_REQUEST,
            format!("limit must be at most {MAX_DIAGNOSTICS_LIMIT}"),
        ));
    }
    let store = state.store.ok_or_else(|| {
        diagnostics_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "diagnostic timeline was not installed at daemon startup",
        )
    })?;
    let storage_query = atm_runtime::DiagnosticQuery {
        since: query.since,
        until: query.until,
        level_at_least: query.level,
        component_prefix: query.component,
        limit: Some(limit),
    };
    let events = tokio::task::spawn_blocking(move || store.query(&storage_query))
        .await
        .map_err(|_| {
            diagnostics_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "diagnostic timeline query worker stopped",
            )
        })?
        .map_err(|error| diagnostics_error(StatusCode::SERVICE_UNAVAILABLE, error.to_string()))?;
    Ok(Json(DiagnosticTimelineResponse {
        records: events.into_iter().map(diagnostic_record).collect(),
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
