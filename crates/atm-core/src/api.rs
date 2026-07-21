//! Transport-neutral API request and response contracts.
//!
//! UDS and future HTTPS adapters translate HTTP into this surface.  The
//! application router receives no socket, storage, or nudge capability.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use crate::error::AtmError;
use crate::protocol::{RequestEnvelope, ResponseEnvelope};

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
    .map_err(|_source| AtmError::daemon_unavailable("failed to write daemon HTTP request headers"))?;
    writer.write_all(&body).map_err(|_source| {
        AtmError::daemon_unavailable("failed to write daemon HTTP request body")
    })?;
    writer
        .flush()
        .map_err(|_source| AtmError::daemon_unavailable("failed to flush daemon HTTP request"))
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

pub fn decode_request(request: HttpRequest) -> Result<RequestEnvelope, AtmError> {
    if request.body.len() > MAX_HTTP_REQUEST_BODY_BYTES {
        return Err(AtmError::validation(
            "daemon HTTP request body exceeds 1048576 bytes",
        ));
    }
    serde_json::from_slice(&request.body).map_err(AtmError::from)
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
    .map_err(|_source| AtmError::daemon_unavailable("failed to write daemon HTTP response headers"))?;
    writer.write_all(&body).map_err(|_source| {
        AtmError::daemon_unavailable("failed to write daemon HTTP response body")
    })?;
    writer
        .flush()
        .map_err(|_source| AtmError::daemon_unavailable("failed to flush daemon HTTP response"))
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
            Err(_source) => {
                return Err(AtmError::daemon_unavailable(
                    "failed to read daemon HTTP headers",
                ));
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
    reader
        .read_exact(&mut body)
        .map_err(|_source| AtmError::daemon_unavailable("failed to read daemon HTTP body"))?;
    Ok(body)
}

#[derive(Debug, Clone)]
pub struct ApiRequest {
    request: RequestEnvelope,
}

impl ApiRequest {
    pub const fn new(request: RequestEnvelope) -> Self {
        Self { request }
    }

    pub fn into_inner(self) -> RequestEnvelope {
        self.request
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthenticatedIngress {
    Local,
    /// A peer that completed the HTTPS adapter's mutual-TLS and exact-pin
    /// checks. The application router receives no socket or peer configuration.
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
        MAX_HTTP_REQUEST_BODY_BYTES, decode_request, read_http_request, write_http_request,
    };
    use crate::doctor::DoctorQuery;
    use crate::protocol::RequestEnvelope;

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

        assert!(matches!(decoded, RequestEnvelope::Doctor(_)));
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
