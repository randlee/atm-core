//! Transport-neutral API request and response contracts.
//!
//! UDS and future HTTPS adapters translate HTTP into this surface.  The
//! application router receives no socket, storage, or nudge capability.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::de::DeserializeOwned;

use crate::clear::ClearQuery;
use crate::doctor::DoctorQuery;
use crate::error::AtmError;
use crate::list::ListQuery;
use crate::protocol::{
    CompatibilityPreflight, RequestEnvelope, ResponseEnvelope, TeamMemberHeartbeatRequest,
};
use crate::read::{PeekQuery, ReadQuery};
use crate::search::SearchRequest;
use crate::send::WriteRequest;
use crate::types::HostName;
use base64::Engine as _;

pub const MAX_HTTP_REQUEST_BODY_BYTES: usize = 1_048_576;
/// Version of the daemon's HTTP request contract.
pub const HTTP_API_VERSION: &str = crate::protocol::HTTP_API_VERSION;
/// HTTP response header carrying the canonical clear outcome for the `204`
/// clear route. Framework adapters use this shared contract instead of
/// inventing a JSON body for a no-content response.
pub const CLEAR_OUTCOME_HEADER: &str = "X-ATM-Clear-Outcome";
const MESSAGES_PATH: &str = "/v1/atm/messages";
const INSPECT_PATH: &str = "/v1/atm/messages/inspect";
const READ_PATH: &str = "/v1/atm/messages/read";
const DOCTOR_PATH: &str = "/v1/atm/doctor";
const COMPATIBILITY_PATH: &str = "/v1/atm/compatibility";
const HEARTBEAT_PATH: &str = "/v1/atm/heartbeat";
const RUNTIME_RELOAD_PATH: &str = "/v1/atm/runtime/reload";
const SEARCH_PATH: &str = "/v1/atm/messages/search";

/// One registered HTTP route, published from the same constants as request encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HttpRoute {
    pub method: &'static str,
    pub path_template: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpRouteKind {
    Write,
    List,
    Clear,
    Inspect,
    Receive,
    Doctor,
    RuntimeReload,
    Compatibility,
    Heartbeat,
    Search,
}

#[derive(Debug, Clone, Copy)]
struct HttpRouteSpec {
    kind: HttpRouteKind,
    route: HttpRoute,
}

// This one table is consumed by both outbound request construction and inbound
// decoding. Adding a route cannot make it to one direction without the other.
const HTTP_ROUTE_SPECS: &[HttpRouteSpec] = &[
    HttpRouteSpec {
        kind: HttpRouteKind::Search,
        route: HttpRoute {
            method: "GET",
            path_template: SEARCH_PATH,
        },
    },
    HttpRouteSpec {
        kind: HttpRouteKind::List,
        route: HttpRoute {
            method: "GET",
            path_template: MESSAGES_PATH,
        },
    },
    HttpRouteSpec {
        kind: HttpRouteKind::Write,
        route: HttpRoute {
            method: "POST",
            path_template: MESSAGES_PATH,
        },
    },
    HttpRouteSpec {
        kind: HttpRouteKind::Clear,
        route: HttpRoute {
            method: "DELETE",
            path_template: MESSAGES_PATH,
        },
    },
    HttpRouteSpec {
        kind: HttpRouteKind::Inspect,
        route: HttpRoute {
            method: "POST",
            path_template: INSPECT_PATH,
        },
    },
    HttpRouteSpec {
        kind: HttpRouteKind::Receive,
        route: HttpRoute {
            method: "POST",
            path_template: READ_PATH,
        },
    },
    HttpRouteSpec {
        kind: HttpRouteKind::Doctor,
        route: HttpRoute {
            method: "GET",
            path_template: DOCTOR_PATH,
        },
    },
    HttpRouteSpec {
        kind: HttpRouteKind::RuntimeReload,
        route: HttpRoute {
            method: "POST",
            path_template: RUNTIME_RELOAD_PATH,
        },
    },
    HttpRouteSpec {
        kind: HttpRouteKind::Compatibility,
        route: HttpRoute {
            method: "POST",
            path_template: COMPATIBILITY_PATH,
        },
    },
    HttpRouteSpec {
        kind: HttpRouteKind::Heartbeat,
        route: HttpRoute {
            method: "POST",
            path_template: HEARTBEAT_PATH,
        },
    },
];

/// Registered HTTP route inventory for documentation conformance tests.
pub fn http_route_surface() -> impl Iterator<Item = HttpRoute> {
    HTTP_ROUTE_SPECS.iter().map(|spec| spec.route)
}

fn route_spec(kind: HttpRouteKind) -> &'static HttpRouteSpec {
    // Keep outbound route selection exhaustive: adding a route kind cannot
    // compile until its shared codec entry is selected here.
    match kind {
        HttpRouteKind::Search => &HTTP_ROUTE_SPECS[0],
        HttpRouteKind::List => &HTTP_ROUTE_SPECS[1],
        HttpRouteKind::Write => &HTTP_ROUTE_SPECS[2],
        HttpRouteKind::Clear => &HTTP_ROUTE_SPECS[3],
        HttpRouteKind::Inspect => &HTTP_ROUTE_SPECS[4],
        HttpRouteKind::Receive => &HTTP_ROUTE_SPECS[5],
        HttpRouteKind::Doctor => &HTTP_ROUTE_SPECS[6],
        HttpRouteKind::RuntimeReload => &HTTP_ROUTE_SPECS[7],
        HttpRouteKind::Compatibility => &HTTP_ROUTE_SPECS[8],
        HttpRouteKind::Heartbeat => &HTTP_ROUTE_SPECS[9],
    }
}

fn route_kind_for_request(request: &RequestEnvelope) -> HttpRouteKind {
    match request {
        RequestEnvelope::Write(_) => HttpRouteKind::Write,
        RequestEnvelope::List(_) => HttpRouteKind::List,
        RequestEnvelope::Peek(_) => HttpRouteKind::Inspect,
        RequestEnvelope::Receive(_) => HttpRouteKind::Receive,
        RequestEnvelope::Clear(_) => HttpRouteKind::Clear,
        RequestEnvelope::Doctor(_) => HttpRouteKind::Doctor,
        RequestEnvelope::Search(_) => HttpRouteKind::Search,
        RequestEnvelope::ReloadRuntimeView => HttpRouteKind::RuntimeReload,
        RequestEnvelope::CompatibilityPreflight(_) => HttpRouteKind::Compatibility,
        RequestEnvelope::Heartbeat(_) => HttpRouteKind::Heartbeat,
    }
}

/// Resolves a framework HTTP method and path through the canonical route table.
#[must_use]
pub fn http_route_kind(method: &str, path: &str) -> Option<HttpRouteKind> {
    // Request parameters are part of the HTTP representation, not the route
    // identity.  In particular, search is a bodyless GET so caches and
    // ordinary HTTP tooling can address a complete typed query by URL.
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    HTTP_ROUTE_SPECS.iter().find_map(|spec| {
        (spec.route.method == method && spec.route.path_template == path).then_some(spec.kind)
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub headers: Vec<String>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        http_header(&self.headers, name)
    }
}

pub fn endpoint_for(request: &RequestEnvelope) -> (&'static str, String) {
    let spec = route_spec(route_kind_for_request(request));
    let path = spec.route.path_template.to_string();
    (spec.route.method, path)
}

/// Encodes one application request as its route-specific HTTP representation.
///
/// The application [`RequestEnvelope`] remains an in-process dispatch type;
/// this function is the one shared translation to the existing route bodies.
/// Framework-backed clients and retained stream adapters use the same encoder.
pub fn encode_http_request(
    request: &RequestEnvelope,
    adapter_headers: &[(&str, &str)],
) -> Result<HttpRequest, AtmError> {
    let body = encode_request_body(request)?;
    let (method, mut path) = endpoint_for(request);
    if let RequestEnvelope::Search(value) = request {
        let encoded = serde_json::to_vec(value).map_err(AtmError::from)?;
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(encoded);
        path.push_str("?request=");
        path.push_str(&encoded);
    }
    let mut headers = Vec::with_capacity(adapter_headers.len() + 1);
    headers.push("Content-Type: application/json".to_string());
    headers.extend(
        adapter_headers
            .iter()
            .map(|(name, value)| format!("{name}: {value}")),
    );
    Ok(HttpRequest {
        method: method.to_string(),
        path,
        headers,
        body,
    })
}

/// Decodes one route-specific HTTP response into the application envelope.
///
/// Framework-backed clients provide the status, headers, and body after their
/// connector completes; retained framing adapters provide the identical values.
pub fn decode_http_response(
    request: &RequestEnvelope,
    status: u16,
    headers: &[String],
    body: &[u8],
) -> Result<ResponseEnvelope, AtmError> {
    if status == 204 {
        return decode_no_content_response(request, headers, body);
    }
    if !(200..300).contains(&status) {
        return decode_response_body(body, "error").map(ResponseEnvelope::Error);
    }
    decode_success_response(request, body)
}

fn decode_no_content_response(
    request: &RequestEnvelope,
    headers: &[String],
    body: &[u8],
) -> Result<ResponseEnvelope, AtmError> {
    if !body.is_empty() {
        return Err(AtmError::validation_with_recovery(
            "daemon returned a body with an HTTP 204 response",
            "ensure the daemon returns an empty body for HTTP 204 responses",
        ));
    }
    let RequestEnvelope::Clear(_) = request else {
        return Err(AtmError::validation_with_recovery(
            "daemon returned HTTP 204 for a request that does not clear messages",
            "ensure the daemon returns HTTP 204 only for clear-message requests",
        ));
    };
    let encoded = http_header(headers, CLEAR_OUTCOME_HEADER).ok_or_else(|| {
        AtmError::validation_with_recovery(
            "daemon HTTP 204 response is missing clear outcome metadata",
            "ensure the daemon includes clear outcome metadata in its HTTP 204 response",
        )
    })?;
    let outcome = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|source| {
            AtmError::validation_with_recovery(
                "daemon HTTP clear outcome metadata is invalid",
                "ensure the daemon encodes valid clear outcome metadata in its HTTP 204 response",
            )
            .with_cause(source)
        })?;
    decode_response_body(&outcome, "clear outcome").map(ResponseEnvelope::Clear)
}

fn encode_request_body(request: &RequestEnvelope) -> Result<Vec<u8>, AtmError> {
    match request {
        RequestEnvelope::Write(value) => serde_json::to_vec(value),
        RequestEnvelope::CompatibilityPreflight(value) => serde_json::to_vec(value),
        RequestEnvelope::Heartbeat(value) => serde_json::to_vec(value),
        RequestEnvelope::List(value) => serde_json::to_vec(value),
        RequestEnvelope::Peek(value) => serde_json::to_vec(value),
        RequestEnvelope::Receive(value) => serde_json::to_vec(value),
        RequestEnvelope::Clear(value) => serde_json::to_vec(value),
        RequestEnvelope::Doctor(value) => serde_json::to_vec(value),
        // Search is a bodyless GET. Its typed request is encoded as the
        // URL-safe `request` query value in `encode_http_request`.
        RequestEnvelope::Search(_) => Ok(Vec::new()),
        RequestEnvelope::ReloadRuntimeView => serde_json::to_vec(&()),
    }
    .map_err(AtmError::from)
}

fn decode_response_body<T: DeserializeOwned>(
    body: &[u8],
    response_kind: &str,
) -> Result<T, AtmError> {
    serde_json::from_slice(body).map_err(|source| {
        AtmError::validation_with_recovery(
            format!("daemon HTTP {response_kind} response body is invalid: {source}"),
            "ensure the daemon returns the documented JSON response body and retry",
        )
    })
}

fn decode_success_response(
    request: &RequestEnvelope,
    body: &[u8],
) -> Result<ResponseEnvelope, AtmError> {
    match request {
        RequestEnvelope::Write(request) if request.acknowledges_message_id.is_some() => {
            decode_response_body(body, "acknowledged write").map(|value| {
                ResponseEnvelope::Send(crate::protocol::SendResponseEnvelope::Acknowledged(value))
            })
        }
        RequestEnvelope::Write(_) => decode_response_body(body, "write").map(|value| {
            ResponseEnvelope::Send(crate::protocol::SendResponseEnvelope::Sent(value))
        }),
        RequestEnvelope::CompatibilityPreflight(_) => {
            decode_response_body(body, "compatibility").map(ResponseEnvelope::CompatibilityVerdict)
        }
        RequestEnvelope::Heartbeat(_) => {
            decode_response_body(body, "heartbeat").map(ResponseEnvelope::Heartbeat)
        }
        RequestEnvelope::List(_) => decode_response_body(body, "list").map(ResponseEnvelope::List),
        RequestEnvelope::Peek(_) => {
            decode_response_body(body, "peek").map(|value| ResponseEnvelope::Peek(Box::new(value)))
        }
        RequestEnvelope::Receive(_) => decode_response_body(body, "receive")
            .map(|value| ResponseEnvelope::Receive(Box::new(value))),
        RequestEnvelope::Clear(_) => {
            decode_response_body(body, "clear").map(ResponseEnvelope::Clear)
        }
        RequestEnvelope::Doctor(_) => decode_response_body(body, "doctor")
            .map(|value| ResponseEnvelope::Doctor(Box::new(value))),
        RequestEnvelope::Search(_) => decode_response_body(body, "search")
            .map(|value| ResponseEnvelope::Search(Box::new(value))),
        RequestEnvelope::ReloadRuntimeView => decode_response_body::<()>(body, "runtime reload")
            .map(|()| ResponseEnvelope::RuntimeViewReloaded),
    }
}

fn http_header<'a>(headers: &'a [String], name: &str) -> Option<&'a str> {
    headers.iter().find_map(|header| {
        header
            .split_once(':')
            .filter(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.trim())
    })
}

#[derive(Debug, Clone)]
pub enum ApiRequest {
    Messages(Box<MessageCollectionRequest>),
    Write(Box<WriteRequest>),
    Clear(ClearQuery),
    Doctor(DoctorQuery),
    Search(Box<SearchRequest>),
    CompatibilityPreflight(CompatibilityPreflight),
    Heartbeat(TeamMemberHeartbeatRequest),
    ReloadRuntimeView,
}

#[derive(Debug, Clone)]
pub enum MessageCollectionRequest {
    List(ListQuery),
    Peek(PeekQuery),
    Receive(ReadQuery),
}

impl ApiRequest {
    pub fn new(request: RequestEnvelope) -> Self {
        Self::from(request)
    }

    pub fn into_inner(self) -> RequestEnvelope {
        match self {
            Self::Messages(request) => match *request {
                MessageCollectionRequest::List(query) => RequestEnvelope::List(query),
                MessageCollectionRequest::Peek(query) => RequestEnvelope::Peek(query),
                MessageCollectionRequest::Receive(query) => RequestEnvelope::Receive(query),
            },
            Self::Write(request) => RequestEnvelope::Write(request),
            Self::Clear(query) => RequestEnvelope::Clear(query),
            Self::Doctor(query) => RequestEnvelope::Doctor(query),
            Self::Search(query) => RequestEnvelope::Search(query),
            Self::CompatibilityPreflight(preflight) => {
                RequestEnvelope::CompatibilityPreflight(preflight)
            }
            Self::Heartbeat(request) => RequestEnvelope::Heartbeat(request),
            Self::ReloadRuntimeView => RequestEnvelope::ReloadRuntimeView,
        }
    }
}

impl From<RequestEnvelope> for ApiRequest {
    fn from(request: RequestEnvelope) -> Self {
        match request {
            RequestEnvelope::Write(request) => Self::Write(request),
            RequestEnvelope::List(query) => {
                Self::Messages(Box::new(MessageCollectionRequest::List(query)))
            }
            RequestEnvelope::Peek(query) => {
                Self::Messages(Box::new(MessageCollectionRequest::Peek(query)))
            }
            RequestEnvelope::Receive(query) => {
                Self::Messages(Box::new(MessageCollectionRequest::Receive(query)))
            }
            RequestEnvelope::Clear(query) => Self::Clear(query),
            RequestEnvelope::Doctor(query) => Self::Doctor(query),
            RequestEnvelope::Search(query) => Self::Search(query),
            RequestEnvelope::CompatibilityPreflight(preflight) => {
                Self::CompatibilityPreflight(preflight)
            }
            RequestEnvelope::Heartbeat(request) => Self::Heartbeat(request),
            RequestEnvelope::ReloadRuntimeView => Self::ReloadRuntimeView,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApiResponse {
    response: ResponseEnvelope,
}

impl ApiResponse {
    pub const fn new(response: ResponseEnvelope) -> Self {
        Self { response }
    }

    pub fn into_inner(self) -> ResponseEnvelope {
        self.response
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;

    use super::{HttpRouteKind, encode_http_request, http_route_kind};
    use crate::protocol::RequestEnvelope;
    use crate::search::SearchRequest;

    #[test]
    fn search_is_encoded_as_one_bodyless_get_query_parameter() {
        let request = RequestEnvelope::Search(Box::new(SearchRequest {
            query: crate::search::SearchInput::default(),
            lifecycle: None,
        }));
        let encoded = encode_http_request(&request, &[]).expect("HTTP request");
        assert_eq!(encoded.method, "GET");
        assert!(encoded.body.is_empty());
        assert_eq!(
            http_route_kind(&encoded.method, &encoded.path),
            Some(HttpRouteKind::Search)
        );
        let value = encoded
            .path
            .split_once("?request=")
            .expect("request query value")
            .1;
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(value)
            .expect("base64url request");
        assert_eq!(
            serde_json::from_slice::<SearchRequest>(&decoded).expect("typed JSON"),
            SearchRequest {
                query: crate::search::SearchInput::default(),
                lifecycle: None,
            }
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthenticatedIngress {
    Local,
    /// A peer that completed the HTTPS adapter's mutual-TLS and exact-pin
    /// checks. The application router receives no socket or peer configuration.
    Peer,
    /// Explicit plaintext-test provenance. This is not peer authentication.
    UntrustedSmoke(UntrustedSmokeProvenance),
    /// A plaintext diagnostic request without declared source provenance.
    /// It is never peer authentication and cannot carry a write.
    AnonymousSmoke,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntrustedSmokeProvenance {
    declared_source_host: HostName,
}

impl UntrustedSmokeProvenance {
    pub const fn new(declared_source_host: HostName) -> Self {
        Self {
            declared_source_host,
        }
    }

    pub const fn declared_source_host(&self) -> &HostName {
        &self.declared_source_host
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RequestDeadline(Instant);

impl RequestDeadline {
    pub fn after(duration: Duration) -> Self {
        Self(Instant::now() + duration)
    }

    pub fn expired(self) -> bool {
        self.remaining().is_none()
    }

    pub fn remaining(self) -> Option<Duration> {
        self.0
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
    }
}

/// The one application routing boundary for every daemon ingress.
pub trait ApiRouter: crate::boundary::sealed::Sealed + Send + Sync {
    /// Routes a validated API request to canonical application handlers.
    fn route(
        &self,
        request: ApiRequest,
        ingress: AuthenticatedIngress,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError>;
}

/// The one client-facing daemon API for CLI, graft, and test adapters.
#[async_trait]
pub trait DaemonApiClient: crate::boundary::sealed::Sealed + Send + Sync {
    /// Executes one API request through the configured transport adapter.
    async fn execute(&self, request: ApiRequest) -> Result<ApiResponse, AtmError>;
}

/// Requests that must prove the retained client/daemon compatibility contract
/// before they cross a mutating or runtime-control boundary.
///
/// This is intentionally shared by the CLI and graft adapters: allowing each
/// client crate to keep its own match list previously let their compatibility
/// requirements drift apart.
#[must_use]
pub fn request_requires_compatibility_verification(request: &RequestEnvelope) -> bool {
    matches!(
        request,
        RequestEnvelope::Write(_) | RequestEnvelope::Clear(_) | RequestEnvelope::ReloadRuntimeView
    )
}
