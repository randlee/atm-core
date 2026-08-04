//! Transport-neutral API request and response contracts.
//!
//! UDS, loopback, and configured peer HTTP adapters translate HTTP into this surface. The
//! application router receives no socket, storage, or nudge capability.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;

use crate::clear::ClearQuery;
use crate::doctor::DoctorQuery;
use crate::error::AtmError;
use crate::list::ListQuery;
use crate::protocol::{
    CompatibilityPreflight, RequestEnvelope, ResponseEnvelope, TeamMemberHeartbeatRequest,
};
use crate::read::{PeekQuery, ReadQuery};
use crate::send::WriteRequest;
use base64::Engine as _;

mod http_frame_reader;
pub use http_frame_reader::HttpFrameReader;
pub(crate) use http_frame_reader::HttpResponseFrame;

pub const MAX_HTTP_REQUEST_BODY_BYTES: usize = 1_048_576;
/// Version of the daemon's HTTP request contract.
pub const HTTP_API_VERSION: &str = crate::protocol::HTTP_API_VERSION;
const MAX_HTTP_HEADER_BYTES: usize = 16 * 1024;
const CLEAR_OUTCOME_HEADER: &str = "X-ATM-Clear-Outcome";
const MESSAGES_PATH: &str = "/v1/atm/messages";
const INSPECT_PATH: &str = "/v1/atm/messages/inspect";
const READ_PATH: &str = "/v1/atm/messages/read";
const DOCTOR_PATH: &str = "/v1/atm/doctor";
const COMPATIBILITY_PATH: &str = "/v1/atm/compatibility";
const HEARTBEAT_PATH: &str = "/v1/atm/heartbeat";
const RUNTIME_RELOAD_PATH: &str = "/v1/atm/runtime/reload";

/// One registered HTTP route, published from the same constants as request encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HttpRoute {
    pub method: &'static str,
    pub path_template: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HttpRouteKind {
    Write,
    List,
    Clear,
    Inspect,
    Receive,
    Doctor,
    RuntimeReload,
    Compatibility,
    Heartbeat,
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
        HttpRouteKind::List => &HTTP_ROUTE_SPECS[0],
        HttpRouteKind::Write => &HTTP_ROUTE_SPECS[1],
        HttpRouteKind::Clear => &HTTP_ROUTE_SPECS[2],
        HttpRouteKind::Inspect => &HTTP_ROUTE_SPECS[3],
        HttpRouteKind::Receive => &HTTP_ROUTE_SPECS[4],
        HttpRouteKind::Doctor => &HTTP_ROUTE_SPECS[5],
        HttpRouteKind::RuntimeReload => &HTTP_ROUTE_SPECS[6],
        HttpRouteKind::Compatibility => &HTTP_ROUTE_SPECS[7],
        HttpRouteKind::Heartbeat => &HTTP_ROUTE_SPECS[8],
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
        RequestEnvelope::ReloadRuntimeView => HttpRouteKind::RuntimeReload,
        RequestEnvelope::CompatibilityPreflight(_) => HttpRouteKind::Compatibility,
        RequestEnvelope::Heartbeat(_) => HttpRouteKind::Heartbeat,
    }
}

fn route_kind_for_http(method: &str, path: &str) -> Option<HttpRouteKind> {
    HTTP_ROUTE_SPECS.iter().find_map(|spec| {
        (spec.route.method == method && spec.route.path_template == path).then_some(spec.kind)
    })
}

type EncodedHttpResponse = (u16, &'static str, Vec<u8>, Option<String>);

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

pub fn write_http_request(
    writer: &mut impl Write,
    request: &RequestEnvelope,
) -> Result<(), AtmError> {
    write_http_request_with_headers(writer, request, &[])
}

/// Serializes one route-specific HTTP request with adapter-owned headers.
pub fn write_http_request_with_headers(
    writer: &mut impl Write,
    request: &RequestEnvelope,
    headers: &[(&str, &str)],
) -> Result<(), AtmError> {
    write_http_request_with_headers_and_connection(writer, request, headers, false)
}

/// Serializes one route-specific HTTP request with caller-selected bounded
/// connection reuse.  The public default stays close-after-response; peer
/// delivery uses keep-alive only while emitting a supplied finite frame slice.
pub fn write_http_request_with_headers_and_connection(
    writer: &mut impl Write,
    request: &RequestEnvelope,
    headers: &[(&str, &str)],
    keep_alive: bool,
) -> Result<(), AtmError> {
    // The protocol envelope is an in-process dispatch type, never an HTTP
    // representation. Each route serializes its own OpenAPI request body.
    let body = encode_request_body(request)?;
    let (method, path) = endpoint_for(request);
    let headers = headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect::<String>();
    write!(
        writer,
        "{method} {path} HTTP/1.1\r\nContent-Type: application/json\r\n{headers}Content-Length: {}\r\nConnection: {}\r\n\r\n",
        body.len(),
        if keep_alive { "keep-alive" } else { "close" },
    )
    .map_err(|source| {
        AtmError::daemon_unavailable(format!("failed to write daemon HTTP request headers: {source}"))
    })?;
    writer.write_all(&body).map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to write daemon HTTP request body: {source}"
        ))
    })?;
    writer.flush().map_err(|source| {
        AtmError::daemon_unavailable(format!("failed to flush daemon HTTP request: {source}"))
    })
}

pub fn read_http_request(reader: &mut impl Read) -> Result<Option<HttpRequest>, AtmError> {
    HttpFrameReader::new().read_request(reader)
}

/// Writes an HTTP response with the supplied local connection policy.
///
/// Local adapters use this for their opt-in bounded keep-alive loop; the
/// public default remains [`write_http_response`], which closes the connection.
pub fn write_local_http_response(
    writer: &mut impl Write,
    response: &ResponseEnvelope,
    keep_alive: bool,
) -> Result<(), AtmError> {
    match response {
        ResponseEnvelope::Clear(outcome) => {
            write_no_content_response_with_connection(writer, outcome, keep_alive)
        }
        _ => write_http_response_body_with_connection(writer, response, keep_alive),
    }
}

pub fn decode_request(request: HttpRequest) -> Result<ApiRequest, AtmError> {
    if request.body.len() > MAX_HTTP_REQUEST_BODY_BYTES {
        return Err(AtmError::validation(
            "daemon HTTP request body exceeds 1048576 bytes",
        ));
    }
    decode_route_request(
        &request.method.to_ascii_uppercase(),
        &request.path,
        &request.body,
    )
}

pub fn write_http_response(
    writer: &mut impl Write,
    response: &ResponseEnvelope,
) -> Result<(), AtmError> {
    match response {
        ResponseEnvelope::Clear(outcome) => write_no_content_response(writer, outcome),
        _ => write_http_response_body(writer, response),
    }
}

fn write_http_response_body(
    writer: &mut impl Write,
    response: &ResponseEnvelope,
) -> Result<(), AtmError> {
    write_http_response_body_with_connection(writer, response, false)
}

fn write_http_response_body_with_connection(
    writer: &mut impl Write,
    response: &ResponseEnvelope,
    keep_alive: bool,
) -> Result<(), AtmError> {
    let (status, reason, body, location) = encode_response(response)?;
    let location = location
        .as_deref()
        .map(|value| format!("Location: {value}\r\n"))
        .unwrap_or_default();
    // One application write keeps a small local TCP response from becoming a
    // header packet followed by a body packet when TCP_NODELAY is enabled.
    // The framing remains identical for every local transport.
    let headers = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\n{location}Content-Length: {}\r\nConnection: {}\r\n\r\n",
        body.len(),
        if keep_alive { "keep-alive" } else { "close" },
    );
    let mut encoded = Vec::with_capacity(headers.len() + body.len());
    encoded.extend_from_slice(headers.as_bytes());
    encoded.extend_from_slice(&body);
    writer.write_all(&encoded).map_err(|source| {
        AtmError::daemon_unavailable(format!("failed to write daemon HTTP response: {source}"))
    })?;
    writer.flush().map_err(|source| {
        AtmError::daemon_unavailable(format!("failed to flush daemon HTTP response: {source}"))
    })
}

pub fn read_http_response(
    reader: &mut impl Read,
    request: &RequestEnvelope,
) -> Result<ResponseEnvelope, AtmError> {
    read_http_response_with_frame_reader(&mut HttpFrameReader::new(), reader, request)
}

/// Reads one HTTP response through a persistent bounded frame reader.
///
/// Keep `frames` for the lifetime of a connection when a caller may read more
/// than one response. This preserves coalesced bytes for the next response;
/// [`read_http_response`] remains the one-response compatibility wrapper.
pub fn read_http_response_with_frame_reader(
    frames: &mut HttpFrameReader,
    reader: &mut impl Read,
    request: &RequestEnvelope,
) -> Result<ResponseEnvelope, AtmError> {
    let Some(HttpResponseFrame {
        status_line,
        headers,
        body,
    }) = frames.read_response(reader)?
    else {
        return Err(AtmError::daemon_unavailable(
            "daemon closed HTTP connection before a response",
        ));
    };
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| {
            AtmError::validation_with_recovery(
                "daemon HTTP response status is malformed",
                "ensure the daemon returns an HTTP/1.1 status line with a numeric status code",
            )
        })?
        .parse::<u16>()
        .map_err(|_source| {
            AtmError::validation_with_recovery(
                "daemon HTTP response status is malformed",
                "ensure the daemon returns an HTTP/1.1 status line with a numeric status code",
            )
        })?;
    if status == 204 {
        return decode_no_content_response(request, &headers, &body);
    }
    if !(200..300).contains(&status) {
        return decode_response_body(&body, "error").map(ResponseEnvelope::Error);
    }
    decode_success_response(request, &body)
}

fn write_no_content_response(
    writer: &mut impl Write,
    outcome: &crate::clear::ClearOutcome,
) -> Result<(), AtmError> {
    write_no_content_response_with_connection(writer, outcome, false)
}

fn write_no_content_response_with_connection(
    writer: &mut impl Write,
    outcome: &crate::clear::ClearOutcome,
    keep_alive: bool,
) -> Result<(), AtmError> {
    let outcome = serde_json::to_vec(outcome).map_err(AtmError::from)?;
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(outcome);
    write!(
        writer,
        "HTTP/1.1 204 No Content\r\n{CLEAR_OUTCOME_HEADER}: {encoded}\r\nContent-Length: 0\r\nConnection: {}\r\n\r\n",
        if keep_alive { "keep-alive" } else { "close" },
    )
    .map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to write daemon HTTP no-content response: {source}"
        ))
    })?;
    writer.flush().map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to flush daemon HTTP no-content response: {source}"
        ))
    })
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
        .map_err(|_source| {
            AtmError::validation_with_recovery(
                "daemon HTTP clear outcome metadata is invalid",
                "ensure the daemon encodes valid clear outcome metadata in its HTTP 204 response",
            )
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
        RequestEnvelope::ReloadRuntimeView => serde_json::to_vec(&()),
    }
    .map_err(AtmError::from)
}

fn decode_route_request(method: &str, path: &str, body: &[u8]) -> Result<ApiRequest, AtmError> {
    let route = route_kind_for_http(method, path).ok_or_else(|| {
        AtmError::validation(format!("unsupported daemon HTTP route {method} {path}"))
    })?;
    match route {
        HttpRouteKind::Write => serde_json::from_slice(body)
            .map(|value| ApiRequest::Write(Box::new(value)))
            .map_err(|source| invalid_route_body("write", source)),
        HttpRouteKind::List => serde_json::from_slice(body)
            .map(|value| ApiRequest::Messages(Box::new(MessageCollectionRequest::List(value))))
            .map_err(|source| invalid_route_body("messages list", source)),
        HttpRouteKind::Inspect => serde_json::from_slice(body)
            .map(|value| ApiRequest::Messages(Box::new(MessageCollectionRequest::Peek(value))))
            .map_err(|source| invalid_route_body("message inspect", source)),
        HttpRouteKind::Receive => serde_json::from_slice(body)
            .map(|value| ApiRequest::Messages(Box::new(MessageCollectionRequest::Receive(value))))
            .map_err(|source| invalid_route_body("message read", source)),
        HttpRouteKind::Clear => serde_json::from_slice(body)
            .map(ApiRequest::Clear)
            .map_err(|source| invalid_route_body("messages clear", source)),
        HttpRouteKind::Doctor => serde_json::from_slice(body)
            .map(ApiRequest::Doctor)
            .map_err(|source| invalid_route_body("doctor", source)),
        HttpRouteKind::Compatibility => serde_json::from_slice(body)
            .map(ApiRequest::CompatibilityPreflight)
            .map_err(|source| invalid_route_body("compatibility", source)),
        HttpRouteKind::Heartbeat => serde_json::from_slice(body)
            .map(ApiRequest::Heartbeat)
            .map_err(|source| invalid_route_body("heartbeat", source)),
        HttpRouteKind::RuntimeReload => serde_json::from_slice::<()>(body)
            .map(|()| ApiRequest::ReloadRuntimeView)
            .map_err(|source| invalid_route_body("runtime reload", source)),
    }
}

fn invalid_route_body(what: &str, source: serde_json::Error) -> AtmError {
    AtmError::validation_with_recovery(
        format!("invalid {what} HTTP request body: {source}"),
        "ensure the client sends the documented JSON request body and retry",
    )
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
        RequestEnvelope::ReloadRuntimeView => decode_response_body::<()>(body, "runtime reload")
            .map(|()| ResponseEnvelope::RuntimeViewReloaded),
    }
}

fn encode_response(response: &ResponseEnvelope) -> Result<EncodedHttpResponse, AtmError> {
    let (status, reason, location, body) = match response {
        ResponseEnvelope::Send(crate::protocol::SendResponseEnvelope::Sent(value)) => (
            201,
            "Created",
            Some(format!("/v1/atm/message/{}", value.message_id)),
            serde_json::to_vec(value),
        ),
        ResponseEnvelope::Send(crate::protocol::SendResponseEnvelope::Acknowledged(value)) => (
            201,
            "Created",
            Some(format!("/v1/atm/message/{}", value.message_id)),
            serde_json::to_vec(value),
        ),
        ResponseEnvelope::CompatibilityVerdict(value) => {
            (200, "OK", None, serde_json::to_vec(value))
        }
        ResponseEnvelope::Heartbeat(value) => (200, "OK", None, serde_json::to_vec(value)),
        ResponseEnvelope::List(value) => (200, "OK", None, serde_json::to_vec(value)),
        ResponseEnvelope::Peek(value) => (200, "OK", None, serde_json::to_vec(value)),
        ResponseEnvelope::Receive(value) => (200, "OK", None, serde_json::to_vec(value)),
        ResponseEnvelope::Clear(_) => unreachable!("clear responses use HTTP 204 metadata"),
        ResponseEnvelope::Doctor(value) => (200, "OK", None, serde_json::to_vec(value)),
        ResponseEnvelope::RuntimeViewReloaded => (200, "OK", None, serde_json::to_vec(&())),
        ResponseEnvelope::Error(value) => {
            let status = if value.is_validation() { 400 } else { 503 };
            (
                status,
                if status == 400 {
                    "Bad Request"
                } else {
                    "Service Unavailable"
                },
                None,
                serde_json::to_vec(value),
            )
        }
    };
    body.map(|body| (status, reason, body, location))
        .map_err(AtmError::from)
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthenticatedIngress {
    Local,
    /// A configured peer HTTP frame. The application router receives no socket
    /// or peer configuration.
    Peer,
}

#[derive(Debug, Clone, Copy)]
pub struct RequestDeadline(Instant);

impl RequestDeadline {
    pub fn after(duration: Duration) -> Self {
        Self(Instant::now() + duration)
    }

    pub fn expired(self) -> bool {
        Instant::now() >= self.0
    }

    pub fn remaining(self) -> Option<Duration> {
        self.0.checked_duration_since(Instant::now())
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
pub trait DaemonApiClient: crate::boundary::sealed::Sealed + Send + Sync {
    /// Executes one API request through the configured transport adapter.
    fn execute(&self, request: ApiRequest) -> Result<ApiResponse, AtmError>;
}

#[cfg(test)]
mod tests {
    use std::io::{self, Read, Write};

    use base64::Engine;

    use super::{
        ApiRequest, HttpFrameReader, MAX_HTTP_HEADER_BYTES, MAX_HTTP_REQUEST_BODY_BYTES,
        decode_request, read_http_request, read_http_response, write_http_request,
        write_http_response,
    };
    use crate::ack::AckRequest;
    use crate::clear::{ClearOutcome, ClearQuery, RemovedByClass};
    use crate::doctor::DoctorQuery;
    use crate::error::AtmError;
    use crate::protocol::{RequestEnvelope, ResponseEnvelope};
    use crate::schema::AtmMessageId;
    use crate::send::{SendMessageSource, SendRequest};
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::types::CommandAction;

    /// A reader that presents a valid HTTP stream in deliberately small reads.
    ///
    /// TCP may split one HTTP frame at any byte boundary; adapters must rely on
    /// HTTP framing rather than a single socket read.
    struct FragmentedReader {
        bytes: Vec<u8>,
        position: usize,
        maximum_chunk: usize,
    }

    impl FragmentedReader {
        fn new(bytes: Vec<u8>, maximum_chunk: usize) -> Self {
            Self {
                bytes,
                position: 0,
                maximum_chunk,
            }
        }
    }

    impl Read for FragmentedReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.position == self.bytes.len() {
                return Ok(0);
            }
            let count = (self.bytes.len() - self.position)
                .min(self.maximum_chunk)
                .min(buffer.len());
            buffer[..count].copy_from_slice(&self.bytes[self.position..self.position + count]);
            self.position += count;
            Ok(count)
        }
    }

    #[derive(Default)]
    struct WriteCountingWriter {
        bytes: Vec<u8>,
        writes: usize,
    }

    impl Write for WriteCountingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.writes += 1;
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn http_uds_request_round_trip_preserves_the_canonical_envelope() {
        let request = RequestEnvelope::Doctor(DoctorQuery::default());
        let mut bytes = Vec::new();

        write_http_request(&mut bytes, &request).expect("write HTTP request");
        let decoded = decode_request(
            read_http_request(&mut bytes.as_slice())
                .expect("read HTTP request")
                .expect("request"),
        )
        .expect("decode HTTP request");

        assert!(matches!(decoded, ApiRequest::Doctor(_)));
    }

    #[test]
    fn http_request_parser_accepts_a_request_fragmented_at_every_transport_read() {
        let request = RequestEnvelope::Doctor(DoctorQuery::default());
        let mut wire = Vec::new();
        write_http_request(&mut wire, &request).expect("write HTTP request");

        for mut frames in [HttpFrameReader::new(), HttpFrameReader::scalar_for_test()] {
            let mut fragmented = FragmentedReader::new(wire.clone(), 1);
            let decoded = decode_request(
                frames
                    .read_request(&mut fragmented)
                    .expect("read fragmented HTTP request")
                    .expect("request"),
            )
            .expect("decode fragmented HTTP request");

            assert!(matches!(decoded, ApiRequest::Doctor(_)));
        }
    }

    #[test]
    fn http_request_parser_handles_coalesced_runs_of_consecutive_frames() {
        let request = RequestEnvelope::Doctor(DoctorQuery::default());

        for frame_count in [1_usize, 2, 4, 8, 16, 32, 64] {
            let mut wire = Vec::new();
            for _ in 0..frame_count {
                write_http_request(&mut wire, &request).expect("write coalesced HTTP request");
            }
            for mut frames in [HttpFrameReader::new(), HttpFrameReader::scalar_for_test()] {
                let mut coalesced = wire.as_slice();
                for frame_index in 0..frame_count {
                    let decoded = decode_request(
                        frames
                            .read_request(&mut coalesced)
                            .expect("read coalesced HTTP request")
                            .unwrap_or_else(|| {
                                panic!(
                                    "coalesced run of {frame_count} frames ended before frame {}",
                                    frame_index + 1
                                )
                            }),
                    )
                    .expect("decode coalesced request");
                    assert!(matches!(decoded, ApiRequest::Doctor(_)));
                }
                assert!(
                    coalesced.is_empty(),
                    "coalesced run of {frame_count} complete frames left trailing bytes"
                );
            }
        }
    }

    #[test]
    fn http_frame_reader_retains_bytes_after_a_body_boundary() {
        let wire = b"POST /v1/atm/messages HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}GET /v1/atm/doctor HTTP/1.1\r\nContent-Length: 0\r\n\r\n";
        for mut frames in [HttpFrameReader::new(), HttpFrameReader::scalar_for_test()] {
            let mut source = FragmentedReader::new(wire.to_vec(), wire.len());
            let first = frames
                .read_request(&mut source)
                .expect("read first frame")
                .expect("first frame");
            let second = frames
                .read_request(&mut source)
                .expect("read second frame")
                .expect("second frame");

            assert_eq!(first.body, b"{}");
            assert_eq!(second.method, "GET");
            assert_eq!(second.path, "/v1/atm/doctor");
        }
    }

    #[test]
    fn http_frame_reader_consumes_a_complete_buffered_follow_up_without_reading() {
        let wire = b"GET /v1/atm/doctor HTTP/1.1\r\nContent-Length: 0\r\n\r\nGET /v1/atm/doctor HTTP/1.1\r\nContent-Length: 0\r\n\r\n";
        let mut frames = HttpFrameReader::new();
        let mut source = wire.as_slice();

        let first = frames
            .read_request(&mut source)
            .expect("read first frame")
            .expect("first frame");
        let second = frames
            .read_buffered_request()
            .expect("read buffered frame")
            .expect("buffered frame");

        assert_eq!(first.path, "/v1/atm/doctor");
        assert_eq!(second.path, "/v1/atm/doctor");
        assert!(source.is_empty());
    }

    #[test]
    fn http_response_frame_reader_accepts_delimiter_and_body_fragmentation() {
        let wire = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";

        for mut frames in [HttpFrameReader::new(), HttpFrameReader::scalar_for_test()] {
            let mut source = FragmentedReader::new(wire.to_vec(), 1);
            let response = frames
                .read_response(&mut source)
                .expect("read fragmented HTTP response")
                .expect("response");

            assert_eq!(response.status_line, "HTTP/1.1 200 OK");
            assert_eq!(response.body, b"{}");
        }
    }

    #[test]
    fn public_http_response_reader_uses_bounded_fragmented_framing() {
        let request = RequestEnvelope::Doctor(DoctorQuery::default());
        let mut wire = Vec::new();
        write_http_response(
            &mut wire,
            &ResponseEnvelope::Error(AtmError::validation("fragmented")),
        )
        .expect("write response");
        let mut source = FragmentedReader::new(wire, 1);

        let response = read_http_response(&mut source, &request)
            .expect("bounded public response reader must decode fragmented response");

        assert!(matches!(response, ResponseEnvelope::Error(error) if error.is_validation()));
    }

    #[test]
    fn http_response_frame_reader_retains_coalesced_follow_up() {
        let wire = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}HTTP/1.1 503 Service Unavailable\r\nContent-Length: 2\r\n\r\n[]";

        for mut frames in [HttpFrameReader::new(), HttpFrameReader::scalar_for_test()] {
            let mut source = FragmentedReader::new(wire.to_vec(), wire.len());
            let first = frames
                .read_response(&mut source)
                .expect("read first HTTP response")
                .expect("first response");
            let second = frames
                .read_buffered_response()
                .expect("read buffered HTTP response")
                .expect("second response");

            assert_eq!(first.body, b"{}");
            assert_eq!(second.status_line, "HTTP/1.1 503 Service Unavailable");
            assert_eq!(second.body, b"[]");
            assert_eq!(source.position, source.bytes.len());
        }
    }

    #[test]
    fn http_response_frame_reader_rejects_ambiguous_or_oversized_frames() {
        let duplicate = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n";
        let duplicate_error = HttpFrameReader::new()
            .read_response(&mut duplicate.as_slice())
            .expect_err("duplicate response Content-Length must fail");
        assert!(duplicate_error.is_validation());
        assert!(
            duplicate_error
                .message()
                .contains("duplicate Content-Length")
        );

        let oversized = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
            MAX_HTTP_REQUEST_BODY_BYTES + 1
        );
        let mut oversized_source = oversized.as_bytes();
        let oversized_error = HttpFrameReader::new()
            .read_response(&mut oversized_source)
            .expect_err("oversized response body must fail");
        assert!(oversized_error.is_validation());
        assert!(oversized_error.message().contains("body exceeds"));
    }

    #[test]
    fn http_response_frame_reader_errors_include_recovery_guidance() {
        let invalid_headers = b"HTTP/1.1 200 OK\r\nX-Test: \xff\r\n\r\n";
        let duplicate_length = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n";
        let invalid_length = b"HTTP/1.1 200 OK\r\nContent-Length: nope\r\n\r\n";
        let oversized_headers = format!(
            "HTTP/1.1 200 OK\r\nX-Test: {}\r\n\r\n",
            "x".repeat(MAX_HTTP_HEADER_BYTES)
        );
        let oversized_body = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
            MAX_HTTP_REQUEST_BODY_BYTES + 1
        );

        for wire in [
            invalid_headers.as_slice(),
            duplicate_length.as_slice(),
            invalid_length.as_slice(),
            oversized_headers.as_bytes(),
            oversized_body.as_bytes(),
        ] {
            let mut source = wire;
            let error = HttpFrameReader::new()
                .read_response(&mut source)
                .expect_err("malformed response must fail with recovery guidance");
            assert!(error.is_validation());
            assert!(
                error.message().contains("Recovery:"),
                "response framing error must explain recovery: {error:?}"
            );
        }
    }

    #[test]
    fn response_decoding_errors_include_recovery_guidance() {
        let clear = RequestEnvelope::Clear(ClearQuery {
            home_dir: std::env::temp_dir(),
            current_dir: std::env::temp_dir(),
            caller_identity: TEST_SENDER.parse().expect("caller"),
            caller_team: TEST_TEAM.parse().expect("team"),
            older_than: None,
            idle_only: false,
            dry_run: false,
        });
        let doctor = RequestEnvelope::Doctor(DoctorQuery::default());
        let valid_outcome = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&ClearOutcome {
                action: CommandAction::Clear,
                team: TEST_TEAM.parse().expect("team"),
                agent: TEST_SENDER.parse().expect("agent"),
                removed_total: 0,
                remaining_total: 0,
                removed_by_class: RemovedByClass::default(),
            })
            .expect("serialize outcome"),
        );
        let cases = [
            (
                b"HTTP/1.1 OK\r\nContent-Length: 0\r\n\r\n".as_slice(),
                &doctor,
            ),
            (
                b"HTTP/1.1 nope OK\r\nContent-Length: 0\r\n\r\n".as_slice(),
                &doctor,
            ),
            (
                b"HTTP/1.1 204 No Content\r\nContent-Length: 1\r\n\r\nx".as_slice(),
                &clear,
            ),
            (
                b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".as_slice(),
                &doctor,
            ),
        ];

        for (mut wire, request) in cases {
            let error = read_http_response(&mut wire, request)
                .expect_err("malformed response must include recovery guidance");
            assert!(error.is_validation());
            assert!(error.message().contains("Recovery:"), "{error:?}");
        }

        for (mut wire, request) in [
            (
                b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\nx".as_slice(),
                &doctor,
            ),
            (
                b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 1\r\n\r\nx".as_slice(),
                &doctor,
            ),
        ] {
            let error = read_http_response(&mut wire, request)
                .expect_err("invalid JSON response must include recovery guidance");
            assert!(error.is_validation());
            assert!(error.message().contains("Recovery:"), "{error:?}");
        }

        for mut wire in [
            b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".as_slice(),
            b"HTTP/1.1 204 No Content\r\nX-ATM-Clear-Outcome: invalid!\r\nContent-Length: 0\r\n\r\n".as_slice(),
        ] {
            let error = read_http_response(&mut wire, &clear)
                .expect_err("malformed clear response must include recovery guidance");
            assert!(error.is_validation());
            assert!(error.message().contains("Recovery:"), "{error:?}");
        }

        let wire = format!(
            "HTTP/1.1 204 No Content\r\nX-ATM-Clear-Outcome: {valid_outcome}\r\nContent-Length: 0\r\n\r\n"
        );
        assert!(read_http_response(&mut wire.as_bytes(), &clear).is_ok());
    }

    #[test]
    fn http_frame_reader_rejects_duplicate_content_length() {
        let wire =
            b"POST /v1/atm/messages HTTP/1.1\r\nContent-Length: 2\r\nContent-Length: 2\r\n\r\n{}";
        let error = HttpFrameReader::new()
            .read_request(&mut wire.as_slice())
            .expect_err("duplicate Content-Length must be rejected");

        assert!(error.is_validation());
        assert!(error.message().contains("duplicate Content-Length"));
    }

    #[test]
    fn http_wire_body_is_route_schema_not_request_envelope() {
        let request = RequestEnvelope::Doctor(DoctorQuery::default());
        let mut bytes = Vec::new();

        write_http_request(&mut bytes, &request).expect("write HTTP request");

        let text = String::from_utf8(bytes).expect("HTTP UTF-8");
        assert!(text.starts_with("GET /v1/atm/doctor HTTP/1.1"));
        assert!(!text.contains("Doctor"));
        assert!(!text.contains("RequestEnvelope"));
    }

    #[test]
    fn http_error_is_direct_error_body_with_non_success_status() {
        let request = RequestEnvelope::Doctor(DoctorQuery::default());
        let mut bytes = Vec::new();

        write_http_response(
            &mut bytes,
            &ResponseEnvelope::Error(AtmError::validation("bad")),
        )
        .expect("write HTTP error");

        let text = String::from_utf8(bytes.clone()).expect("HTTP UTF-8");
        assert!(text.starts_with("HTTP/1.1 400 Bad Request"));
        assert!(!text.contains("Error\":{") && !text.contains("Error\""));
        let response = read_http_response(&mut bytes.as_slice(), &request).expect("read error");
        assert!(matches!(response, ResponseEnvelope::Error(error) if error.is_validation()));
    }

    #[test]
    fn http_response_writes_headers_and_body_as_one_transport_frame() {
        let mut writer = WriteCountingWriter::default();

        write_http_response(
            &mut writer,
            &ResponseEnvelope::Error(AtmError::validation("bad")),
        )
        .expect("write HTTP error");

        assert_eq!(
            writer.writes, 1,
            "response must not split header and body writes"
        );
        assert!(writer.bytes.starts_with(b"HTTP/1.1 400 Bad Request"));
    }

    #[test]
    fn http_response_remains_explicitly_connection_close_until_keep_alive_is_introduced() {
        let mut bytes = Vec::new();

        write_http_response(
            &mut bytes,
            &ResponseEnvelope::Error(AtmError::validation("bad")),
        )
        .expect("write HTTP error");

        let headers = std::str::from_utf8(&bytes)
            .expect("HTTP response is UTF-8")
            .split("\r\n\r\n")
            .next()
            .expect("response headers");
        assert!(
            headers
                .lines()
                .any(|line| line.eq_ignore_ascii_case("Connection: close"))
        );
    }

    #[test]
    fn http_clear_uses_no_content_with_outcome_metadata() {
        let request = RequestEnvelope::Clear(ClearQuery {
            home_dir: std::env::temp_dir(),
            current_dir: std::env::temp_dir(),
            caller_identity: TEST_SENDER.parse().expect("caller"),
            caller_team: TEST_TEAM.parse().expect("team"),
            older_than: None,
            idle_only: false,
            dry_run: false,
        });
        let response = ResponseEnvelope::Clear(ClearOutcome {
            action: CommandAction::Clear,
            team: TEST_TEAM.parse().expect("team"),
            agent: TEST_SENDER.parse().expect("agent"),
            removed_total: 1,
            remaining_total: 2,
            removed_by_class: RemovedByClass {
                acknowledged: 1,
                read: 0,
            },
        });
        let mut bytes = Vec::new();

        write_http_response(&mut bytes, &response).expect("write clear response");

        let text = String::from_utf8(bytes.clone()).expect("HTTP UTF-8");
        assert!(text.starts_with("HTTP/1.1 204 No Content"));
        assert!(text.contains("X-ATM-Clear-Outcome:"));
        assert!(text.ends_with("\r\n\r\n"));
        let decoded = read_http_response(&mut bytes.as_slice(), &request).expect("read clear");
        assert!(matches!(decoded, ResponseEnvelope::Clear(outcome) if outcome.removed_total == 1));
    }

    #[test]
    fn normal_send_and_ack_share_one_http_write_resource() {
        let send = RequestEnvelope::Write(Box::new(
            SendRequest::new(
                std::env::temp_dir(),
                std::env::temp_dir(),
                TEST_SENDER.parse().expect("sender"),
                "receiver",
                TEST_TEAM.parse().expect("team"),
                SendMessageSource::Inline("hello".to_string()),
                None,
                false,
                None,
                false,
            )
            .expect("send request"),
        ));
        let ack = RequestEnvelope::Write(Box::new(
            AckRequest {
                home_dir: std::env::temp_dir(),
                current_dir: std::env::temp_dir(),
                caller_identity: TEST_SENDER.parse().expect("sender"),
                caller_chat_id: None,
                caller_team: TEST_TEAM.parse().expect("team"),
                message_id: AtmMessageId::new(),
                reply_body: "acknowledged".to_string(),
            }
            .into_write_request(),
        ));

        for (request, is_ack) in [(send, false), (ack, true)] {
            let mut bytes = Vec::new();
            write_http_request(&mut bytes, &request).expect("write HTTP request");
            let raw = String::from_utf8(bytes.clone()).expect("HTTP UTF-8");
            assert!(raw.starts_with("POST /v1/atm/messages HTTP/1.1"));
            let decoded = decode_request(
                read_http_request(&mut bytes.as_slice())
                    .expect("read HTTP request")
                    .expect("request"),
            )
            .expect("decode HTTP request");
            assert!(matches!(
                decoded,
                ApiRequest::Write(request) if request.acknowledges_message_id.is_some() == is_ack
            ));
        }
    }

    #[test]
    fn http_decode_rejects_body_kind_that_does_not_match_route() {
        let request = RequestEnvelope::Doctor(DoctorQuery::default());
        let body = serde_json::to_vec(&request).expect("body");
        let raw = format!(
            "POST /v1/atm/messages HTTP/1.1\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            String::from_utf8(body).expect("utf8")
        );

        let decoded = decode_request(
            read_http_request(&mut raw.as_bytes())
                .expect("read HTTP request")
                .expect("request"),
        );

        let error = decoded.expect_err("route mismatch");
        assert!(error.is_validation());
        assert!(error.message().contains("Recovery:"), "{error:?}");
    }

    #[test]
    fn declared_oversized_http_body_is_rejected_before_decode() {
        let request = format!(
            "POST /v1/atm/messages HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_HTTP_REQUEST_BODY_BYTES + 1
        );

        let error = read_http_request(&mut request.as_bytes()).expect_err("oversized body");

        assert!(error.is_validation());
    }
}
