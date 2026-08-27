//! Canonical typed HTTP ingress for message writes.
//!
//! This module owns HTTP extraction, connector-provenance normalization, and
//! response translation only. It delegates persistence and received-message
//! notification to one replacement-owned async write boundary.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use atm_core::api::{
    ApiRequest, ApiResponse, AuthenticatedIngress, CLEAR_OUTCOME_HEADER, HttpRequest,
    HttpRouteKind, MessageCollectionRequest, RequestDeadline, UntrustedSmokeProvenance,
    http_route_kind, http_route_surface,
};
use atm_core::clear::ClearQuery;
use atm_core::doctor::DoctorQuery;
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::protocol::{
    CompatibilityPreflight, GraftReceiverLookupRequest, GraftReceiverRegistration,
    GraftReceiverRefreshRequest, GraftReceiverUnregistration, QueueGetNextRequest, RequestId,
    ResponseEnvelope,
    SendResponseEnvelope, TeamMemberHeartbeatRequest, next_request_id,
};
use atm_core::read::{PeekQuery, ReadQuery};
use atm_core::search::SearchRequest;
use atm_core::send::WriteRequest;
use atm_core::types::HostName;
use axum::body::{Body, Bytes};
use axum::error_handling::HandleErrorLayer;
use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Extension, State};
use axum::http::header::{CONTENT_TYPE, LOCATION};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::Response;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use base64::Engine as _;
use serde::Serialize;
use std::net::SocketAddr;
use tower::BoxError;
use tower::ServiceBuilder;
use tower::limit::ConcurrencyLimitLayer;
use tower::load_shed::LoadShedLayer;
use tracing::{Instrument, info, info_span, warn};

use crate::{RuntimeLimits, RuntimeTimeouts};

/// Retired peer-provenance claims must never select application routing.
///
/// This remains private to the HTTP boundary solely to reject stale callers;
/// connectors establish provenance independently after authentication.
const RETIRED_PEER_PROVENANCE_HEADER: &str = "X-ATM-Peer-Source-Host";
const REQUEST_ID_HEADER: &str = "X-ATM-Request-Id";

/// Async replacement-owned application operation for the canonical write
/// route. Implementations may isolate synchronous storage or hook adapters in
/// narrow `spawn_blocking` calls, but the HTTP handler itself remains a Tokio
/// future and never dispatches an entire legacy router on a worker pool.
/// Fixed replacement-runtime application boundary.
///
/// This trait is sealed by the core workspace boundary convention: transport
/// adapters may call it, but only ATM-owned composition may provide an
/// implementation. That preserves one canonical write operation instead of a
/// public plugin surface.
pub trait CanonicalWriteHandler: atm_core::boundary::sealed::Sealed + Send + Sync {
    fn write(
        &self,
        request: WriteRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<ApiResponse, AtmError>> + Send + '_>>;

    /// Writes with the correlation ID established at HTTP ingress.
    ///
    /// Focused write-only handlers retain the compatibility implementation;
    /// the production router overrides this to preserve the ID across a
    /// directly forwarded peer acknowledgement.
    fn write_with_request_id(
        &self,
        request: WriteRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
        _request_id: RequestId,
    ) -> Pin<Box<dyn Future<Output = Result<ApiResponse, AtmError>> + Send + '_>> {
        self.write(request, ingress, deadline)
    }

    /// Dispatches a route decoded by the core-owned HTTP codec.
    ///
    /// Existing focused write implementations retain the default, which keeps
    /// the AL.2 write-only router useful in unit tests. The production
    /// replacement composition overrides this method so every retained route
    /// reaches one framework router without falling back to the frozen daemon.
    fn dispatch(
        &self,
        request: ApiRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Pin<Box<dyn Future<Output = Result<ApiResponse, AtmError>> + Send + '_>> {
        match request {
            ApiRequest::Write(request) => self.write(*request, ingress, deadline),
            request => Box::pin(async move {
                Err(AtmError::validation(format!(
                    "replacement HTTP route is not implemented for {:?}",
                    request.into_inner()
                )))
            }),
        }
    }

    /// Dispatches with the correlation ID established at HTTP ingress.
    fn dispatch_with_request_id(
        &self,
        request: ApiRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
        request_id: RequestId,
    ) -> Pin<Box<dyn Future<Output = Result<ApiResponse, AtmError>> + Send + '_>> {
        match request {
            ApiRequest::Write(request) => {
                self.write_with_request_id(*request, ingress, deadline, request_id)
            }
            request => self.dispatch(request, ingress, deadline),
        }
    }
}

/// Provenance established by the transport adapter after authentication.
///
/// The handler never derives this fact from a socket address or from JSON.  A
/// local adapter strips a client-supplied provenance claim; a configured peer
/// adapter replaces that claim with its adapter-owned source host before the
/// one application dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthenticatedConnector {
    /// A local UDS or loopback-capability connection.
    Local,
    /// A peer connection normalized by its configured transport adapter.
    Peer { source_host: HostName },
    /// A direct plain-TCP peer connection. The adapter takes provenance from
    /// the accepted socket rather than any process configuration or payload.
    PeerSocket,
}

impl AuthenticatedConnector {
    /// Returns local connector provenance.
    #[must_use]
    pub const fn local() -> Self {
        Self::Local
    }

    /// Returns peer provenance from the adapter-owned configured identity.
    #[must_use]
    pub fn peer(source_host: HostName) -> Self {
        Self::Peer { source_host }
    }

    /// Returns direct peer provenance from the accepted connection.
    #[must_use]
    pub const fn peer_socket() -> Self {
        Self::PeerSocket
    }

    fn normalize_write(
        &self,
        request: &mut WriteRequest,
        peer_address: Option<SocketAddr>,
    ) -> Result<AuthenticatedIngress, AtmError> {
        match self {
            Self::Local => {
                request.authenticated_source_host = None;
                Ok(AuthenticatedIngress::Local)
            }
            Self::Peer { source_host } => {
                request.authenticated_source_host = Some(source_host.clone());
                if let Some(destination) = request.to.take() {
                    request.to = Some(destination.without_host());
                }
                Ok(AuthenticatedIngress::Peer)
            }
            Self::PeerSocket => {
                let peer_address = peer_address.ok_or_else(|| {
                    AtmError::daemon_unavailable(
                        "direct peer HTTP request did not carry an accepted socket address",
                    )
                })?;
                let source_host: HostName =
                    peer_address.ip().to_string().parse().map_err(|source| {
                        AtmError::validation(
                            "accepted direct peer address cannot be represented as a host identity",
                        )
                        .with_cause(source)
                    })?;
                // A TCP source address is adapter-derived diagnostic
                // provenance, never peer authentication. The ingress below
                // remains `UntrustedSmoke`; retaining the address lets an
                // explicit plaintext test reply to the actual socket peer
                // without accepting a JSON-forged value.
                request.authenticated_source_host = Some(source_host.clone());
                if let Some(destination) = request.to.take() {
                    request.to = Some(destination.without_host());
                }
                Ok(AuthenticatedIngress::UntrustedSmoke(
                    UntrustedSmokeProvenance::new(source_host),
                ))
            }
        }
    }

    fn normalize_request(
        &self,
        request: &mut ApiRequest,
        peer_address: Option<SocketAddr>,
    ) -> Result<AuthenticatedIngress, AtmError> {
        match request {
            ApiRequest::Write(write) => self.normalize_write(write, peer_address),
            _ => match self {
                Self::Local => Ok(AuthenticatedIngress::Local),
                Self::Peer { .. } => Ok(AuthenticatedIngress::Peer),
                Self::PeerSocket => Ok(AuthenticatedIngress::AnonymousSmoke),
            },
        }
    }
}

#[derive(Clone)]
struct MessageRouteState {
    handler: Arc<dyn CanonicalWriteHandler>,
    connector: AuthenticatedConnector,
    request_timeout: std::time::Duration,
    max_body_bytes: usize,
}

/// Builds the one framework-owned typed message-write route.
///
/// `limits` and `timeouts` are the validated AL.1 runtime values. The Tower
/// layers bound body memory and in-flight work; `LoadShedLayer` rejects rather
/// than queues a request whenever the configured capacity is unavailable.
pub fn canonical_message_router(
    handler: Arc<dyn CanonicalWriteHandler>,
    connector: AuthenticatedConnector,
    limits: RuntimeLimits,
    timeouts: RuntimeTimeouts,
) -> Router {
    let state = MessageRouteState {
        handler,
        connector,
        request_timeout: timeouts.request,
        max_body_bytes: limits.max_body_bytes,
    };
    let admission = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(|error: BoxError| async move {
            overload_response(error).await
        }))
        .layer(LoadShedLayer::new())
        .layer(ConcurrencyLimitLayer::new(limits.max_connections));

    Router::new()
        .route(canonical_write_path(), post(post_messages).layer(admission))
        .layer(DefaultBodyLimit::max(limits.max_body_bytes))
        .with_state(state)
}

/// Builds the production framework router for the complete retained core HTTP
/// route surface. Each route is registered from [`http_route_surface`] and is
/// decoded at this framework boundary, so the loopback and UDS
/// connectors share the exact route/body contract with their clients.
///
/// This is deliberately distinct from [`canonical_message_router`]: focused
/// AL.2 tests can exercise the typed write handler alone, while the active
/// daemon must expose every retained contract route through this one Axum
/// router. It never invokes the frozen daemon dispatcher.
pub fn canonical_api_router(
    handler: Arc<dyn CanonicalWriteHandler>,
    connector: AuthenticatedConnector,
    limits: RuntimeLimits,
    timeouts: RuntimeTimeouts,
) -> Router {
    let state = MessageRouteState {
        handler,
        connector,
        request_timeout: timeouts.request,
        max_body_bytes: limits.max_body_bytes,
    };
    let admission = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(|error: BoxError| async move {
            overload_response(error).await
        }))
        .layer(LoadShedLayer::new())
        .layer(ConcurrencyLimitLayer::new(limits.max_connections));

    http_route_surface()
        .fold(
            Router::new().layer(DefaultBodyLimit::max(limits.max_body_bytes)),
            |router, route| match route.method {
                "GET" => router.route(route.path_template, get(dispatch_request)),
                "POST" => router.route(route.path_template, post(dispatch_request)),
                "DELETE" => router.route(route.path_template, delete(dispatch_request)),
                method => panic!("core HTTP route surface has unsupported method {method}"),
            },
        )
        .route_layer(admission)
        .with_state(state)
}

/// Returns the write path from the core-owned HTTP route surface.
///
/// The framework adapter intentionally has no path literal of its own: the
/// existing request codec and the runtime binding must select one declaration.
fn canonical_write_path() -> &'static str {
    let mut matches = http_route_surface()
        .filter(|route| route.method == "POST" && route.path_template.ends_with("/messages"));
    let path = matches
        .next()
        .expect("core HTTP route surface must declare one POST messages route")
        .path_template;
    assert!(
        matches.next().is_none(),
        "core HTTP route surface must declare only one POST messages route"
    );
    path
}

async fn post_messages(
    State(state): State<MessageRouteState>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    request: Result<Json<WriteRequest>, JsonRejection>,
) -> Response {
    let request_id = request_id_from_headers(&headers).unwrap_or_else(next_request_id);
    post_messages_with_request_id(state, peer, headers, request, request_id)
        .instrument(info_span!("atm_http_request", %request_id, method = "POST", path = canonical_write_path()))
        .await
}

async fn post_messages_with_request_id(
    state: MessageRouteState,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    request: Result<Json<WriteRequest>, JsonRejection>,
    request_id: RequestId,
) -> Response {
    info!(%request_id, "accepted canonical HTTP ingress request");
    if let Err(error) = validate_request_headers(&headers) {
        return error_response(error);
    }
    let Json(mut request) = match request {
        Ok(request) => request,
        Err(rejection) => return error_response(framework_rejection(rejection)),
    };
    let ingress = match state
        .connector
        .normalize_write(&mut request, peer.map(|Extension(peer)| peer.0))
    {
        Ok(ingress) => ingress,
        Err(error) => return error_response(error),
    };
    let deadline = RequestDeadline::after(state.request_timeout);

    let response = state
        .handler
        .write_with_request_id(request, ingress, deadline, request_id)
        .await;
    response
        .and_then(map_write_response)
        .unwrap_or_else(error_response)
}

async fn dispatch_request(
    State(state): State<MessageRouteState>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let request_id = request_id_from_headers(&headers).unwrap_or_else(next_request_id);
    let request_method = method.clone();
    let request_path = uri.path_and_query().map_or_else(
        || uri.path().to_owned(),
        |path_and_query| path_and_query.as_str().to_owned(),
    );
    dispatch_request_with_request_id(state, peer, method, uri, headers, body, request_id)
        .instrument(info_span!("atm_http_request", %request_id, method = %request_method, path = %request_path))
        .await
}

async fn dispatch_request_with_request_id(
    state: MessageRouteState,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
    request_id: RequestId,
) -> Response {
    info!(%request_id, "accepted canonical HTTP ingress request");
    if let Err(error) = validate_request_headers(&headers) {
        return error_response(error);
    }
    let headers = match canonical_headers(&headers) {
        Ok(headers) => headers,
        Err(error) => return error_response(error),
    };
    let mut request = match decode_framework_request(
        HttpRequest {
            method: method.as_str().to_owned(),
            path: uri.path_and_query().map_or_else(
                || uri.path().to_owned(),
                |path_and_query| path_and_query.as_str().to_owned(),
            ),
            headers,
            body: body.to_vec(),
        },
        state.max_body_bytes,
    ) {
        Ok(request) => request,
        Err(error) => return error_response(error),
    };
    let ingress = match state
        .connector
        .normalize_request(&mut request, peer.map(|Extension(peer)| peer.0))
    {
        Ok(ingress) => ingress,
        Err(error) => return error_response(error),
    };
    let deadline = RequestDeadline::after(state.request_timeout);
    state
        .handler
        .dispatch_with_request_id(request, ingress, deadline, request_id)
        .await
        .and_then(map_api_response)
        .unwrap_or_else(error_response)
}

fn invalid_framework_body(kind: &str, source: serde_json::Error) -> AtmError {
    AtmError::validation_with_recovery(
        format!("invalid {kind} HTTP request body: {source}"),
        "ensure the client sends the documented JSON request body and retry",
    )
}

fn decode_framework_request(
    request: HttpRequest,
    max_body_bytes: usize,
) -> Result<ApiRequest, AtmError> {
    if request.body.len() > max_body_bytes {
        return Err(AtmError::validation_with_recovery(
            format!("daemon HTTP request body exceeds configured limit of {max_body_bytes} bytes"),
            "reduce the request body or raise the runtime max_body_bytes limit and retry",
        ));
    }
    let body = request.body.as_slice();
    let method = request.method.to_ascii_uppercase();
    match http_route_kind(&method, &request.path) {
        Some(HttpRouteKind::Write) => serde_json::from_slice::<WriteRequest>(body)
            .map(|value| ApiRequest::Write(Box::new(value)))
            .map_err(|source| invalid_framework_body("write", source)),
        Some(HttpRouteKind::List) => serde_json::from_slice(body)
            .map(|value| ApiRequest::Messages(Box::new(MessageCollectionRequest::List(value))))
            .map_err(|source| invalid_framework_body("messages list", source)),
        Some(HttpRouteKind::Inspect) => serde_json::from_slice::<PeekQuery>(body)
            .map(|value| ApiRequest::Messages(Box::new(MessageCollectionRequest::Peek(value))))
            .map_err(|source| invalid_framework_body("message inspect", source)),
        Some(HttpRouteKind::Receive) => serde_json::from_slice::<ReadQuery>(body)
            .map(|value| ApiRequest::Messages(Box::new(MessageCollectionRequest::Receive(value))))
            .map_err(|source| invalid_framework_body("message read", source)),
        Some(HttpRouteKind::Clear) => serde_json::from_slice::<ClearQuery>(body)
            .map(ApiRequest::Clear)
            .map_err(|source| invalid_framework_body("messages clear", source)),
        Some(HttpRouteKind::Doctor) => serde_json::from_slice::<DoctorQuery>(body)
            .map(ApiRequest::Doctor)
            .map_err(|source| invalid_framework_body("doctor", source)),
        Some(HttpRouteKind::Search) => decode_search_query(&request.path)
            .map(Box::new)
            .map(ApiRequest::Search),
        Some(HttpRouteKind::Compatibility) => {
            serde_json::from_slice::<CompatibilityPreflight>(body)
                .map(ApiRequest::CompatibilityPreflight)
                .map_err(|source| invalid_framework_body("compatibility", source))
        }
        Some(HttpRouteKind::Heartbeat) => {
            serde_json::from_slice::<TeamMemberHeartbeatRequest>(body)
                .map(ApiRequest::Heartbeat)
                .map_err(|source| invalid_framework_body("heartbeat", source))
        }
        Some(HttpRouteKind::QueueGetNext) => serde_json::from_slice::<QueueGetNextRequest>(body)
            .map(ApiRequest::QueueGetNext)
            .map_err(|source| invalid_framework_body("queue get next", source)),
        Some(
            kind @ (HttpRouteKind::GraftReceiverRegister
            | HttpRouteKind::GraftReceiverRefresh
            | HttpRouteKind::GraftReceiverUnregister
            | HttpRouteKind::GraftReceiverLookup),
        ) => decode_graft_receiver_request(kind, body)
            .expect("graft receiver route kind must decode to a graft receiver request"),
        Some(HttpRouteKind::RuntimeReload) => serde_json::from_slice::<()>(body)
            .map(|()| ApiRequest::ReloadRuntimeView)
            .map_err(|source| invalid_framework_body("runtime reload", source)),
        None => Err(AtmError::validation_with_recovery(
            format!("unsupported daemon HTTP route {method} {}", request.path),
            "use a method and path from the daemon HTTP route contract and retry",
        )),
    }
}

/// Decodes one of the four local-ingress-only graft receiver routes.
///
/// Returns `None` for any other route kind so the caller's route dispatch
/// stays exhaustive over `HttpRouteKind` without duplicating this match arm
/// group inline (kept `decode_framework_request` under the repository's
/// function-length gate).
fn decode_graft_receiver_request(
    kind: HttpRouteKind,
    body: &[u8],
) -> Option<Result<ApiRequest, AtmError>> {
    let invalid = |request_kind: &str, source: serde_json::Error| {
        AtmError::validation_with_recovery(
            format!("invalid {request_kind} HTTP request body: {source}"),
            "ensure the client sends the documented JSON request body and retry",
        )
    };
    match kind {
        HttpRouteKind::GraftReceiverRegister => Some(
            serde_json::from_slice::<GraftReceiverRegistration>(body)
                .map(ApiRequest::GraftReceiverRegister)
                .map_err(|source| invalid("graft receiver register", source)),
        ),
        HttpRouteKind::GraftReceiverRefresh => Some(
            serde_json::from_slice::<GraftReceiverRefreshRequest>(body)
                .map(ApiRequest::GraftReceiverRefresh)
                .map_err(|source| invalid("graft receiver refresh", source)),
        ),
        HttpRouteKind::GraftReceiverUnregister => Some(
            serde_json::from_slice::<GraftReceiverUnregistration>(body)
                .map(ApiRequest::GraftReceiverUnregister)
                .map_err(|source| invalid("graft receiver unregister", source)),
        ),
        HttpRouteKind::GraftReceiverLookup => Some(
            serde_json::from_slice::<GraftReceiverLookupRequest>(body)
                .map(|value| ApiRequest::GraftReceiverLookup {
                    team: value.team,
                    agent: value.agent,
                })
                .map_err(|source| invalid("graft receiver lookup", source)),
        ),
        _ => None,
    }
}

/// Decodes the canonical bodyless GET representation for search.  The value
/// is URL-safe base64 JSON so a complete typed `SearchRequest` remains one
/// stable HTTP query parameter without inventing a second filter grammar.
fn decode_search_query(path: &str) -> Result<SearchRequest, AtmError> {
    let (_, query) = path.split_once('?').ok_or_else(|| {
        AtmError::validation_with_recovery(
            "message search HTTP request is missing its request query parameter",
            "send GET /v1/atm/messages/search?request=<base64url-json> and retry",
        )
    })?;
    let mut request_values = query
        .split('&')
        .filter_map(|field| field.split_once('='))
        .filter(|(name, _)| *name == "request")
        .map(|(_, value)| value);
    let encoded = request_values.next().ok_or_else(|| {
        AtmError::validation_with_recovery(
            "message search HTTP request is missing its request query parameter",
            "send GET /v1/atm/messages/search?request=<base64url-json> and retry",
        )
    })?;
    if request_values.next().is_some() {
        return Err(AtmError::validation_with_recovery(
            "message search HTTP request repeats its request query parameter",
            "send exactly one request query parameter and retry",
        ));
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|source| {
            AtmError::validation_with_recovery(
                "message search HTTP request query parameter is not valid base64url",
                "encode the JSON SearchRequest as unpadded URL-safe base64 and retry",
            )
            .with_cause(source)
        })?;
    serde_json::from_slice(&bytes).map_err(|source| {
        AtmError::validation_with_recovery(
            format!("message search HTTP request JSON is invalid: {source}"),
            "encode the documented SearchRequest JSON in the request query parameter and retry",
        )
    })
}

fn canonical_headers(headers: &HeaderMap) -> Result<Vec<String>, AtmError> {
    headers
        .iter()
        .map(|(name, value)| {
            let value = value.to_str().map_err(|source| {
                AtmError::validation("canonical HTTP request contains a non-text header value")
                    .with_cause(source)
            })?;
            Ok(format!("{name}: {value}"))
        })
        .collect()
}

fn validate_request_headers(headers: &HeaderMap) -> Result<(), AtmError> {
    if headers.contains_key(RETIRED_PEER_PROVENANCE_HEADER) {
        return Err(AtmError::validation(format!(
            "{RETIRED_PEER_PROVENANCE_HEADER} is not accepted by canonical HTTP ingress"
        )));
    }
    Ok(())
}

fn request_id_from_headers(headers: &HeaderMap) -> Option<RequestId> {
    let values = headers.get_all(REQUEST_ID_HEADER);
    let mut values = values.iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some() {
        return None;
    }
    value
        .parse::<u64>()
        .ok()
        .and_then(|value| RequestId::new(value).ok())
}

async fn overload_response(_: BoxError) -> Response {
    warn!("HTTP message ingress rejected because its in-flight capacity is saturated");
    error_response(AtmError::daemon_connection_saturated(
        "HTTP message ingress is at its configured in-flight capacity",
    ))
}

fn framework_rejection(rejection: JsonRejection) -> AtmError {
    let detail = match &rejection {
        JsonRejection::JsonDataError(_) => {
            "invalid HTTP messages request: JSON fields have invalid types"
        }
        JsonRejection::JsonSyntaxError(_) => "invalid HTTP messages request: malformed JSON",
        JsonRejection::MissingJsonContentType(_) => {
            "invalid HTTP messages request: Content-Type must be application/json"
        }
        JsonRejection::BytesRejection(_) => "invalid HTTP messages request: body could not be read",
        _ => "invalid HTTP messages request: request body was rejected",
    };
    AtmError::validation(detail).with_cause(rejection)
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
        _ => Err(AtmError::new(
            atm_core::error::AtmErrorCode::InternalError,
            "canonical message route received a non-write application response",
        )),
    }
}

fn map_api_response(response: ApiResponse) -> Result<Response, AtmError> {
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
        ResponseEnvelope::CompatibilityVerdict(value) => {
            json_response(StatusCode::OK, &value, None)
        }
        ResponseEnvelope::Heartbeat(value) => json_response(StatusCode::OK, &value, None),
        ResponseEnvelope::QueueGetNext(value) => json_response(StatusCode::OK, &value, None),
        ResponseEnvelope::GraftReceiverRegister => json_response(StatusCode::OK, &(), None),
        ResponseEnvelope::GraftReceiverRefresh => json_response(StatusCode::OK, &(), None),
        ResponseEnvelope::GraftReceiverUnregister => json_response(StatusCode::OK, &(), None),
        ResponseEnvelope::GraftReceiverLookup(value) => json_response(StatusCode::OK, &value, None),
        ResponseEnvelope::List(value) => json_response(StatusCode::OK, &value, None),
        ResponseEnvelope::Peek(value) => json_response(StatusCode::OK, &value, None),
        ResponseEnvelope::Receive(value) => json_response(StatusCode::OK, &value, None),
        ResponseEnvelope::Clear(value) => clear_response(&value),
        ResponseEnvelope::Doctor(value) => json_response(StatusCode::OK, &value, None),
        ResponseEnvelope::Search(value) => json_response(StatusCode::OK, &value, None),
        ResponseEnvelope::RuntimeViewReloaded => json_response(StatusCode::OK, &(), None),
        ResponseEnvelope::Error(error) => Ok(error_response(error)),
    }
}

fn clear_response(outcome: &atm_core::clear::ClearOutcome) -> Result<Response, AtmError> {
    use base64::Engine as _;

    let encoded = serde_json::to_vec(outcome).map_err(AtmError::from)?;
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(encoded);
    let header = HeaderValue::from_str(&header).map_err(|source| {
        AtmError::new(
            atm_core::error::AtmErrorCode::SerializationFailed,
            "failed to serialize canonical clear outcome header",
        )
        .with_cause(source)
    })?;
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::NO_CONTENT;
    response.headers_mut().insert(CLEAR_OUTCOME_HEADER, header);
    Ok(response)
}

#[derive(Serialize)]
struct HttpErrorBody<'error> {
    code: AtmErrorCode,
    message: &'error str,
}

impl<'error> From<&'error AtmError> for HttpErrorBody<'error> {
    fn from(error: &'error AtmError) -> Self {
        // HTTP is an untrusted transport boundary. `cause` remains available
        // to local diagnostics, but must never be serialized to a peer.
        Self {
            code: error.code(),
            message: error.message(),
        }
    }
}

pub(crate) fn error_response(error: AtmError) -> Response {
    let status = if error.is_validation()
        || matches!(
            error.code(),
            atm_core::error::AtmErrorCode::LocalHttpCapabilityInvalid
                | atm_core::error::AtmErrorCode::LocalHttpEndpointNonLoopback
        ) {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    // Preserve the public code/message contract while redacting diagnostic
    // causes before serializing through the untrusted HTTP boundary.
    let body = HttpErrorBody::from(&error);
    json_response(status, &body, None).unwrap_or_else(|_| Response::new(Body::empty()))
}

fn json_response<T: Serialize>(
    status: StatusCode,
    value: &T,
    message_id: Option<String>,
) -> Result<Response, AtmError> {
    let body = serde_json::to_vec(value).map_err(|source| {
        AtmError::new(
            atm_core::error::AtmErrorCode::SerializationFailed,
            "failed to serialize canonical HTTP response",
        )
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
            AtmError::new(
                atm_core::error::AtmErrorCode::SerializationFailed,
                "failed to serialize message location header",
            )
            .with_cause(source)
        })?;
        response.headers_mut().insert(LOCATION, location);
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::Duration;

    use atm_core::api::HttpRequest;
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::protocol::ResponseEnvelope;
    use atm_core::search::{SearchRequest, SearchResponse};
    use atm_core::send::{SendCommandOutcome, SendMessageSource, SendOutcome};
    use atm_core::types::CommandAction;
    use atm_core::workflow_analytics::{WorkflowProjectionRequest, WorkflowSelector};
    use atm_core::{ApiResponse, AuthenticatedIngress, RequestDeadline};
    use atm_storage::{
        ChatId, IsoTimestamp, MessageKey, SearchAggregate, SearchGroup, SearchGroupBy,
        SearchResultKey, SearchTimestampField, StoredSearchAddress,
    };
    use axum::body::{Body, to_bytes};
    use axum::http::header::{CONTENT_TYPE, HeaderName};
    use axum::http::{HeaderMap, HeaderValue, Request, StatusCode};
    use base64::Engine as _;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use super::{
        AuthenticatedConnector, CanonicalWriteHandler, REQUEST_ID_HEADER,
        RETIRED_PEER_PROVENANCE_HEADER, canonical_api_router, canonical_message_router,
        canonical_write_path, decode_framework_request, json_response, map_write_response,
        request_id_from_headers,
    };
    use crate::{NonZeroDuration, RuntimeLimits, RuntimeTimeouts};

    #[derive(Clone)]
    struct RecordingRouter {
        response: ResponseEnvelope,
        calls: Arc<Mutex<Vec<(atm_core::send::WriteRequest, AuthenticatedIngress)>>>,
    }

    impl atm_core::boundary::sealed::Sealed for RecordingRouter {}

    impl CanonicalWriteHandler for RecordingRouter {
        fn write(
            &self,
            request: atm_core::send::WriteRequest,
            ingress: AuthenticatedIngress,
            _deadline: RequestDeadline,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<ApiResponse, AtmError>> + Send + '_>,
        > {
            Box::pin(async move {
                self.calls
                    .lock()
                    .expect("record calls")
                    .push((request, ingress));
                Ok(ApiResponse::new(self.response.clone()))
            })
        }
    }

    struct BlockingRouter {
        gate: Arc<Gate>,
        response: ResponseEnvelope,
    }

    impl atm_core::boundary::sealed::Sealed for BlockingRouter {}

    struct FailingSerialize;

    impl serde::Serialize for FailingSerialize {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            Err(serde::ser::Error::custom(
                "intentional serialization failure",
            ))
        }
    }

    impl CanonicalWriteHandler for BlockingRouter {
        fn write(
            &self,
            _request: atm_core::send::WriteRequest,
            _ingress: AuthenticatedIngress,
            _deadline: RequestDeadline,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<ApiResponse, AtmError>> + Send + '_>,
        > {
            Box::pin(async move {
                let gate = Arc::clone(&self.gate);
                let response = self.response.clone();
                tokio::task::spawn_blocking(move || {
                    gate.wait_until_released();
                    Ok(ApiResponse::new(response))
                })
                .await
                .map_err(|source| {
                    AtmError::new(
                        atm_core::error::AtmErrorCode::InternalError,
                        "test blocking write task ended unexpectedly",
                    )
                    .with_cause(source)
                })?
            })
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
        scheduler_progressed: bool,
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

        fn note_scheduler_progress(&self) {
            self.state.lock().expect("lock gate").scheduler_progressed = true;
            self.changed.notify_all();
        }

        fn wait_until_scheduler_progresses(&self) {
            let mut state = self.state.lock().expect("lock gate");
            while !state.scheduler_progressed {
                state = self.changed.wait(state).expect("wait gate");
            }
        }
    }

    fn limits(body_bytes: usize, in_flight: usize) -> RuntimeLimits {
        RuntimeLimits::new(
            NonZeroUsize::new(body_bytes).expect("non-zero body limit"),
            NonZeroUsize::new(in_flight).expect("non-zero in-flight limit"),
        )
    }

    fn timeouts() -> RuntimeTimeouts {
        timeouts_with_request(Duration::from_secs(1))
    }

    fn timeouts_with_request(request: Duration) -> RuntimeTimeouts {
        RuntimeTimeouts::new(
            NonZeroDuration::new(request).expect("non-zero request timeout"),
            NonZeroDuration::new(Duration::from_secs(1)).expect("non-zero shutdown timeout"),
        )
    }

    #[test]
    fn request_id_header_preserves_a_valid_peer_correlation_id() {
        let mut headers = HeaderMap::new();
        headers.insert(REQUEST_ID_HEADER, HeaderValue::from_static("42"));

        assert_eq!(
            request_id_from_headers(&headers)
                .expect("valid header")
                .into_inner(),
            42
        );

        headers.append(REQUEST_ID_HEADER, HeaderValue::from_static("43"));
        assert!(request_id_from_headers(&headers).is_none());
    }

    fn write_request() -> atm_core::send::WriteRequest {
        let temporary_root = tempfile::tempdir().expect("temporary request paths");
        atm_core::send::WriteRequest::new(
            temporary_root.path().join("home"),
            temporary_root.path().join("workspace"),
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
        let mut request = Request::builder()
            .method("POST")
            .uri(canonical_write_path());
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

    #[derive(Clone, Copy)]
    enum An15HttpProbe {
        Shape,
        ContentType,
        HeaderBoundary,
    }

    const AN15_HTTP_CASE_SHAPES: [&[&str]; 3] = [
        &[
            "truncated-object",
            "truncated-array",
            "truncated-field",
            "plain-text",
            "wrong-scalar",
        ],
        &["plain-text", "xml", "html", "binary", "form"],
        &[
            "retired-header",
            "non-json-content-type",
            "json-syntax",
            "json-data",
        ],
    ];

    fn an15_http_case_shape(probe: An15HttpProbe, case_index: usize) -> (usize, &'static str) {
        let probe_index = match probe {
            An15HttpProbe::Shape => 0,
            An15HttpProbe::ContentType => 1,
            An15HttpProbe::HeaderBoundary => 2,
        };
        let slot = case_index % AN15_HTTP_CASE_SHAPES[probe_index].len();
        (slot, AN15_HTTP_CASE_SHAPES[probe_index][slot])
    }

    #[tokio::test]
    async fn error_response_redacts_diagnostic_causes_at_the_http_boundary() {
        let diagnostic_secret = "Bearer test-only-http-error-secret";
        let response = super::error_response(
            AtmError::validation("HTTP error redaction fixture").with_cause(diagnostic_secret),
        );
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body: Value = serde_json::from_slice(&response_body(response).await)
            .expect("redacted HTTP error response");
        assert_eq!(body["code"], "ATM_MESSAGE_VALIDATION_FAILED");
        assert!(
            body["message"]
                .as_str()
                .expect("stable error message")
                .contains("HTTP error redaction fixture")
        );
        assert!(body.get("cause").is_none(), "HTTP must not expose causes");
        assert!(
            !body.to_string().contains(diagnostic_secret),
            "HTTP must not expose diagnostic secrets"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an15_http_shape_probe_rejects_malformed_json_before_dispatch() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let app = canonical_message_router(
            Arc::new(RecordingRouter {
                response: sent_response(),
                calls: Arc::clone(&calls),
            }),
            AuthenticatedConnector::local(),
            limits(4096, 2),
            timeouts(),
        );

        for case_index in 0..100 {
            let (shape_slot, shape) = an15_http_case_shape(An15HttpProbe::Shape, case_index);
            let body = match shape_slot {
                0 => b"{".to_vec(),
                1 => b"[".to_vec(),
                2 => b"{\"to\":".to_vec(),
                3 => b"not-json".to_vec(),
                _ => b"{\"to\":true".to_vec(),
            };
            eprintln!("AN15_CASE_SHAPE={shape}");
            let response = post(app.clone(), body).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let error: AtmError = serde_json::from_slice(&response_body(response).await)
                .expect("typed validation error response");
            assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
            assert!(
                error.message().contains("invalid HTTP messages request"),
                "case {case_index} must retain the stable malformed-request diagnostic"
            );
            assert!(
                error.message().contains(
                    "Recovery: Correct the invalid ATM request or state before retrying."
                ),
                "case {case_index} must retain the actionable validation recovery"
            );
        }
        assert!(calls.lock().expect("record calls").is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an15_http_template_probe_rejects_non_json_content_before_dispatch() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let app = canonical_message_router(
            Arc::new(RecordingRouter {
                response: sent_response(),
                calls: Arc::clone(&calls),
            }),
            AuthenticatedConnector::local(),
            limits(4096, 2),
            timeouts(),
        );
        let body = serde_json::to_vec(&write_request()).expect("typed request JSON");

        for case_index in 0..100 {
            let (shape_slot, shape) = an15_http_case_shape(An15HttpProbe::ContentType, case_index);
            let content_type = match shape_slot {
                0 => "text/plain",
                1 => "application/xml",
                2 => "text/html",
                3 => "application/octet-stream",
                _ => "application/x-www-form-urlencoded",
            };
            eprintln!("AN15_CASE_SHAPE={shape}");
            let response =
                post_with_headers(app.clone(), body.clone(), &[(CONTENT_TYPE, content_type)]).await;
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "case {case_index}"
            );
            let error: AtmError = serde_json::from_slice(&response_body(response).await)
                .expect("typed validation error response");
            assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
            assert!(
                error.message().contains(
                    "Recovery: Correct the invalid ATM request or state before retrying."
                ),
                "case {case_index} must retain the actionable validation recovery"
            );
        }
        assert!(calls.lock().expect("record calls").is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an15_http_boundary_probe_rejects_retired_provenance_before_dispatch() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let app = canonical_message_router(
            Arc::new(RecordingRouter {
                response: sent_response(),
                calls: Arc::clone(&calls),
            }),
            AuthenticatedConnector::local(),
            limits(4096, 2),
            timeouts(),
        );
        let retired = HeaderName::from_static("x-atm-peer-source-host");

        for case_index in 0..100 {
            let (shape_slot, shape) =
                an15_http_case_shape(An15HttpProbe::HeaderBoundary, case_index);
            let (body, headers, expected_message) = match shape_slot {
                0 => (
                    serde_json::to_vec(&write_request()).expect("typed request JSON"),
                    vec![
                        (CONTENT_TYPE, "application/json"),
                        (retired.clone(), "ignored"),
                    ],
                    "X-ATM-Peer-Source-Host is not accepted",
                ),
                1 => (
                    serde_json::to_vec(&write_request()).expect("typed request JSON"),
                    vec![(CONTENT_TYPE, "text/plain")],
                    "invalid HTTP messages request: Content-Type must be application/json",
                ),
                2 => (
                    b"{".to_vec(),
                    vec![(CONTENT_TYPE, "application/json")],
                    "invalid HTTP messages request: malformed JSON",
                ),
                3 => (
                    b"{\"to\":true}".to_vec(),
                    vec![(CONTENT_TYPE, "application/json")],
                    "invalid HTTP messages request: JSON fields have invalid types",
                ),
                _ => unreachable!("header-boundary corpus has four shapes"),
            };
            eprintln!("AN15_CASE_SHAPE={shape}");
            let response = post_with_headers(app.clone(), body, &headers).await;
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "case {case_index}"
            );
            let error: AtmError = serde_json::from_slice(&response_body(response).await)
                .expect("typed validation error response");
            assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
            assert!(
                error.message().contains(expected_message),
                "case {case_index} must retain the production header/framing diagnostic"
            );
            assert!(
                error.message().contains(
                    "Recovery: Correct the invalid ATM request or state before retrying."
                ),
                "case {case_index} must retain the actionable validation recovery"
            );
        }
        assert!(calls.lock().expect("record calls").is_empty());
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
        assert!(
            peer_calls[0]
                .0
                .to
                .as_ref()
                .expect("peer write retains a recipient")
                .host()
                .is_none(),
            "the physical peer qualifier is consumed before shared mailbox routing"
        );
        assert_eq!(peer_calls[0].1, AuthenticatedIngress::Peer);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn production_router_registers_every_route_from_the_core_contract() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let app = canonical_api_router(
            Arc::new(RecordingRouter {
                response: sent_response(),
                calls,
            }),
            AuthenticatedConnector::local(),
            limits(4096, 2),
            timeouts(),
        );

        for route in atm_core::api::http_route_surface() {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(route.method)
                        .uri(route.path_template)
                        .body(Body::empty())
                        .expect("core route request"),
                )
                .await
                .expect("infallible Axum service");
            assert_ne!(
                response.status(),
                StatusCode::NOT_FOUND,
                "{} {} must be registered by the production router",
                route.method,
                route.path_template
            );
            assert_ne!(
                response.status(),
                StatusCode::METHOD_NOT_ALLOWED,
                "{} {} must retain its core method",
                route.method,
                route.path_template
            );
        }
    }

    #[test]
    fn framework_decoder_recognizes_every_route_from_the_core_contract() {
        for route in atm_core::api::http_route_surface() {
            let result = decode_framework_request(
                HttpRequest {
                    method: route.method.to_owned(),
                    path: route.path_template.to_owned(),
                    headers: Vec::new(),
                    body: Vec::new(),
                },
                1,
            );
            let handled = match result {
                Ok(_) => true,
                Err(error) => {
                    error.message().contains("invalid") || error.message().contains("missing")
                }
            };
            assert!(
                handled,
                "{} {} must be handled by the framework decoder",
                route.method, route.path_template
            );
        }
    }

    #[test]
    fn search_route_decodes_the_shared_core_request_contract() {
        let expected = SearchRequest {
            query: atm_core::search::SearchInput::default(),
            lifecycle: Some(WorkflowProjectionRequest {
                scope_kind: atm_storage::WorkflowScopeKind::new("release-train")
                    .expect("valid opaque scope kind"),
                scope_id: Some(
                    atm_storage::WorkflowScopeId::new("train-42").expect("valid opaque scope id"),
                ),
                start: WorkflowSelector {
                    state: Some(
                        atm_storage::WorkflowState::new("queued").expect("valid opaque state"),
                    ),
                    stage: None,
                    transition: None,
                },
                end: WorkflowSelector {
                    state: Some(
                        atm_storage::WorkflowState::new("shipped").expect("valid opaque state"),
                    ),
                    stage: None,
                    transition: None,
                },
                time_range: None,
            }),
        };
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&expected).expect("JSON"));
        let decoded = decode_framework_request(
            HttpRequest {
                method: "GET".to_owned(),
                path: format!("/v1/atm/messages/search?request={encoded}"),
                headers: Vec::new(),
                body: Vec::new(),
            },
            1024,
        )
        .expect("search request");
        assert!(matches!(decoded, atm_core::api::ApiRequest::Search(value) if *value == expected));
    }

    #[test]
    fn malformed_query_keys_fail_after_the_real_http_request_decoder() {
        for query in [
            atm_core::search::SearchInput {
                template_meta: vec!["../phase=an".to_owned()],
                ..Default::default()
            },
            atm_core::search::SearchInput {
                vars: vec!["phase/path=an".to_owned()],
                ..Default::default()
            },
            atm_core::search::SearchInput {
                aggregate: Some(atm_core::search::SearchAggregateInput::GroupBy(
                    "var:../../phase".to_owned(),
                )),
                ..Default::default()
            },
        ] {
            let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
                serde_json::to_vec(&SearchRequest {
                    query,
                    lifecycle: None,
                })
                .expect("request JSON"),
            );
            let request = decode_framework_request(
                HttpRequest {
                    method: "GET".to_owned(),
                    path: format!("/v1/atm/messages/search?request={encoded}"),
                    headers: Vec::new(),
                    body: Vec::new(),
                },
                1024,
            )
            .expect("HTTP transport accepts the public DTO");
            let atm_core::api::ApiRequest::Search(request) = request else {
                unreachable!("search request")
            };
            let error = request
                .compile_query()
                .expect_err("core compilation rejects invalid key");
            assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
        }
    }

    #[test]
    fn search_response_uses_the_shared_core_response_contract() {
        let response = super::map_api_response(atm_core::ApiResponse::new(
            ResponseEnvelope::Search(Box::new(SearchResponse {
                hits: Vec::new(),
                aggregate: None,
                next_cursor: None,
                lifecycle: None,
            })),
        ))
        .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn search_response_contract_round_trips_every_aggregate_variant() {
        let timestamp: IsoTimestamp = "2026-08-12T00:00:00Z".parse().expect("timestamp");
        let aggregates = [
            SearchAggregate::Count { value: 3 },
            SearchAggregate::Groups {
                by: SearchGroupBy::Field(atm_storage::SearchGroupField::Category),
                groups: vec![SearchGroup {
                    key: "task".to_owned(),
                    count: 2,
                }],
            },
            SearchAggregate::Timestamp {
                field: SearchTimestampField::MessageAt,
                value: Some(timestamp),
            },
            SearchAggregate::Timestamp {
                field: SearchTimestampField::MessageAt,
                value: None,
            },
        ];
        for aggregate in aggregates {
            let response = SearchResponse {
                hits: Vec::new(),
                aggregate: Some(aggregate),
                next_cursor: None,
                lifecycle: None,
            };
            let json = serde_json::to_value(&response).expect("serialize response");
            assert_eq!(
                serde_json::from_value::<SearchResponse>(json).expect("deserialize response"),
                response
            );
        }
    }

    #[test]
    fn search_response_contract_preserves_address_dimensions() {
        let team: atm_storage::TeamName = "query-team".parse().expect("team");
        let sender: atm_storage::AgentName = "sender".parse().expect("sender");
        let recipient: atm_storage::AgentName = "recipient".parse().expect("recipient");
        let sender_chat: ChatId = "1001".parse().expect("sender chat id");
        let recipient_chat: ChatId = "1002".parse().expect("recipient chat id");
        let message_at: IsoTimestamp = "2026-08-12T00:00:00Z".parse().expect("timestamp");
        let response = SearchResponse {
            hits: vec![atm_core::search::SearchHit {
                key: SearchResultKey {
                    team: team.clone(),
                    agent: recipient.clone(),
                    message_key: MessageKey::new("atm:address-contract").expect("message key"),
                },
                message_id: Some("01KZTTRD6K9WJYJ2N7E39CVB9P".to_owned()),
                message_at,
                from_agent: StoredSearchAddress {
                    agent: sender,
                    team: team.clone(),
                    chat_id: Some(sender_chat.clone()),
                },
                to_agent: StoredSearchAddress {
                    agent: recipient,
                    team,
                    chat_id: Some(recipient_chat.clone()),
                },
                template_type: Some("dev-task".to_owned()),
                category: Some("workflow".to_owned()),
                snippet: "address contract".to_owned(),
                workflow: None,
            }],
            aggregate: None,
            next_cursor: None,
            lifecycle: None,
        };
        let json = serde_json::to_value(&response).expect("serialize response");
        assert_eq!(
            json["hits"][0]["from_agent"]["chat_id"],
            serde_json::json!(sender_chat)
        );
        assert_eq!(
            json["hits"][0]["to_agent"]["chat_id"],
            serde_json::json!(recipient_chat)
        );
        assert_eq!(
            serde_json::from_value::<SearchResponse>(json).expect("deserialize response"),
            response
        );
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
                    HeaderName::from_bytes(RETIRED_PEER_PROVENANCE_HEADER.as_bytes())
                        .expect("canonical peer source header"),
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
    async fn retired_peer_provenance_header_is_rejected_before_dispatch() {
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
        let headers = [
            (CONTENT_TYPE, "application/json"),
            (
                HeaderName::from_bytes(RETIRED_PEER_PROVENANCE_HEADER.as_bytes())
                    .expect("retired peer provenance header"),
                "not a valid host",
            ),
        ];

        let response = post_with_headers(app, request, &headers).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let error: AtmError = serde_json::from_slice(&response_body(response).await)
            .expect("ADR-032 header error JSON");
        assert!(error.is_validation(), "{error:?}");
        assert!(
            error.message().contains(RETIRED_PEER_PROVENANCE_HEADER),
            "{error:?}"
        );
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

    #[test]
    fn response_translation_uses_internal_and_serialization_error_codes() {
        let mismatch =
            match map_write_response(ApiResponse::new(ResponseEnvelope::RuntimeViewReloaded)) {
                Ok(_) => panic!("non-write response must be rejected"),
                Err(error) => error,
            };
        assert_eq!(mismatch.code(), AtmErrorCode::InternalError);

        let serialization = match json_response(StatusCode::OK, &FailingSerialize, None) {
            Ok(_) => panic!("failing serializer must be rejected"),
            Err(error) => error,
        };
        assert_eq!(serialization.code(), AtmErrorCode::SerializationFailed);
    }

    #[test]
    fn canonical_route_is_selected_from_the_core_route_surface() {
        assert!(atm_core::api::http_route_surface().any(|route| {
            route.method == "POST" && route.path_template == canonical_write_path()
        }));
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
        assert_eq!(error.code(), AtmErrorCode::DaemonConnectionSaturated);

        gate.release();
        assert_eq!(
            first.join().expect("first request thread").status(),
            StatusCode::CREATED
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn blocking_handler_operation_does_not_stall_the_tokio_worker() {
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
        let (start_progress, wait_for_start) = tokio::sync::oneshot::channel();
        let progressed = Arc::clone(&gate);
        tokio::spawn(async move {
            wait_for_start
                .await
                .expect("blocking handler entered before scheduler progress check");
            progressed.note_scheduler_progress();
        });
        let entered = Arc::clone(&gate);
        let release_gate = Arc::clone(&gate);
        let release = std::thread::spawn(move || {
            entered.wait_until_entered();
            start_progress
                .send(())
                .expect("scheduler progress task remains available");
            release_gate.wait_until_scheduler_progresses();
            release_gate.release();
        });

        let response = post(
            app,
            serde_json::to_vec(&write_request()).expect("typed JSON"),
        )
        .await;

        release.join().expect("release gate thread");
        assert_eq!(response.status(), StatusCode::CREATED);
        assert!(
            gate.state.lock().expect("lock gate").scheduler_progressed,
            "the Tokio worker must remain schedulable while the blocking handler operation runs"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn started_dispatch_returns_its_actual_response_after_the_advisory_deadline() {
        let gate = Arc::new(Gate::default());
        let app = canonical_message_router(
            Arc::new(BlockingRouter {
                gate: Arc::clone(&gate),
                response: sent_response(),
            }),
            AuthenticatedConnector::local(),
            limits(4096, 1),
            timeouts_with_request(Duration::from_millis(10)),
        );
        let response = tokio::spawn(post(
            app,
            serde_json::to_vec(&write_request()).expect("typed JSON"),
        ));
        let entered = Arc::clone(&gate);
        tokio::task::spawn_blocking(move || entered.wait_until_entered())
            .await
            .expect("wait for blocking handler");
        tokio::time::advance(Duration::from_millis(25)).await;
        assert!(
            !response.is_finished(),
            "the adapter must not synthesize a timeout while a started route owns the durable outcome"
        );
        gate.release();
        let response = response
            .await
            .expect("request task joins after the started route completes");
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    #[test]
    fn openapi_and_serde_keep_the_existing_typed_write_and_search_contracts() {
        let openapi: Value =
            serde_yaml::from_str(include_str!("../../../docs/atm-http-runtime/openapi.yaml"))
                .expect("parse checked-in OpenAPI document");
        let write_operation = openapi
            .pointer("/paths/~1messages/post")
            .expect("OpenAPI must declare POST /messages");

        assert_eq!(
            write_operation,
            &json!({
                "operationId": "writeMessage",
                "requestBody": {
                    "required": true,
                    "content": {
                        "application/json": {
                            "schema": {"$ref": "#/components/schemas/WriteRequest"}
                        }
                    }
                },
                "responses": {
                    "201": {
                        "description": "Created message",
                        "content": {
                            "application/json": {
                                "schema": {"$ref": "#/components/schemas/Message"}
                            }
                        }
                    },
                    "default": {"$ref": "#/components/responses/AtmError"}
                }
            }),
            "the parsed OpenAPI write operation must remain the AL.1 compatibility oracle"
        );

        let search_operation = openapi
            .pointer("/paths/~1messages~1search/get")
            .expect("OpenAPI must declare GET /messages/search");
        assert_eq!(
            search_operation
                .pointer("/operationId")
                .and_then(Value::as_str),
            Some("searchMessages")
        );
        assert_eq!(
            search_operation
                .pointer("/parameters/0/name")
                .and_then(Value::as_str),
            Some("request")
        );
        assert_eq!(
            search_operation
                .pointer("/parameters/0/schema/format")
                .and_then(Value::as_str),
            Some("base64url")
        );

        let serialized = serde_json::to_value(write_request())
            .expect("serialize the existing route-specific WriteRequest");
        let round_tripped: atm_core::send::WriteRequest =
            serde_json::from_value(serialized.clone())
                .expect("deserialize the existing route-specific WriteRequest");
        assert_eq!(
            serde_json::to_value(round_tripped).expect("re-serialize WriteRequest"),
            serialized,
            "the handler must retain the existing WriteRequest Serde representation"
        );

        for aggregate in [
            atm_core::search::SearchAggregateInput::Count,
            atm_core::search::SearchAggregateInput::GroupBy("var:phase".to_owned()),
            atm_core::search::SearchAggregateInput::MinMessageAt,
            atm_core::search::SearchAggregateInput::MaxMessageAt,
        ] {
            let search = SearchRequest {
                query: atm_core::search::SearchInput {
                    aggregate: Some(aggregate),
                    ..atm_core::search::SearchInput::default()
                },
                lifecycle: None,
            };
            let serialized = serde_json::to_value(&search).expect("serialize SearchRequest");
            assert_eq!(
                serde_json::from_value::<SearchRequest>(serialized.clone())
                    .expect("deserialize SearchRequest"),
                search
            );
        }

        let forbidden = ["Http", "Frame", "Reader"].concat();
        assert!(!include_str!("message_handler.rs").contains(&forbidden));
        let forbidden = ["Peer", "Message", "Array"].concat();
        assert!(!include_str!("message_handler.rs").contains(&forbidden));
    }
}
