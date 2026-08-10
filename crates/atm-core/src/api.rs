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
/// Adapter-owned provenance header for the explicit plaintext peer smoke mode.
pub const PEER_SOURCE_HOST_HEADER: &str = "X-ATM-Peer-Source-Host";
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
    route: HttpRoute,
}

// This one table is consumed by both outbound request construction and inbound
// decoding. Adding a route cannot make it to one direction without the other.
const HTTP_ROUTE_SPECS: &[HttpRouteSpec] = &[
    HttpRouteSpec {
        route: HttpRoute {
            method: "GET",
            path_template: MESSAGES_PATH,
        },
    },
    HttpRouteSpec {
        route: HttpRoute {
            method: "POST",
            path_template: MESSAGES_PATH,
        },
    },
    HttpRouteSpec {
        route: HttpRoute {
            method: "DELETE",
            path_template: MESSAGES_PATH,
        },
    },
    HttpRouteSpec {
        route: HttpRoute {
            method: "POST",
            path_template: INSPECT_PATH,
        },
    },
    HttpRouteSpec {
        route: HttpRoute {
            method: "POST",
            path_template: READ_PATH,
        },
    },
    HttpRouteSpec {
        route: HttpRoute {
            method: "GET",
            path_template: DOCTOR_PATH,
        },
    },
    HttpRouteSpec {
        route: HttpRoute {
            method: "POST",
            path_template: RUNTIME_RELOAD_PATH,
        },
    },
    HttpRouteSpec {
        route: HttpRoute {
            method: "POST",
            path_template: COMPATIBILITY_PATH,
        },
    },
    HttpRouteSpec {
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

#[allow(
    dead_code,
    reason = "the route table remains the published contract while framework ingress owns decoding"
)]
fn route_kind_for_http(method: &str, path: &str) -> Option<HttpRouteKind> {
    match (method, path) {
        ("POST", MESSAGES_PATH) => Some(HttpRouteKind::Write),
        ("GET", MESSAGES_PATH) => Some(HttpRouteKind::List),
        ("DELETE", MESSAGES_PATH) => Some(HttpRouteKind::Clear),
        ("POST", INSPECT_PATH) => Some(HttpRouteKind::Inspect),
        ("POST", READ_PATH) => Some(HttpRouteKind::Receive),
        ("GET", DOCTOR_PATH) => Some(HttpRouteKind::Doctor),
        ("POST", COMPATIBILITY_PATH) => Some(HttpRouteKind::Compatibility),
        ("POST", HEARTBEAT_PATH) => Some(HttpRouteKind::Heartbeat),
        ("POST", RUNTIME_RELOAD_PATH) => Some(HttpRouteKind::RuntimeReload),
        _ => None,
    }
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
    let (method, path) = endpoint_for(request);
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

#[cfg(all(test, any()))]
mod tests {
    use std::time::Duration;

    use base64::Engine;

    use super::{
        MAX_HTTP_REQUEST_BODY_BYTES, RequestDeadline, decode_http_response, encode_http_request,
        retired_decoder,
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

    #[test]
    fn zero_request_deadline_has_no_remaining_budget() {
        let deadline = RequestDeadline::after(Duration::ZERO);

        assert!(deadline.expired());
        assert_eq!(deadline.remaining(), None);
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
            let error = retired_response_decoder(&mut wire, request)
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
            let error = retired_response_decoder(&mut wire, request)
                .expect_err("invalid JSON response must include recovery guidance");
            assert!(error.is_validation());
            assert!(error.message().contains("Recovery:"), "{error:?}");
        }

        for mut wire in [
            b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".as_slice(),
            b"HTTP/1.1 204 No Content\r\nX-ATM-Clear-Outcome: invalid!\r\nContent-Length: 0\r\n\r\n".as_slice(),
        ] {
            let error = retired_response_decoder(&mut wire, &clear)
                .expect_err("malformed clear response must include recovery guidance");
            assert!(error.is_validation());
            assert!(error.message().contains("Recovery:"), "{error:?}");
        }

        let wire = format!(
            "HTTP/1.1 204 No Content\r\nX-ATM-Clear-Outcome: {valid_outcome}\r\nContent-Length: 0\r\n\r\n"
        );
        assert!(retired_response_decoder(&mut wire.as_bytes(), &clear).is_ok());
    }

    #[test]
    fn http_frame_reader_rejects_duplicate_content_length() {
        let wire =
            b"POST /v1/atm/messages HTTP/1.1\r\nContent-Length: 2\r\nContent-Length: 2\r\n\r\n{}";
        let error = RetiredFrameReader::new()
            .read_request(&mut wire.as_slice())
            .expect_err("duplicate Content-Length must be rejected");

        assert!(error.is_validation());
        assert!(error.message().contains("duplicate Content-Length"));
    }

    #[test]
    fn http_wire_body_is_route_schema_not_request_envelope() {
        let request = RequestEnvelope::Doctor(DoctorQuery::default());
        let mut bytes = Vec::new();

        retired_request_writer(&mut bytes, &request).expect("write HTTP request");

        let text = String::from_utf8(bytes).expect("HTTP UTF-8");
        assert!(text.starts_with("GET /v1/atm/doctor HTTP/1.1"));
        assert!(!text.contains("Doctor"));
        assert!(!text.contains("RequestEnvelope"));
    }

    #[test]
    fn http_error_is_direct_error_body_with_non_success_status() {
        let request = RequestEnvelope::Doctor(DoctorQuery::default());
        let mut bytes = Vec::new();

        retired_response_writer(
            &mut bytes,
            &ResponseEnvelope::Error(AtmError::validation("bad")),
        )
        .expect("write HTTP error");

        let text = String::from_utf8(bytes.clone()).expect("HTTP UTF-8");
        assert!(text.starts_with("HTTP/1.1 400 Bad Request"));
        assert!(!text.contains("Error\":{") && !text.contains("Error\""));
        let response =
            retired_response_decoder(&mut bytes.as_slice(), &request).expect("read error");
        assert!(matches!(response, ResponseEnvelope::Error(error) if error.is_validation()));
    }

    #[test]
    fn http_response_writes_headers_and_body_as_one_transport_frame() {
        let mut writer = WriteCountingWriter::default();

        retired_response_writer(
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

        retired_response_writer(
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

        retired_response_writer(&mut bytes, &response).expect("write clear response");

        let text = String::from_utf8(bytes.clone()).expect("HTTP UTF-8");
        assert!(text.starts_with("HTTP/1.1 204 No Content"));
        assert!(text.contains("X-ATM-Clear-Outcome:"));
        assert!(text.ends_with("\r\n\r\n"));
        let decoded =
            retired_response_decoder(&mut bytes.as_slice(), &request).expect("read clear");
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
                activity_observation: None,
                message_id: AtmMessageId::new(),
                reply_body: "acknowledged".to_string(),
            }
            .into_write_request(),
        ));

        for (request, is_ack) in [(send, false), (ack, true)] {
            let mut bytes = Vec::new();
            retired_request_writer(&mut bytes, &request).expect("write HTTP request");
            let raw = String::from_utf8(bytes.clone()).expect("HTTP UTF-8");
            assert!(raw.starts_with("POST /v1/atm/messages HTTP/1.1"));
            let decoded = retired_decoder(
                retired_request_reader(&mut bytes.as_slice())
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

        let decoded = retired_decoder(
            retired_request_reader(&mut raw.as_bytes())
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

        let error = retired_request_reader(&mut request.as_bytes()).expect_err("oversized body");

        assert!(error.is_validation());
    }
}
