//! Transport-neutral API request and response contracts.
//!
//! UDS and future HTTPS adapters translate HTTP into this surface.  The
//! application router receives no socket, storage, or nudge capability.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use crate::ack::AckRequest;
use crate::clear::ClearQuery;
use crate::doctor::DoctorQuery;
use crate::error::AtmError;
use crate::list::ListQuery;
use crate::protocol::{
    CompatibilityPreflight, RequestEnvelope, ResponseEnvelope, TeamMemberHeartbeatRequest,
};
use crate::read::{PeekQuery, ReadQuery};
use crate::send::WriteRequest;

pub const MAX_HTTP_REQUEST_BODY_BYTES: usize = 1_048_576;
/// Version of the daemon's HTTP request contract.
pub const HTTP_API_VERSION: u16 = 1;
const MAX_HTTP_HEADER_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    pub body: Vec<u8>,
}

pub fn endpoint_for(request: &RequestEnvelope) -> (&'static str, String) {
    match request {
        RequestEnvelope::Write(request) => match request.acknowledges_message_id {
            Some(message_id) => ("POST", format!("/v1/atm/message/{message_id}/ack")),
            None => ("POST", "/v1/atm/messages".to_string()),
        },
        RequestEnvelope::List(_) | RequestEnvelope::Peek(_) | RequestEnvelope::Receive(_) => {
            ("GET", "/v1/atm/messages".to_string())
        }
        RequestEnvelope::Clear(_) => ("DELETE", "/v1/atm/messages".to_string()),
        RequestEnvelope::Doctor(_) => ("GET", "/v1/atm/doctor".to_string()),
        RequestEnvelope::CompatibilityPreflight(_) => ("POST", "/v1/atm/compatibility".to_string()),
        RequestEnvelope::Heartbeat(_) => ("POST", "/v1/atm/heartbeat".to_string()),
    }
}

pub fn write_http_request(
    writer: &mut impl Write,
    request: &RequestEnvelope,
) -> Result<(), AtmError> {
    let body = serde_json::to_vec(request).map_err(AtmError::from)?;
    let (method, path) = endpoint_for(request);
    write!(
        writer,
        "{method} {path} HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
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
        body,
    }))
}

pub fn decode_request(request: HttpRequest) -> Result<ApiRequest, AtmError> {
    if request.body.len() > MAX_HTTP_REQUEST_BODY_BYTES {
        return Err(AtmError::validation(
            "daemon HTTP request body exceeds 1048576 bytes",
        ));
    }
    let method = request.method.to_ascii_uppercase();
    if route_is_teams(&method, &request.path) {
        return Ok(ApiRequest::Teams(TeamRequest {
            method,
            path: request.path,
        }));
    }
    let envelope: RequestEnvelope =
        serde_json::from_slice(&request.body).map_err(AtmError::from)?;
    ApiRequest::from_http_parts(method.as_str(), request.path.as_str(), envelope)
}

pub fn write_http_response(
    writer: &mut impl Write,
    response: &ResponseEnvelope,
) -> Result<(), AtmError> {
    let body = serde_json::to_vec(response).map_err(AtmError::from)?;
    write!(
        writer,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
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

pub fn read_http_response(reader: &mut impl Read) -> Result<ResponseEnvelope, AtmError> {
    let Some((status_line, headers)) = read_http_headers(reader)? else {
        return Err(AtmError::daemon_unavailable(
            "daemon closed HTTP connection before a response",
        ));
    };
    if !status_line.starts_with("HTTP/1.1 2") {
        return Err(AtmError::daemon_unavailable(format!(
            "daemon returned HTTP status `{status_line}`"
        )));
    }
    let body = read_http_body(reader, &headers)?;
    serde_json::from_slice(&body).map_err(AtmError::from)
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
    Message(MessageRequest),
    Clear(ClearQuery),
    Doctor(DoctorQuery),
    Teams(TeamRequest),
    CompatibilityPreflight(CompatibilityPreflight),
    Heartbeat(TeamMemberHeartbeatRequest),
}

#[derive(Debug, Clone)]
pub enum MessageCollectionRequest {
    List(ListQuery),
    Peek(PeekQuery),
    Receive(ReadQuery),
}

#[derive(Debug, Clone)]
pub enum MessageRequest {
    Acknowledge(AckRequest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamRequest {
    pub method: String,
    pub path: String,
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
            Self::Message(MessageRequest::Acknowledge(request)) => {
                RequestEnvelope::Write(Box::new(request.into_write_request()))
            }
            Self::Clear(query) => RequestEnvelope::Clear(query),
            Self::Doctor(query) => RequestEnvelope::Doctor(query),
            Self::CompatibilityPreflight(preflight) => {
                RequestEnvelope::CompatibilityPreflight(preflight)
            }
            Self::Heartbeat(request) => RequestEnvelope::Heartbeat(request),
            Self::Teams(request) => panic!(
                "ApiRequest::Teams({}, {}) has no legacy RequestEnvelope representation",
                request.method, request.path
            ),
        }
    }

    fn from_http_parts(
        method: &str,
        path: &str,
        envelope: RequestEnvelope,
    ) -> Result<Self, AtmError> {
        let request = Self::from(envelope);
        if request.matches_route(method, path) {
            return Ok(request);
        }
        Err(AtmError::validation(format!(
            "daemon HTTP route {method} {path} does not match request body kind"
        )))
    }

    fn matches_route(&self, method: &str, path: &str) -> bool {
        match self {
            Self::Messages(_) => method == "GET" && path == "/v1/atm/messages",
            Self::Write(request) => {
                if request.acknowledges_message_id.is_some() {
                    method == "POST"
                        && path
                            .strip_prefix("/v1/atm/message/")
                            .is_some_and(|suffix| suffix.ends_with("/ack"))
                } else {
                    method == "POST" && path == "/v1/atm/messages"
                }
            }
            Self::Message(MessageRequest::Acknowledge(_)) => {
                method == "POST"
                    && path
                        .strip_prefix("/v1/atm/message/")
                        .is_some_and(|suffix| suffix.ends_with("/ack"))
            }
            Self::Clear(_) => {
                method == "DELETE" && (path == "/v1/atm/messages" || is_message_detail_path(path))
            }
            Self::Doctor(_) => method == "GET" && path == "/v1/atm/doctor",
            Self::Teams(_) => route_is_teams(method, path),
            Self::CompatibilityPreflight(_) => method == "POST" && path == "/v1/atm/compatibility",
            Self::Heartbeat(_) => method == "POST" && path == "/v1/atm/heartbeat",
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
        }
    }
}

fn is_message_detail_path(path: &str) -> bool {
    path.strip_prefix("/v1/atm/message/")
        .is_some_and(|suffix| !suffix.is_empty() && !suffix.contains('/'))
}

fn route_is_teams(method: &str, path: &str) -> bool {
    matches!(method, "GET" | "POST") && path == "/v1/atm/teams"
        || matches!(method, "GET" | "PATCH" | "DELETE")
            && path
                .strip_prefix("/v1/atm/team/")
                .is_some_and(|team| !team.is_empty() && !team.contains('/'))
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthenticatedIngress {
    Local,
    Peer(AuthenticatedPeer),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthenticatedPeer {
    _private: (),
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
    use super::{
        ApiRequest, MAX_HTTP_REQUEST_BODY_BYTES, decode_request, read_http_request,
        write_http_request,
    };
    use crate::doctor::DoctorQuery;
    use crate::protocol::RequestEnvelope;
    use crate::send::{SendMessageSource, SendRequest};
    use crate::test_support::{TEST_SENDER, TEST_TEAM};

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
    fn http_decode_uses_method_and_path_to_choose_write_variant() {
        let request = RequestEnvelope::Write(Box::new(
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
        let mut bytes = Vec::new();

        write_http_request(&mut bytes, &request).expect("write HTTP request");
        let decoded = decode_request(
            read_http_request(&mut bytes.as_slice())
                .expect("read HTTP request")
                .expect("request"),
        )
        .expect("decode HTTP request");

        assert!(matches!(
            decoded,
            ApiRequest::Write(request) if request.acknowledges_message_id.is_none()
        ));
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
    fn http_decode_declares_teams_route_correspondence() {
        let raw = "GET /v1/atm/teams HTTP/1.1\r\nContent-Length: 0\r\n\r\n";

        let decoded = decode_request(
            read_http_request(&mut raw.as_bytes())
                .expect("read HTTP request")
                .expect("request"),
        )
        .expect("decode teams request");

        assert!(matches!(decoded, ApiRequest::Teams(_)));
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
