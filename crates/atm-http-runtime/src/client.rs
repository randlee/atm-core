//! Connector-neutral client-side HTTP translation.
//!
//! This module owns no socket, DNS, TLS, or listener setup.  Physical
//! connectors are deliberately introduced by AL.5--AL.7.  It owns the one
//! route-body encoder and result decoder used after a connector is selected.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use atm_core::api::{
    ApiRequest, ApiResponse, DaemonApiClient, HttpRequest, RequestDeadline, decode_http_response,
    encode_http_request,
};
use atm_core::boundary;
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::local_http::{LOCAL_CAPABILITY_HEADER, LocalCapability, LocalHttpEndpointRecord};

use reqwest::header::{HeaderName, HeaderValue};

/// Connector-stage failure vocabulary retained by the shared client before it
/// becomes the public ATM error contract. Keeping the stage explicit prevents
/// connector implementations from silently collapsing DNS, TLS, write, and
/// cancellation failures into an indistinguishable generic transport error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HttpRuntimeClientFailure {
    EndpointRecord(AtmError),
    Connect(String),
    RequestWrite(String),
    ResponseDecode(AtmError),
    Cancelled,
    Timeout,
}

impl HttpRuntimeClientFailure {
    fn into_atm_error(self) -> AtmError {
        match self {
            Self::EndpointRecord(error) => error,
            Self::Connect(cause) => AtmError::daemon_unavailable(
                "HTTP client could not connect to the configured daemon endpoint",
            )
            .with_cause(cause),
            Self::RequestWrite(cause) => AtmError::daemon_unavailable(
                "HTTP client could not write the request to the daemon endpoint",
            )
            .with_cause(cause),
            Self::ResponseDecode(error) => error,
            Self::Cancelled => AtmError::new(
                AtmErrorCode::WaitTimeout,
                "HTTP client request was cancelled before a response arrived",
            ),
            Self::Timeout => AtmError::new(
                AtmErrorCode::WaitTimeout,
                "HTTP client request exceeded its absolute request budget",
            ),
        }
    }
}

/// Builds the shared daemon API client over the active capability-authenticated
/// loopback endpoint record. The record is resolved for every exchange so a
/// revoked or successor record fails before any request reaches the server.
pub fn loopback_tcp_client(
    endpoint_record_path: impl AsRef<Path>,
    request_timeout: Duration,
) -> Result<Arc<dyn DaemonApiClient>, AtmError> {
    if request_timeout.is_zero() {
        return Err(AtmError::config(
            "loopback HTTP client request timeout must be greater than zero",
        ));
    }
    if endpoint_record_path.as_ref().as_os_str().is_empty() {
        return Err(AtmError::config(
            "loopback HTTP client endpoint record path must not be empty",
        ));
    }
    let connector = LoopbackTcpConnector::new(endpoint_record_path.as_ref())?;
    Ok(Arc::new(HttpRuntimeClient::new(
        Arc::new(connector),
        request_timeout,
    )))
}

/// Reqwest-backed physical loopback connector. It adds exactly the local
/// capability header from the validated endpoint record; all request DTO
/// encoding and response mapping remain in [`HttpRuntimeClient`].
#[derive(Debug)]
struct LoopbackTcpConnector {
    client: reqwest::Client,
    endpoint_record_path: PathBuf,
}

impl LoopbackTcpConnector {
    fn new(endpoint_record_path: &Path) -> Result<Self, AtmError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|source| {
                AtmError::config("failed to build loopback HTTP client").with_cause(source)
            })?;
        Ok(Self {
            client,
            endpoint_record_path: endpoint_record_path.to_path_buf(),
        })
    }
}

#[async_trait]
impl HttpRuntimeConnector for LoopbackTcpConnector {
    async fn exchange(
        &self,
        request: HttpRequest,
        deadline: RequestDeadline,
    ) -> Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure> {
        let (endpoint, capability) =
            load_active_loopback_endpoint(&self.endpoint_record_path).await?;
        let url = reqwest::Url::parse(&format!("http://{endpoint}{}", request.path)).map_err(
            |source| {
                HttpRuntimeClientFailure::RequestWrite(format!(
                    "shared HTTP request has an invalid loopback route `{}`: {source}",
                    request.path
                ))
            },
        )?;
        execute_reqwest_request(
            &self.client,
            url,
            request,
            deadline,
            Some((LOCAL_CAPABILITY_HEADER, capability.to_base64url())),
        )
        .await
    }
}

async fn load_active_loopback_endpoint(
    endpoint_record_path: &Path,
) -> Result<(std::net::SocketAddr, LocalCapability), HttpRuntimeClientFailure> {
    let path = endpoint_record_path.to_path_buf();
    tokio::task::spawn_blocking(move || load_active_loopback_endpoint_blocking(&path))
        .await
        .map_err(|source| {
            HttpRuntimeClientFailure::EndpointRecord(
                AtmError::daemon_unavailable("loopback endpoint lookup task ended unexpectedly")
                    .with_cause(source),
            )
        })?
        .map_err(HttpRuntimeClientFailure::EndpointRecord)
}

fn load_active_loopback_endpoint_blocking(
    endpoint_record_path: &Path,
) -> Result<(std::net::SocketAddr, LocalCapability), AtmError> {
    let contents = std::fs::read(endpoint_record_path).map_err(|source| {
        AtmError::daemon_unavailable("failed to read local HTTP endpoint record").with_cause(source)
    })?;
    let record: LocalHttpEndpointRecord = serde_json::from_slice(&contents).map_err(|source| {
        AtmError::daemon_unavailable("failed to parse local HTTP endpoint record")
            .with_cause(source)
    })?;
    let capability = record.capability()?;
    let owner_instance_id =
        atm_core::local_http::owner_instance_id_for_local_http_record(endpoint_record_path)?;
    if record.daemon_instance_id != owner_instance_id {
        return Err(AtmError::daemon_unavailable(
            "local HTTP endpoint record belongs to a different daemon instance",
        ));
    }
    let endpoint = record
        .ipv4_loopback
        .or(record.ipv6_loopback)
        .ok_or_else(|| {
            AtmError::local_http_endpoint_missing(
                "local HTTP endpoint record has no loopback endpoint",
            )
        })?;
    if !endpoint.ip().is_loopback() {
        return Err(AtmError::local_http_endpoint_non_loopback(
            "local HTTP endpoint record contains a non-loopback address",
        ));
    }
    Ok((endpoint, capability))
}

/// Physical connection setup for the one shared HTTP client.
///
/// Implementations own only endpoint/DNS resolution, connection, TLS, request
/// write, and response read. They must preserve the supplied absolute deadline
/// and return the existing [`AtmError`] vocabulary with their concrete cause.
/// This is a connector seam, not a second ATM client boundary.
#[async_trait]
pub(crate) trait HttpRuntimeConnector: Send + Sync {
    async fn exchange(
        &self,
        request: HttpRequest,
        deadline: RequestDeadline,
    ) -> Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure>;
}

/// Builds the shared daemon API client over an owner-authorized Unix socket.
///
/// The physical socket is the only Unix-specific concern. Request encoding,
/// response decoding, deadline enforcement, and the public error contract are
/// all owned by [`HttpRuntimeClient`].
#[cfg(unix)]
pub fn unix_socket_client(
    socket_path: impl AsRef<Path>,
    request_timeout: Duration,
) -> Result<Arc<dyn DaemonApiClient>, AtmError> {
    if request_timeout.is_zero() {
        return Err(AtmError::config(
            "Unix HTTP client request timeout must be greater than zero",
        ));
    }
    crate::validate_unix_socket_path(socket_path.as_ref())?;
    let connector = UnixSocketConnector::new(socket_path.as_ref())?;
    Ok(Arc::new(HttpRuntimeClient::new(
        Arc::new(connector),
        request_timeout,
    )))
}

/// Reqwest-backed physical Unix-domain connector.
///
/// Reqwest owns connection pooling, HTTP framing, cancellation, and I/O. This
/// adapter only supplies the configured Unix endpoint and converts the
/// core-owned HTTP DTO at the shared-client seam.
#[cfg(unix)]
#[derive(Debug)]
struct UnixSocketConnector {
    client: reqwest::Client,
}

#[cfg(unix)]
impl UnixSocketConnector {
    fn new(socket_path: &Path) -> Result<Self, AtmError> {
        let client = reqwest::Client::builder()
            .unix_socket(socket_path)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|source| {
                AtmError::config("failed to build Unix HTTP client").with_cause(source)
            })?;
        Ok(Self { client })
    }
}

#[cfg(unix)]
#[async_trait]
impl HttpRuntimeConnector for UnixSocketConnector {
    async fn exchange(
        &self,
        request: HttpRequest,
        deadline: RequestDeadline,
    ) -> Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure> {
        let url = reqwest::Url::parse(&format!("http://localhost{}", request.path)).map_err(
            |source| {
                HttpRuntimeClientFailure::RequestWrite(format!(
                    "shared HTTP request has an invalid route `{}`: {source}",
                    request.path
                ))
            },
        )?;
        execute_reqwest_request(&self.client, url, request, deadline, None).await
    }
}

async fn execute_reqwest_request(
    client: &reqwest::Client,
    url: reqwest::Url,
    request: HttpRequest,
    deadline: RequestDeadline,
    additional_header: Option<(&'static str, String)>,
) -> Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure> {
    if deadline.expired() {
        return Err(HttpRuntimeClientFailure::Cancelled);
    }
    let method = request.method.parse().map_err(|source| {
        HttpRuntimeClientFailure::RequestWrite(format!(
            "shared HTTP request has an invalid method `{}`: {source}",
            request.method
        ))
    })?;
    let mut outbound = reqwest::Request::new(method, url);
    *outbound.body_mut() = Some(request.body.into());
    for header in request.headers {
        let (name, value) = header.split_once(':').ok_or_else(|| {
            HttpRuntimeClientFailure::RequestWrite(format!(
                "shared HTTP request has a malformed header `{header}`"
            ))
        })?;
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|source| {
            HttpRuntimeClientFailure::RequestWrite(format!(
                "shared HTTP request has an invalid header name `{name}`: {source}"
            ))
        })?;
        let value = HeaderValue::from_str(value.trim_start()).map_err(|source| {
            HttpRuntimeClientFailure::RequestWrite(format!(
                "shared HTTP request has an invalid value for `{name}`: {source}"
            ))
        })?;
        outbound.headers_mut().append(name, value);
    }
    if let Some((name, value)) = additional_header {
        let value = HeaderValue::from_str(&value).map_err(|source| {
            HttpRuntimeClientFailure::RequestWrite(format!(
                "loopback capability header has an invalid value: {source}"
            ))
        })?;
        outbound.headers_mut().insert(name, value);
    }

    let response = client.execute(outbound).await.map_err(|source| {
        HttpRuntimeClientFailure::Connect(format!("HTTP connector request failed: {source}"))
    })?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.bytes().await.map_err(|source| {
        HttpRuntimeClientFailure::ResponseDecode(
            AtmError::daemon_unavailable("failed to read HTTP response body").with_cause(source),
        )
    })?;
    let mut builder = axum::http::Response::builder().status(status);
    for (name, value) in &headers {
        builder = builder.header(name.as_str(), value.as_bytes());
    }
    builder.body(body.to_vec()).map_err(|source| {
        HttpRuntimeClientFailure::ResponseDecode(
            AtmError::daemon_unavailable("failed to construct shared HTTP response")
                .with_cause(source),
        )
    })
}

/// One framework-backed client operation for every physical adapter.
///
/// The connector is the only place UDS, loopback, or TLS can differ. Request
/// encoding, route selection, response decoding, and outcome mapping remain
/// in this type.
#[derive(Debug, Clone)]
pub(crate) struct HttpRuntimeClient<Connector> {
    connector: Arc<Connector>,
    request_timeout: Duration,
}

impl<Connector> HttpRuntimeClient<Connector> {
    #[must_use]
    pub(crate) fn new(connector: Arc<Connector>, request_timeout: Duration) -> Self {
        Self {
            connector,
            request_timeout,
        }
    }

    #[tracing::instrument(
        name = "atm_http_runtime.client.execute",
        skip(self, request),
        fields(deadline_remaining_ms = ?deadline.remaining().map(|duration| duration.as_millis()))
    )]
    async fn execute_with_deadline(
        &self,
        request: ApiRequest,
        deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError>
    where
        Connector: HttpRuntimeConnector,
    {
        let request = request.into_inner();
        let encoded = encode_http_request(&request, &[])?;
        let remaining = deadline.remaining().ok_or_else(|| {
            AtmError::new(
                AtmErrorCode::WaitTimeout,
                "HTTP client request budget elapsed before connector dispatch",
            )
        })?;
        let response = tokio::time::timeout(remaining, self.connector.exchange(encoded, deadline))
            .await
            .map_err(|_| HttpRuntimeClientFailure::Timeout.into_atm_error())?
            .map_err(HttpRuntimeClientFailure::into_atm_error)?;
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| {
                format!(
                    "{name}: {}",
                    value.to_str().unwrap_or("<non-UTF-8 header value>")
                )
            })
            .collect::<Vec<_>>();
        decode_http_response(
            &request,
            response.status().as_u16(),
            &headers,
            response.body(),
        )
        .map(ApiResponse::new)
        .map_err(|error| HttpRuntimeClientFailure::ResponseDecode(error).into_atm_error())
    }
}

impl<Connector> boundary::sealed::Sealed for HttpRuntimeClient<Connector> where
    Connector: Send + Sync
{
}

#[async_trait]
impl<Connector> DaemonApiClient for HttpRuntimeClient<Connector>
where
    Connector: HttpRuntimeConnector + 'static,
{
    async fn execute(&self, request: ApiRequest) -> Result<ApiResponse, AtmError> {
        self.execute_with_deadline(request, RequestDeadline::after(self.request_timeout))
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;
    use atm_core::api::{ApiRequest, DaemonApiClient, HttpRequest, RequestDeadline};
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, SendResponseEnvelope};
    use atm_core::send::{SendCommandOutcome, SendMessageSource, SendRequest};
    use atm_core::test_support::{TEST_RECIPIENT, TEST_SENDER, TEST_TEAM};
    use atm_core::types::{AgentName, CommandAction, TeamName};

    use super::{HttpRuntimeClient, HttpRuntimeClientFailure, HttpRuntimeConnector};

    #[derive(Default)]
    struct RecordingConnector {
        requests: Mutex<Vec<HttpRequest>>,
        responses: Mutex<VecDeque<Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure>>>,
    }

    #[async_trait]
    impl HttpRuntimeConnector for RecordingConnector {
        async fn exchange(
            &self,
            request: HttpRequest,
            _deadline: RequestDeadline,
        ) -> Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure> {
            self.requests.lock().expect("requests").push(request);
            self.responses
                .lock()
                .expect("responses")
                .pop_front()
                .expect("configured response")
        }
    }

    fn write_request() -> RequestEnvelope {
        RequestEnvelope::Write(Box::new(
            SendRequest::new(
                ".".into(),
                ".".into(),
                AgentName::from_validated(TEST_SENDER),
                TEST_RECIPIENT,
                TeamName::from_validated(TEST_TEAM),
                SendMessageSource::Inline("shared client".to_owned()),
                None,
                false,
                None,
                false,
            )
            .expect("request"),
        ))
    }

    fn sent_response() -> axum::http::Response<Vec<u8>> {
        let outcome = atm_core::send::SendOutcome {
            action: CommandAction::Send,
            team: TeamName::from_validated(TEST_TEAM),
            agent: AgentName::from_validated(TEST_RECIPIENT),
            sender: AgentName::from_validated(TEST_SENDER),
            outcome: SendCommandOutcome::Sent,
            message_id: atm_core::schema::AtmMessageId::new(),
            requires_ack: false,
            task_id: None,
            summary: None,
            message: None,
            warnings: Vec::new(),
            dry_run: false,
        };
        axum::http::Response::builder()
            .status(201)
            .body(serde_json::to_vec(&outcome).expect("encode outcome"))
            .expect("response")
    }

    #[tokio::test]
    async fn uds_and_loopback_connectors_share_one_write_translation() {
        for connector_kind in ["uds", "loopback"] {
            let connector = Arc::new(RecordingConnector::default());
            connector
                .responses
                .lock()
                .expect("responses")
                .push_back(Ok(sent_response()));
            let client = HttpRuntimeClient::new(connector.clone(), Duration::from_secs(1));

            let response = client
                .execute(ApiRequest::new(write_request()))
                .await
                .expect("shared client response");

            assert!(matches!(
                response.into_inner(),
                ResponseEnvelope::Send(SendResponseEnvelope::Sent(_))
            ));
            let requests = connector.requests.lock().expect("requests");
            assert_eq!(requests.len(), 1, "{connector_kind} connector");
            assert_eq!(requests[0].method, "POST", "{connector_kind} connector");
            assert_eq!(
                requests[0].path, "/v1/atm/messages",
                "{connector_kind} connector"
            );
            assert!(
                serde_json::from_slice::<atm_core::send::WriteRequest>(&requests[0].body).is_ok(),
                "{connector_kind} connector"
            );
        }
    }

    #[tokio::test]
    async fn response_status_preserves_the_adr_032_error_body() {
        let connector = Arc::new(RecordingConnector::default());
        let expected = AtmError::validation("rejected by server");
        connector.responses.lock().expect("responses").push_back(Ok(
            axum::http::Response::builder()
                .status(400)
                .body(serde_json::to_vec(&expected).expect("encode error"))
                .expect("response"),
        ));
        let client = HttpRuntimeClient::new(connector, Duration::from_secs(1));

        let response = client
            .execute(ApiRequest::new(write_request()))
            .await
            .expect("typed error is a response");

        assert!(
            matches!(response.into_inner(), ResponseEnvelope::Error(error) if error == expected)
        );
    }

    #[tokio::test]
    async fn malformed_response_body_uses_the_existing_protocol_error() {
        let connector = Arc::new(RecordingConnector::default());
        connector.responses.lock().expect("responses").push_back(Ok(
            axum::http::Response::builder()
                .status(201)
                .body(b"not a response envelope".to_vec())
                .expect("response"),
        ));
        let client = HttpRuntimeClient::new(connector, Duration::from_secs(1));

        let error = client
            .execute(ApiRequest::new(write_request()))
            .await
            .expect_err("malformed response must not be accepted");

        assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
    }

    #[tokio::test]
    async fn every_named_client_failure_has_stable_code_and_recovery_context() {
        for (cause, failure, code, recovery_fragment) in [
            (
                "connect/refusal/network reachability",
                HttpRuntimeClientFailure::Connect("connection refused".to_owned()),
                AtmErrorCode::DaemonUnavailable,
                "connect",
            ),
            (
                "request write",
                HttpRuntimeClientFailure::RequestWrite("broken pipe".to_owned()),
                AtmErrorCode::DaemonUnavailable,
                "write",
            ),
            (
                "cancellation",
                HttpRuntimeClientFailure::Cancelled,
                AtmErrorCode::WaitTimeout,
                "cancelled",
            ),
            (
                "timeout",
                HttpRuntimeClientFailure::Timeout,
                AtmErrorCode::WaitTimeout,
                "budget",
            ),
        ] {
            let connector = Arc::new(RecordingConnector::default());
            connector
                .responses
                .lock()
                .expect("responses")
                .push_back(Err(failure));
            let client = HttpRuntimeClient::new(connector, Duration::from_secs(1));

            let error = client
                .execute(ApiRequest::new(write_request()))
                .await
                .expect_err(cause);

            assert_eq!(error.code(), code, "{cause}");
            assert!(
                error.message().contains(recovery_fragment),
                "{cause}: {error}"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn request_deadline_bounds_the_connector_once() {
        struct WaitingConnector;
        #[async_trait]
        impl HttpRuntimeConnector for WaitingConnector {
            async fn exchange(
                &self,
                _request: HttpRequest,
                _deadline: RequestDeadline,
            ) -> Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure> {
                tokio::time::sleep(Duration::from_secs(60)).await;
                unreachable!("deadline must cancel the connector")
            }
        }

        let client = HttpRuntimeClient::new(Arc::new(WaitingConnector), Duration::from_millis(1));
        let task =
            tokio::spawn(async move { client.execute(ApiRequest::new(write_request())).await });
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(1)).await;
        let error = task
            .await
            .expect("timeout task completes")
            .expect_err("request must time out");
        assert_eq!(error.code(), AtmErrorCode::WaitTimeout);
    }
}
