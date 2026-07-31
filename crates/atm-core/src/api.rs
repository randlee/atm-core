//! Transport-neutral API request and response contracts.
//!
//! UDS and future HTTPS adapters translate HTTP into this surface.  The
//! application router receives no socket, storage, or nudge capability.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use crate::clear::ClearQuery;
use crate::doctor::DoctorQuery;
use crate::error::AtmError;
use crate::list::ListQuery;
use crate::protocol::{
    CompatibilityPreflight, PeerSyncRequest, RequestEnvelope, ResponseEnvelope,
    TeamMemberHeartbeatRequest,
};
use crate::read::{PeekQuery, ReadQuery};
use crate::send::WriteRequest;
use crate::types::HostName;
use base64::Engine as _;

pub const MAX_HTTP_REQUEST_BODY_BYTES: usize = 1_048_576;
/// Version of the daemon's HTTP request contract.
pub const HTTP_API_VERSION: &str = crate::protocol::HTTP_API_VERSION;
const MAX_HTTP_HEADER_BYTES: usize = 16 * 1024;
const CLEAR_OUTCOME_HEADER: &str = "X-ATM-Clear-Outcome";
const MESSAGES_PATH: &str = "/v1/atm/messages";
const INSPECT_PATH: &str = "/v1/atm/messages/inspect";
const READ_PATH: &str = "/v1/atm/messages/read";
const DOCTOR_PATH: &str = "/v1/atm/doctor";
const PEER_SYNC_PREFIX: &str = "/v1/atm/peers/";
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
    PeerSync,
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
        kind: HttpRouteKind::PeerSync,
        route: HttpRoute {
            method: "POST",
            path_template: "/v1/atm/peers/{peer}/sync",
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
        HttpRouteKind::PeerSync => &HTTP_ROUTE_SPECS[6],
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
        RequestEnvelope::PeerSync(_) => HttpRouteKind::PeerSync,
        RequestEnvelope::ReloadRuntimeView => HttpRouteKind::RuntimeReload,
        RequestEnvelope::CompatibilityPreflight(_) => HttpRouteKind::Compatibility,
        RequestEnvelope::Heartbeat(_) => HttpRouteKind::Heartbeat,
    }
}

fn route_kind_for_http(method: &str, path: &str) -> Option<HttpRouteKind> {
    HTTP_ROUTE_SPECS.iter().find_map(|spec| {
        (spec.route.method == method
            && (spec.route.path_template == path
                || (spec.kind == HttpRouteKind::PeerSync && peer_sync_path_host(path).is_some())))
        .then_some(spec.kind)
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
    let path = match request {
        RequestEnvelope::PeerSync(request) => format!("{PEER_SYNC_PREFIX}{}/sync", request.peer),
        _ => spec.route.path_template.to_string(),
    };
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
        "{method} {path} HTTP/1.1\r\nContent-Type: application/json\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
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
    let Some((start_line, headers)) = read_http_headers(reader)? else {
        return Ok(None);
    };
    let mut parts = start_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| AtmError::validation("malformed daemon HTTP request method"))?;
    let path = parts
        .next()
        .ok_or_else(|| AtmError::validation("malformed daemon HTTP request path"))?;
    if parts.next().is_none() {
        return Err(AtmError::validation(
            "malformed daemon HTTP request version",
        ));
    }
    let body = read_http_body(reader, &headers)?;
    Ok(Some(HttpRequest {
        method: method.to_string(),
        path: path.to_string(),
        headers,
        body,
    }))
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
    let (status, reason, body, location) = encode_response(response)?;
    let location = location
        .as_deref()
        .map(|value| format!("Location: {value}\r\n"))
        .unwrap_or_default();
    write!(
        writer,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\n{location}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len(),
    )
    .map_err(|source| {
        AtmError::daemon_unavailable(format!("failed to write daemon HTTP response headers: {source}"))
    })?;
    writer.write_all(&body).map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to write daemon HTTP response body: {source}"
        ))
    })?;
    writer.flush().map_err(|source| {
        AtmError::daemon_unavailable(format!("failed to flush daemon HTTP response: {source}"))
    })
}

pub fn read_http_response(
    reader: &mut impl Read,
    request: &RequestEnvelope,
) -> Result<ResponseEnvelope, AtmError> {
    let Some((status_line, headers)) = read_http_headers(reader)? else {
        return Err(AtmError::daemon_unavailable(
            "daemon closed HTTP connection before a response",
        ));
    };
    let body = read_http_body(reader, &headers)?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| AtmError::validation("daemon HTTP response status is malformed"))?
        .parse::<u16>()
        .map_err(|_source| AtmError::validation("daemon HTTP response status is malformed"))?;
    if status == 204 {
        return decode_no_content_response(request, &headers, &body);
    }
    if !(200..300).contains(&status) {
        return serde_json::from_slice(&body)
            .map(ResponseEnvelope::Error)
            .map_err(AtmError::from);
    }
    decode_success_response(request, &body)
}

fn write_no_content_response(
    writer: &mut impl Write,
    outcome: &crate::clear::ClearOutcome,
) -> Result<(), AtmError> {
    let outcome = serde_json::to_vec(outcome).map_err(AtmError::from)?;
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(outcome);
    write!(
        writer,
        "HTTP/1.1 204 No Content\r\n{CLEAR_OUTCOME_HEADER}: {encoded}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
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
        return Err(AtmError::validation(
            "daemon returned a body with an HTTP 204 response",
        ));
    }
    let RequestEnvelope::Clear(_) = request else {
        return Err(AtmError::validation(
            "daemon returned HTTP 204 for a request that does not clear messages",
        ));
    };
    let encoded = http_header(headers, CLEAR_OUTCOME_HEADER).ok_or_else(|| {
        AtmError::validation("daemon HTTP 204 response is missing clear outcome metadata")
    })?;
    let outcome = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_source| AtmError::validation("daemon HTTP clear outcome metadata is invalid"))?;
    serde_json::from_slice(&outcome)
        .map(ResponseEnvelope::Clear)
        .map_err(AtmError::from)
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
        RequestEnvelope::PeerSync(value) => serde_json::to_vec(value),
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
        HttpRouteKind::PeerSync => {
            let request: PeerSyncRequest = serde_json::from_slice(body)
                .map_err(|source| invalid_route_body("peer sync", source))?;
            if peer_sync_path_host(path) != Some(request.peer.as_str()) {
                return Err(AtmError::validation(
                    "peer sync request body does not match its target peer path",
                ));
            }
            Ok(ApiRequest::PeerSync(request))
        }
        HttpRouteKind::RuntimeReload => serde_json::from_slice::<()>(body)
            .map(|()| ApiRequest::ReloadRuntimeView)
            .map_err(|source| invalid_route_body("runtime reload", source)),
    }
}

fn invalid_route_body(what: &str, source: serde_json::Error) -> AtmError {
    AtmError::validation(format!("invalid {what} HTTP request body: {source}"))
}

fn decode_success_response(
    request: &RequestEnvelope,
    body: &[u8],
) -> Result<ResponseEnvelope, AtmError> {
    match request {
        RequestEnvelope::Write(request) if request.acknowledges_message_id.is_some() => {
            serde_json::from_slice(body)
                .map(|value| {
                    ResponseEnvelope::Send(crate::protocol::SendResponseEnvelope::Acknowledged(
                        value,
                    ))
                })
                .map_err(AtmError::from)
        }
        RequestEnvelope::Write(_) => serde_json::from_slice(body)
            .map(|value| ResponseEnvelope::Send(crate::protocol::SendResponseEnvelope::Sent(value)))
            .map_err(AtmError::from),
        RequestEnvelope::CompatibilityPreflight(_) => serde_json::from_slice(body)
            .map(ResponseEnvelope::CompatibilityVerdict)
            .map_err(AtmError::from),
        RequestEnvelope::Heartbeat(_) => serde_json::from_slice(body)
            .map(ResponseEnvelope::Heartbeat)
            .map_err(AtmError::from),
        RequestEnvelope::List(_) => serde_json::from_slice(body)
            .map(ResponseEnvelope::List)
            .map_err(AtmError::from),
        RequestEnvelope::Peek(_) => serde_json::from_slice(body)
            .map(|value| ResponseEnvelope::Peek(Box::new(value)))
            .map_err(AtmError::from),
        RequestEnvelope::Receive(_) => serde_json::from_slice(body)
            .map(|value| ResponseEnvelope::Receive(Box::new(value)))
            .map_err(AtmError::from),
        RequestEnvelope::Clear(_) => serde_json::from_slice(body)
            .map(ResponseEnvelope::Clear)
            .map_err(AtmError::from),
        RequestEnvelope::Doctor(_) => serde_json::from_slice(body)
            .map(|value| ResponseEnvelope::Doctor(Box::new(value)))
            .map_err(AtmError::from),
        RequestEnvelope::PeerSync(_) => serde_json::from_slice(body)
            .map(ResponseEnvelope::PeerSync)
            .map_err(AtmError::from),
        RequestEnvelope::ReloadRuntimeView => serde_json::from_slice::<()>(body)
            .map(|()| ResponseEnvelope::RuntimeViewReloaded)
            .map_err(AtmError::from),
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
        ResponseEnvelope::PeerSync(value) => (200, "OK", None, serde_json::to_vec(value)),
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

fn peer_sync_path_host(path: &str) -> Option<&str> {
    path.strip_prefix("/v1/atm/peers/").and_then(|suffix| {
        suffix
            .strip_suffix("/sync")
            .filter(|peer| !peer.is_empty() && !peer.contains('/'))
    })
}

fn http_header<'a>(headers: &'a [String], name: &str) -> Option<&'a str> {
    headers.iter().find_map(|header| {
        header
            .split_once(':')
            .filter(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.trim())
    })
}

fn read_http_headers(reader: &mut impl Read) -> Result<Option<(String, Vec<String>)>, AtmError> {
    let mut bytes = Vec::new();
    let mut one = [0_u8; 1];
    loop {
        match reader.read(&mut one) {
            Ok(0) if bytes.is_empty() => return Ok(None),
            Ok(0) => {
                return Err(AtmError::daemon_unavailable(
                    "daemon HTTP headers ended unexpectedly",
                ));
            }
            Ok(_) => {
                bytes.push(one[0]);
                if bytes.len() > MAX_HTTP_HEADER_BYTES {
                    return Err(AtmError::validation(
                        "daemon HTTP headers exceed 16384 bytes",
                    ));
                }
                if bytes.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            Err(source) => {
                return Err(AtmError::daemon_unavailable(format!(
                    "failed to read daemon HTTP headers: {source}",
                )));
            }
        }
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_source| AtmError::validation("daemon HTTP headers are not UTF-8"))?;
    let mut lines = text.split("\r\n");
    let start_line = lines.next().unwrap_or_default().to_string();
    Ok(Some((
        start_line,
        lines
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect(),
    )))
}

fn read_http_body(reader: &mut impl Read, headers: &[String]) -> Result<Vec<u8>, AtmError> {
    let length = headers
        .iter()
        .find_map(|header| {
            header
                .split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        })
        .map(|(_, value)| value.trim().parse::<usize>())
        .transpose()
        .map_err(|_source| AtmError::validation("daemon HTTP Content-Length is invalid"))?
        .unwrap_or(0);
    if length > MAX_HTTP_REQUEST_BODY_BYTES {
        return Err(AtmError::validation(
            "daemon HTTP body exceeds 1048576 bytes",
        ));
    }
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body).map_err(|source| {
        AtmError::daemon_unavailable(format!("failed to read daemon HTTP body: {source}"))
    })?;
    Ok(body)
}

#[derive(Debug, Clone)]
pub enum ApiRequest {
    Messages(Box<MessageCollectionRequest>),
    Write(Box<WriteRequest>),
    Clear(ClearQuery),
    Doctor(DoctorQuery),
    CompatibilityPreflight(CompatibilityPreflight),
    Heartbeat(TeamMemberHeartbeatRequest),
    PeerSync(PeerSyncRequest),
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
            Self::PeerSync(request) => RequestEnvelope::PeerSync(request),
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
            RequestEnvelope::PeerSync(request) => Self::PeerSync(request),
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
    use std::io::{self, Read};

    use super::{
        ApiRequest, MAX_HTTP_REQUEST_BODY_BYTES, decode_request, read_http_request,
        read_http_response, write_http_request, write_http_response,
    };
    use crate::ack::AckRequest;
    use crate::clear::{ClearOutcome, ClearQuery, RemovedByClass};
    use crate::doctor::DoctorQuery;
    use crate::error::AtmError;
    use crate::protocol::{PeerSyncRequest, RequestEnvelope, ResponseEnvelope};
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
        let mut fragmented = FragmentedReader::new(wire, 1);

        let decoded = decode_request(
            read_http_request(&mut fragmented)
                .expect("read fragmented HTTP request")
                .expect("request"),
        )
        .expect("decode fragmented HTTP request");

        assert!(matches!(decoded, ApiRequest::Doctor(_)));
    }

    #[test]
    fn http_request_parser_leaves_a_coalesced_following_request_for_the_next_read() {
        let first = RequestEnvelope::Doctor(DoctorQuery::default());
        let second = RequestEnvelope::Doctor(DoctorQuery::default());
        let mut wire = Vec::new();
        write_http_request(&mut wire, &first).expect("write first HTTP request");
        write_http_request(&mut wire, &second).expect("write second HTTP request");
        let mut coalesced = wire.as_slice();

        let first = decode_request(
            read_http_request(&mut coalesced)
                .expect("read first coalesced HTTP request")
                .expect("first request"),
        )
        .expect("decode first request");
        let second = decode_request(
            read_http_request(&mut coalesced)
                .expect("read second coalesced HTTP request")
                .expect("second request"),
        )
        .expect("decode second request");

        assert!(matches!(first, ApiRequest::Doctor(_)));
        assert!(matches!(second, ApiRequest::Doctor(_)));
        assert!(
            coalesced.is_empty(),
            "both complete HTTP frames were consumed"
        );
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
    fn peer_sync_uses_the_peer_scoped_http_route_and_rejects_path_body_mismatch() {
        let request = RequestEnvelope::PeerSync(PeerSyncRequest {
            peer: "peer.example.test".parse().expect("peer host"),
        });
        let mut bytes = Vec::new();

        write_http_request(&mut bytes, &request).expect("write peer sync request");
        let raw = String::from_utf8(bytes.clone()).expect("HTTP UTF-8");
        assert!(raw.starts_with("POST /v1/atm/peers/peer.example.test/sync HTTP/1.1"));
        let decoded = decode_request(
            read_http_request(&mut bytes.as_slice())
                .expect("read HTTP request")
                .expect("request"),
        )
        .expect("decode peer sync request");
        assert!(
            matches!(decoded, ApiRequest::PeerSync(request) if request.peer.as_str() == "peer.example.test")
        );

        let mismatched = raw.replace("peer.example.test/sync", "other.example.test/sync");
        let error = decode_request(
            read_http_request(&mut mismatched.as_bytes())
                .expect("read mismatch request")
                .expect("request"),
        )
        .expect_err("path and body must name the same peer");
        assert!(error.is_validation());
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

        assert!(decoded.expect_err("route mismatch").is_validation());
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
