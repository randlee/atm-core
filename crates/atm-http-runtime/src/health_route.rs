//! Read-only retained-observability health projection.

use std::sync::Arc;

use atm_core::observability_counters::{
    DiagnosticCounters, DiagnosticCountersSource, RetainedObservabilityHealthResponse,
};
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};

use crate::RuntimeHealth;

#[derive(Clone)]
struct HealthState {
    counters: Option<Arc<dyn DiagnosticCountersSource>>,
    _runtime_health: RuntimeHealth,
}

/// Adds the replacement runtime's read-only health endpoint.
pub(crate) fn health_router(
    runtime_health: RuntimeHealth,
    counters: Option<Arc<dyn DiagnosticCountersSource>>,
) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .with_state(HealthState {
            counters,
            _runtime_health: runtime_health,
        })
}

async fn health(State(state): State<HealthState>) -> Json<RetainedObservabilityHealthResponse> {
    let counters = state.counters.as_deref().map_or_else(
        DiagnosticCounters::default,
        DiagnosticCountersSource::snapshot,
    );
    Json(counters.into())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use atm_core::observability_counters::{DiagnosticCounters, DiagnosticCountersSource};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::health_router;
    use crate::RuntimeHealth;

    struct CounterFixture(DiagnosticCounters);

    impl DiagnosticCountersSource for CounterFixture {
        fn snapshot(&self) -> DiagnosticCounters {
            self.0
        }
    }

    #[tokio::test]
    async fn health_projects_counters_and_marks_dropped_sink_degraded() {
        let app = health_router(
            RuntimeHealth::default(),
            Some(Arc::new(CounterFixture(DiagnosticCounters {
                jsonl_forwarded_total: 9,
                timeline_written_total: 8,
                timeline_dropped_persist_error_total: 1,
                ..DiagnosticCounters::default()
            }))),
        );
        let response = app
            .oneshot(
                Request::get("/v1/health")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("health response");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let value: serde_json::Value = serde_json::from_slice(&bytes).expect("health JSON");
        assert_eq!(value["observability"]["jsonl"]["forwarded_total"], 9);
        assert_eq!(value["observability"]["timeline"]["written_total"], 8);
        assert_eq!(
            value["observability"]["degraded"],
            serde_json::json!(["timeline"])
        );
    }
}
