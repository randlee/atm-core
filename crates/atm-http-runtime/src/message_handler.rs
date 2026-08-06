//! Canonical typed HTTP ingress for message writes.
//!
//! This module owns HTTP extraction, connector-provenance normalization, and
//! response translation only.  It deliberately delegates persistence and all
//! message policy to the sealed [`ApiRouter`] boundary exactly once.

use std::sync::Arc;

use atm_core::ApiRouter;
use atm_core::api::{ApiRequest, ApiResponse, AuthenticatedIngress, RequestDeadline};
use atm_core::error::AtmError;
use atm_core::protocol::{ResponseEnvelope, SendResponseEnvelope};
use atm_core::send::WriteRequest;
use atm_core::types::HostName;
use axum::body::Body;
use axum::error_handling::HandleErrorLayer;
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::header::{CONTENT_TYPE, HeaderName, LOCATION};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::post;
use axum::{Json, Router};
use serde::Serialize;
use tower::BoxError;
use tower::ServiceBuilder;
use tower::limit::ConcurrencyLimitLayer;
use tower::load_shed::LoadShedLayer;

use crate::{RuntimeLimits, RuntimeTimeouts};

const MESSAGES_PATH: &str = "/v1/atm/messages";
const LEGACY_PEER_SOURCE_HEADER: HeaderName = HeaderName::from_static("x-atm-peer-source-host");

/// Provenance established by the transport adapter after authentication.
///
/// The handler never derives this fact from a socket address or from JSON.  A
/// local adapter strips a client-supplied host claim; an authenticated peer
/// adapter replaces that claim with its TLS-authenticated source host before
/// the one application dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthenticatedConnector {
    /// A local UDS or loopback-capability connection.
    Local,
    /// A peer connection authenticated by the TLS adapter.
    Peer { source_host: HostName },
}

impl AuthenticatedConnector {
    /// Returns local connector provenance.
    #[must_use]
    pub const fn local() -> Self {
        Self::Local
    }

    /// Returns authenticated peer provenance from the adapter-owned TLS identity.
    #[must_use]
    pub fn peer(source_host: HostName) -> Self {
        Self::Peer { source_host }
    }

    fn normalize_write(&self, request: &mut WriteRequest) -> AuthenticatedIngress {
        match self {
            Self::Local => {
                request.authenticated_source_host = None;
                AuthenticatedIngress::Local
            }
            Self::Peer { source_host } => {
                request.authenticated_source_host = Some(source_host.clone());
                AuthenticatedIngress::Peer
            }
        }
    }
}

#[derive(Clone)]
struct MessageRouteState {
    router: Arc<dyn ApiRouter>,
    connector: AuthenticatedConnector,
    request_timeout: std::time::Duration,
}

/// Builds the one framework-owned typed message-write route.
///
/// `limits` and `timeouts` are the validated AL.1 runtime values. The Tower
/// layers bound body memory and in-flight work; `LoadShedLayer` rejects rather
/// than queues a request whenever the configured capacity is unavailable.
pub fn canonical_message_router(
    router: Arc<dyn ApiRouter>,
    connector: AuthenticatedConnector,
    limits: RuntimeLimits,
    timeouts: RuntimeTimeouts,
) -> Router {
    let state = MessageRouteState {
        router,
        connector,
        request_timeout: timeouts.request,
    };
    let admission = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(|error: BoxError| async move {
            overload_response(error).await
        }))
        .layer(LoadShedLayer::new())
        .layer(ConcurrencyLimitLayer::new(limits.max_connections));

    Router::new()
        .route(MESSAGES_PATH, post(post_messages).layer(admission))
        .layer(DefaultBodyLimit::max(limits.max_body_bytes))
        .with_state(state)
}

async fn post_messages(
    State(state): State<MessageRouteState>,
    headers: HeaderMap,
    request: Result<Json<WriteRequest>, JsonRejection>,
) -> Response {
    if let Err(error) = validate_request_headers(&headers) {
        return error_response(error);
    }
    let Json(mut request) = match request {
        Ok(request) => request,
        Err(rejection) => return error_response(framework_rejection(rejection)),
    };
    let ingress = state.connector.normalize_write(&mut request);
    let deadline = RequestDeadline::after(state.request_timeout);

    let response = state
        .router
        .route(ApiRequest::Write(Box::new(request)), ingress, deadline);
    response
        .and_then(map_write_response)
        .unwrap_or_else(error_response)
}

fn validate_request_headers(headers: &HeaderMap) -> Result<(), AtmError> {
    if headers.contains_key(&LEGACY_PEER_SOURCE_HEADER) {
        return Err(AtmError::validation(
            "X-ATM-Peer-Source-Host is not accepted by canonical HTTP ingress",
        ));
    }
    Ok(())
}

async fn overload_response(_: BoxError) -> Response {
    error_response(AtmError::daemon_unavailable(
        "HTTP message ingress is at its configured in-flight capacity",
    ))
}

fn framework_rejection(rejection: JsonRejection) -> AtmError {
    AtmError::validation("invalid HTTP messages request").with_cause(rejection)
}

fn map_write_response(response: ApiResponse) -> Result<Response, AtmError> {
    match response.into_inner() {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => json_response(
            StatusCode::CREATED,
            &outcome,
            Some(outcome.message_id.to_string()),
        ),
        ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome)) => json_response(
            StatusCode::CREATED,
            &outcome,
            Some(outcome.message_id.to_string()),
        ),
        ResponseEnvelope::Error(error) => Ok(error_response(error)),
        _ => Err(AtmError::daemon_unavailable(
            "canonical message route received a non-write application response",
        )),
    }
}

fn error_response(error: AtmError) -> Response {
    let status = if error.is_validation() {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    // `AtmError` is the repository-wide serde error contract. If serialization
    // itself fails, return a minimal 503 without inventing another JSON shape.
    json_response(status, &error, None).unwrap_or_else(|_| Response::new(Body::empty()))
}

fn json_response<T: Serialize>(
    status: StatusCode,
    value: &T,
    message_id: Option<String>,
) -> Result<Response, AtmError> {
    let body = serde_json::to_vec(value).map_err(|source| {
        AtmError::daemon_unavailable("failed to serialize canonical HTTP response")
            .with_cause(source)
    })?;
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if let Some(message_id) = message_id {
        let location = format!("/v1/atm/message/{message_id}");
        let location = HeaderValue::from_str(&location).map_err(|source| {
            AtmError::daemon_unavailable("failed to serialize message location header")
                .with_cause(source)
        })?;
        response.headers_mut().insert(LOCATION, location);
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::path::PathBuf;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::Duration;

    use atm_core::boundary;
    use atm_core::error::AtmError;
    use atm_core::protocol::ResponseEnvelope;
    use atm_core::send::{SendCommandOutcome, SendMessageSource, SendOutcome};
    use atm_core::types::CommandAction;
    use atm_core::{ApiRequest, ApiResponse, ApiRouter, AuthenticatedIngress, RequestDeadline};
    use axum::body::{Body, to_bytes};
    use axum::http::header::{CONTENT_TYPE, HeaderName};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use super::{AuthenticatedConnector, canonical_message_router};
    use crate::{NonZeroDuration, RuntimeLimits, RuntimeTimeouts};

    #[derive(Clone)]
    struct RecordingRouter {
        response: ResponseEnvelope,
        calls: Arc<Mutex<Vec<(atm_core::send::WriteRequest, AuthenticatedIngress)>>>,
    }

    impl boundary::sealed::Sealed for RecordingRouter {}

    impl ApiRouter for RecordingRouter {
        fn route(
            &self,
            request: ApiRequest,
            ingress: AuthenticatedIngress,
            _deadline: RequestDeadline,
        ) -> Result<ApiResponse, AtmError> {
            let ApiRequest::Write(request) = request else {
                return Err(AtmError::validation("test expected a write request"));
            };
            self.calls
                .lock()
                .expect("record calls")
                .push((*request, ingress));
            Ok(ApiResponse::new(self.response.clone()))
        }
    }

    struct BlockingRouter {
        gate: Arc<Gate>,
        response: ResponseEnvelope,
    }

    impl boundary::sealed::Sealed for BlockingRouter {}

    impl ApiRouter for BlockingRouter {
        fn route(
            &self,
            _request: ApiRequest,
            _ingress: AuthenticatedIngress,
            _deadline: RequestDeadline,
        ) -> Result<ApiResponse, AtmError> {
            self.gate.wait_until_released();
            Ok(ApiResponse::new(self.response.clone()))
        }
    }

    #[derive(Default)]
    struct Gate {
        state: Mutex<GateState>,
        changed: Condvar,
    }

    #[derive(Default)]
    struct GateState {
        entered: bool,
        released: bool,
    }

    impl Gate {
        fn wait_until_released(&self) {
            let mut state = self.state.lock().expect("lock gate");
            state.entered = true;
            self.changed.notify_all();
            while !state.released {
                state = self.changed.wait(state).expect("wait gate");
            }
        }

        fn wait_until_entered(&self) {
            let mut state = self.state.lock().expect("lock gate");
            while !state.entered {
                state = self.changed.wait(state).expect("wait gate");
            }
        }

        fn release(&self) {
            self.state.lock().expect("lock gate").released = true;
            self.changed.notify_all();
        }
    }

    fn limits(body_bytes: usize, in_flight: usize) -> RuntimeLimits {
        RuntimeLimits::new(
            NonZeroUsize::new(body_bytes).expect("non-zero body limit"),
            NonZeroUsize::new(in_flight).expect("non-zero in-flight limit"),
        )
    }

    fn timeouts() -> RuntimeTimeouts {
        RuntimeTimeouts::new(
            NonZeroDuration::new(Duration::from_secs(1)).expect("non-zero request timeout"),
            NonZeroDuration::new(Duration::from_secs(1)).expect("non-zero shutdown timeout"),
        )
    }

    fn write_request() -> atm_core::send::WriteRequest {
        atm_core::send::WriteRequest::new(
            PathBuf::from("/tmp/atm-http-runtime-test-home"),
            PathBuf::from("/tmp/atm-http-runtime-test-workspace"),
            "sender".parse().expect("agent"),
            "recipient@test-team",
            "test-team".parse().expect("team"),
            SendMessageSource::Inline("typed route fixture".to_owned()),
            None,
            false,
            None,
            false,
        )
        .expect("write request")
    }

    fn sent_response() -> ResponseEnvelope {
        ResponseEnvelope::Send(atm_core::protocol::SendResponseEnvelope::Sent(
            SendOutcome {
                action: CommandAction::Send,
                team: "test-team".parse().expect("team"),
                agent: "recipient".parse().expect("agent"),
                sender: "sender".parse().expect("agent"),
                outcome: SendCommandOutcome::Sent,
                message_id: atm_core::schema::AtmMessageId::new(),
                requires_ack: false,
                task_id: None,
                summary: None,
                message: Some("typed route fixture".to_owned()),
                warnings: Vec::new(),
                dry_run: false,
            },
        ))
    }

    async fn post(app: axum::Router, body: Vec<u8>) -> axum::response::Response {
        post_with_headers(app, body, &[(CONTENT_TYPE, "application/json")]).await
    }

    async fn post_with_headers(
        app: axum::Router,
        body: Vec<u8>,
        headers: &[(HeaderName, &str)],
    ) -> axum::response::Response {
        let mut request = Request::builder().method("POST").uri("/v1/atm/messages");
        for (name, value) in headers {
            request = request.header(name, *value);
        }
        app.oneshot(request.body(Body::from(body)).expect("HTTP request"))
            .await
            .expect("infallible Axum service")
    }

    async fn response_body(response: axum::response::Response) -> Vec<u8> {
        to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response bytes")
            .to_vec()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn local_and_peer_use_identical_write_json_and_one_dispatch() {
        let mut request = write_request();
        request.authenticated_source_host = Some("forged.example.test".parse().expect("host"));
        let body = serde_json::to_vec(&request).expect("typed write JSON");
        let response = sent_response();
        let expected = match &response {
            ResponseEnvelope::Send(atm_core::protocol::SendResponseEnvelope::Sent(outcome)) => {
                serde_json::to_vec(outcome).expect("typed outcome JSON")
            }
            _ => unreachable!("test response is a send outcome"),
        };

        let local_calls = Arc::new(Mutex::new(Vec::new()));
        let local = canonical_message_router(
            Arc::new(RecordingRouter {
                response: response.clone(),
                calls: Arc::clone(&local_calls),
            }),
            AuthenticatedConnector::local(),
            limits(4096, 2),
            timeouts(),
        );
        let local_response = post(local, body.clone()).await;
        assert_eq!(local_response.status(), StatusCode::CREATED);
        assert_eq!(response_body(local_response).await, expected);

        let peer_calls = Arc::new(Mutex::new(Vec::new()));
        let peer = canonical_message_router(
            Arc::new(RecordingRouter {
                response,
                calls: Arc::clone(&peer_calls),
            }),
            AuthenticatedConnector::peer("trusted.example.test".parse().expect("host")),
            limits(4096, 2),
            timeouts(),
        );
        let peer_response = post(peer, body.clone()).await;
        assert_eq!(peer_response.status(), StatusCode::CREATED);
        assert_eq!(response_body(peer_response).await, expected);

        assert_eq!(body, serde_json::to_vec(&request).expect("same typed JSON"));
        let local_calls = local_calls.lock().expect("local calls");
        let peer_calls = peer_calls.lock().expect("peer calls");
        assert_eq!(local_calls.len(), 1, "local request has one core dispatch");
        assert_eq!(peer_calls.len(), 1, "peer request has one core dispatch");
        assert_eq!(local_calls[0].0.authenticated_source_host, None);
        assert_eq!(local_calls[0].1, AuthenticatedIngress::Local);
        assert_eq!(
            peer_calls[0].0.authenticated_source_host,
            Some("trusted.example.test".parse().expect("host"))
        );
        assert_eq!(peer_calls[0].1, AuthenticatedIngress::Peer);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn framework_json_and_body_rejections_keep_the_adr_032_schema() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let app = canonical_message_router(
            Arc::new(RecordingRouter {
                response: sent_response(),
                calls: Arc::clone(&calls),
            }),
            AuthenticatedConnector::local(),
            limits(32, 1),
            timeouts(),
        );

        for body in [b"not-json".to_vec(), vec![b'x'; 33]] {
            let response = post(app.clone(), body).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let error: AtmError =
                serde_json::from_slice(&response_body(response).await).expect("ADR-032 error JSON");
            assert!(error.is_validation(), "{error:?}");
        }
        assert!(calls.lock().expect("calls").is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn malformed_headers_are_rejected_as_direct_adr_032_errors() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let app = canonical_message_router(
            Arc::new(RecordingRouter {
                response: sent_response(),
                calls: Arc::clone(&calls),
            }),
            AuthenticatedConnector::local(),
            limits(4096, 1),
            timeouts(),
        );
        let request = serde_json::to_vec(&write_request()).expect("typed JSON");

        for headers in [
            vec![(CONTENT_TYPE, "text/plain")],
            vec![
                (CONTENT_TYPE, "application/json"),
                (
                    HeaderName::from_static("x-atm-peer-source-host"),
                    "not a valid host",
                ),
            ],
        ] {
            let response = post_with_headers(app.clone(), request.clone(), &headers).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let error: AtmError = serde_json::from_slice(&response_body(response).await)
                .expect("ADR-032 header error JSON");
            assert!(error.is_validation(), "{error:?}");
        }
        assert!(calls.lock().expect("calls").is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn typed_core_errors_use_the_one_adr_032_response_mapper() {
        let expected = AtmError::daemon_unavailable("test core failure");
        let app = canonical_message_router(
            Arc::new(RecordingRouter {
                response: ResponseEnvelope::Error(expected.clone()),
                calls: Arc::new(Mutex::new(Vec::new())),
            }),
            AuthenticatedConnector::local(),
            limits(4096, 1),
            timeouts(),
        );

        let response = post(
            app,
            serde_json::to_vec(&write_request()).expect("typed JSON"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response_body(response).await,
            serde_json::to_vec(&expected).expect("typed ADR-032 JSON")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn load_shed_rejects_without_an_application_queue() {
        let gate = Arc::new(Gate::default());
        let app = canonical_message_router(
            Arc::new(BlockingRouter {
                gate: Arc::clone(&gate),
                response: sent_response(),
            }),
            AuthenticatedConnector::local(),
            limits(4096, 1),
            timeouts(),
        );
        let first_app = app.clone();
        let first_body = serde_json::to_vec(&write_request()).expect("typed write JSON");
        let first = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("test runtime")
                .block_on(post(first_app, first_body))
        });
        gate.wait_until_entered();

        let started = std::time::Instant::now();
        let overloaded = post(
            app,
            serde_json::to_vec(&write_request()).expect("typed JSON"),
        )
        .await;
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "load shedding must reject within the configured request budget"
        );
        assert_eq!(overloaded.status(), StatusCode::SERVICE_UNAVAILABLE);
        let error: AtmError = serde_json::from_slice(&response_body(overloaded).await)
            .expect("ADR-032 overload JSON");
        assert!(error.is_daemon_unavailable(), "{error:?}");

        gate.release();
        assert_eq!(
            first.join().expect("first request thread").status(),
            StatusCode::CREATED
        );
    }

    #[test]
    fn openapi_and_handler_keep_the_existing_typed_write_contract() {
        let openapi = include_str!("../../../docs/atm-daemon/openapi.yaml");
        assert!(openapi.contains("operationId: writeMessage"));
        assert!(openapi.contains("$ref: '#/components/schemas/WriteRequest'"));
        let forbidden = ["Http", "Frame", "Reader"].concat();
        assert!(!include_str!("message_handler.rs").contains(&forbidden));
        let forbidden = ["Peer", "Message", "Array"].concat();
        assert!(!include_str!("message_handler.rs").contains(&forbidden));
    }
}
