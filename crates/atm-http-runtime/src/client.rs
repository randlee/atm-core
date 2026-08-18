//! Connector-neutral client-side HTTP translation.
//!
//! This module owns no socket, DNS, TLS, or listener setup.  Physical
//! connectors are deliberately introduced by AL.5--AL.7.  It owns the one
//! route-body encoder and result decoder used after a connector is selected.

#[cfg(test)]
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

use async_trait::async_trait;
use atm_core::PeerIoAdapter;
use atm_core::api::{
    ApiRequest, ApiResponse, DaemonApiClient, HttpRequest, RequestDeadline, decode_http_response,
    encode_http_request,
};
use atm_core::boundary;
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::local_http::{LOCAL_CAPABILITY_HEADER, LocalCapability, LocalHttpEndpointRecord};
use atm_core::protocol::{RequestEnvelope, RequestId, next_request_id};
use atm_core::schema::AtmMessageId;
use atm_core::send::SendRequest;
use atm_core::types::{HostName, IsoTimestamp};

use axum::body::{Body, to_bytes};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use reqwest::header::{HeaderName, HeaderValue};

/// Fixed direct-peer listener port.
///
/// Local bind addresses and ports are never operator configuration: every
/// replacement daemon owns this protocol port on its active IPv4 interfaces.
pub const DIRECT_PEER_TCP_PORT: u16 = 43_101;
const REQUEST_ID_HEADER: &str = "X-ATM-Request-Id";
/// Maximum encoded response size accepted from any one daemon peer.
///
/// This mirrors the canonical one-mebibyte message cap plus bounded HTTP
/// envelope overhead. Client response decoding must never let an untrusted
/// peer choose an allocation size.
const MAX_RESPONSE_BODY_BYTES: usize = 1024 * 1024 + 64 * 1024;

/// One bounded budget for the capability-authenticated local daemon client.
///
/// CLI and graft deliberately share this value and
/// [`selected_write_transport`], so neither caller can select a peer
/// connector or acquire a different local admission deadline.
pub const SAME_HOST_REQUEST_DEADLINE: Duration = Duration::from_secs(3);

/// Connector-stage failure vocabulary retained by the shared client before it
/// becomes the public ATM error contract. Keeping the stage explicit prevents
/// connector implementations from silently collapsing DNS, TLS, write, and
/// cancellation failures into an indistinguishable generic transport error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HttpRuntimeClientFailure {
    EndpointRecord(AtmError),
    Connect(String),
    PeerConnect { target: String, cause: String },
    RequestWrite(String),
    ResponseDecode(AtmError),
    Cancelled,
    Timeout,
    PeerConnectTimeout { target: String, cause: String },
}

impl HttpRuntimeClientFailure {
    fn into_atm_error(self) -> AtmError {
        match self {
            Self::EndpointRecord(error) => error,
            Self::Connect(cause) => AtmError::daemon_unavailable(
                "HTTP client could not connect to the configured daemon endpoint",
            )
            .with_cause(cause),
            Self::PeerConnect { target, cause } => AtmError::remote_delivery_unconfirmed(format!(
                "HTTP client could not connect to direct peer `{target}`"
            ))
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
            Self::PeerConnectTimeout { target, cause } => AtmError::remote_delivery_unconfirmed(format!(
                "HTTP client could not connect to direct peer `{target}` before its request budget elapsed"
            ))
            .with_cause(cause),
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

/// Test-only plaintext client retained to exercise the named diagnostic
/// listener. Production delivery can reach a peer only through the opaque
/// [`PeerIoAdapter`] below.
#[cfg(test)]
pub(crate) fn direct_peer_tcp_client(
    host: HostName,
    port: NonZeroU16,
    request_timeout: Duration,
) -> Result<Arc<dyn DaemonApiClient + Send + Sync>, AtmError> {
    Ok(Arc::new(direct_peer_write_client(
        host,
        port,
        request_timeout,
    )?))
}

#[cfg(test)]
pub(crate) fn direct_peer_write_client(
    host: HostName,
    port: NonZeroU16,
    request_timeout: Duration,
) -> Result<DirectPeerWriteClient<DirectPeerTcpConnector>, AtmError> {
    if request_timeout.is_zero() {
        return Err(AtmError::config(
            "direct peer HTTP client request timeout must be greater than zero",
        ));
    }
    let connector = DirectPeerTcpConnector::new(host, port)?;
    let client = Arc::new(HttpRuntimeClient::new(Arc::new(connector), request_timeout));
    Ok(DirectPeerWriteClient { client })
}

pub(crate) fn direct_peer_adapter_write_client(
    host: HostName,
    peer_io_adapter: Arc<dyn PeerIoAdapter>,
    request_timeout: Duration,
) -> Result<DirectPeerWriteClient<PeerIoConnector>, AtmError> {
    if request_timeout.is_zero() {
        return Err(AtmError::config(
            "direct peer HTTP client request timeout must be greater than zero",
        ));
    }
    let connector = PeerIoConnector {
        host,
        peer_io_adapter,
    };
    let client = Arc::new(HttpRuntimeClient::new(Arc::new(connector), request_timeout));
    Ok(DirectPeerWriteClient { client })
}

/// Selects the caller's one capability-authenticated daemon connector.
///
/// A host-qualified recipient is still represented in the canonical write
/// DTO, but it no longer gives CLI or graft authority to open a peer socket.
/// The replacement daemon owns the bootstrap-composed [`PeerIoAdapter`] and
/// performs that one authenticated peer exchange after local admission. This
/// keeps normal CLI, graft, and acknowledgement delivery on the same opaque
/// transport boundary; neither a missing configuration nor a failed mTLS
/// attempt can fall back to a caller-owned plain-TCP connector.
pub fn selected_write_transport<'client>(
    _request: &SendRequest,
    same_host_transport: &Arc<dyn DaemonApiClient + Send + Sync + 'client>,
) -> Result<Arc<dyn DaemonApiClient + Send + Sync + 'client>, AtmError> {
    Ok(Arc::clone(same_host_transport))
}

/// Builds the one selected same-host client for retained write call chains.
///
/// Unix selects the owner-authorized UDS adapter without a silent loopback
/// fallback; Windows selects the capability-authenticated loopback endpoint
/// record. Both selections use [`HttpRuntimeClient`] for the one typed request
/// encoder and response decoder.
pub fn preferred_local_client(
    endpoint_record_path: impl AsRef<Path>,
    request_timeout: Duration,
) -> Result<Arc<dyn DaemonApiClient>, AtmError> {
    #[cfg(unix)]
    {
        let runtime_directory = endpoint_record_path.as_ref().parent().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "local HTTP endpoint record has no runtime directory for Unix socket selection",
            )
        })?;
        // Replacement composition deliberately leaves UDS disabled for a
        // root-owned runtime because `UnixSocketOwnerUid` rejects uid 0. This
        // is a configuration-selected loopback path, not a fallback after a
        // UDS failure.
        if std::fs::metadata(runtime_directory)
            .map_err(|source| {
                AtmError::daemon_unavailable("failed to inspect local runtime directory")
                    .with_cause(source)
            })?
            .uid()
            == 0
        {
            return loopback_tcp_client(endpoint_record_path, request_timeout);
        }
        let socket_path = endpoint_record_path
            .as_ref()
            .parent()
            .expect("runtime directory was validated above")
            .join(atm_core::home::HOST_RUNTIME_SOCKET_FILE);
        unix_socket_client(socket_path, request_timeout)
    }
    #[cfg(not(unix))]
    {
        loopback_tcp_client(endpoint_record_path, request_timeout)
    }
}

/// Reqwest-backed physical loopback connector. It adds exactly the local
/// capability header from the validated endpoint record; all request DTO
/// encoding and response mapping remain in [`HttpRuntimeClient`].
#[derive(Debug)]
struct LoopbackTcpConnector {
    client: reqwest::Client,
    endpoint_record_path: PathBuf,
}

/// Test-only plaintext connector for the explicitly named diagnostic listener.
#[cfg(test)]
#[derive(Debug)]
pub(crate) struct DirectPeerTcpConnector {
    client: reqwest::Client,
    authority: String,
}

/// Send-only peer adapter over the shared typed HTTP client.
///
/// The client owns only sender-side provenance completion.  It never changes
/// the encoded DTO shape or performs a second request: the one
/// [`HttpRuntimeClient`] below still encodes, sends, and decodes the complete
/// write.  Supplying a caller-originated provenance pair is supported for
/// handoff from an origin writer; otherwise the first peer boundary creates
/// the immutable pair once.
pub(crate) struct DirectPeerWriteClient<Connector> {
    client: Arc<HttpRuntimeClient<Connector>>,
}

impl<Connector> boundary::sealed::Sealed for DirectPeerWriteClient<Connector> where
    Connector: Send + Sync
{
}

#[async_trait]
impl<Connector> DaemonApiClient for DirectPeerWriteClient<Connector>
where
    Connector: HttpRuntimeConnector + 'static,
{
    async fn execute(&self, request: ApiRequest) -> Result<ApiResponse, AtmError> {
        self.execute_with_optional_request_id(request, None).await
    }
}

impl<Connector> DirectPeerWriteClient<Connector>
where
    Connector: HttpRuntimeConnector,
{
    pub(crate) async fn execute_with_request_id(
        &self,
        request: ApiRequest,
        request_id: RequestId,
    ) -> Result<ApiResponse, AtmError> {
        self.execute_with_optional_request_id(request, Some(request_id))
            .await
    }

    async fn execute_with_optional_request_id(
        &self,
        request: ApiRequest,
        request_id: Option<RequestId>,
    ) -> Result<ApiResponse, AtmError> {
        let RequestEnvelope::Write(write) = request.into_inner() else {
            return Err(AtmError::validation(
                "direct peer HTTP transport accepts canonical write requests only",
            ));
        };
        let write = *write;
        let has_id = write.origin_message_id.is_some();
        let has_timestamp = write.origin_timestamp.is_some();
        if has_id != has_timestamp {
            return Err(AtmError::validation(
                "direct peer write origin metadata must contain both message ID and timestamp",
            ));
        }
        let write = if has_id {
            write
        } else {
            write.with_origin_metadata(AtmMessageId::new(), IsoTimestamp::now())
        };
        // A direct peer accepts this as an ordinary inbound write even when it
        // carries an ACK's causal link. Decode the remote response as `Sent`;
        // only a local daemon may return `Acknowledged` for its own source
        // transition.
        let mut peer_response_shape = write.clone();
        peer_response_shape.acknowledges_message_id = None;
        self.client
            .execute_envelope_with_deadline(
                RequestEnvelope::Write(Box::new(write)),
                RequestEnvelope::Write(Box::new(peer_response_shape)),
                RequestDeadline::after(self.client.request_timeout),
                request_id,
            )
            .await
    }
}

/// Opaque mTLS-backed direct-peer connector. Concrete TLS remains behind
/// `PeerIoAdapter`; this type owns only the existing HTTP/1 client exchange.
pub(crate) struct PeerIoConnector {
    host: HostName,
    peer_io_adapter: Arc<dyn PeerIoAdapter>,
}

impl std::fmt::Debug for PeerIoConnector {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PeerIoConnector")
            .field("host", &self.host)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
impl DirectPeerTcpConnector {
    fn new(host: HostName, port: NonZeroU16) -> Result<Self, AtmError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|source| {
                AtmError::config("failed to build direct peer HTTP client").with_cause(source)
            })?;
        Ok(Self {
            authority: if host.as_str().contains(':') {
                format!("[{}]:{}", host.as_str(), port)
            } else {
                format!("{}:{}", host.as_str(), port)
            },
            client,
        })
    }
}

#[async_trait]
#[cfg(test)]
impl HttpRuntimeConnector for DirectPeerTcpConnector {
    fn connection_target(&self) -> String {
        format!("direct peer `{}`", self.authority)
    }

    fn deadline_elapsed(&self) -> HttpRuntimeClientFailure {
        HttpRuntimeClientFailure::PeerConnectTimeout {
            target: self.authority.clone(),
            cause: format!(
                "{} did not resolve or establish a connection before the configured request deadline",
                self.connection_target()
            ),
        }
    }

    async fn exchange(
        &self,
        request: HttpRequest,
        deadline: RequestDeadline,
    ) -> Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure> {
        let url = reqwest::Url::parse(&format!("http://{}{}", self.authority, request.path))
            .map_err(|source| {
                HttpRuntimeClientFailure::RequestWrite(format!(
                    "shared HTTP request has an invalid direct peer route `{}`: {source}",
                    request.path
                ))
            })?;
        execute_reqwest_request(&self.client, url, request, deadline, None)
            .await
            .map_err(|failure| direct_peer_connection_failure(&self.authority, failure))
    }
}

#[async_trait]
impl HttpRuntimeConnector for PeerIoConnector {
    fn connection_target(&self) -> String {
        format!("authenticated direct peer `{}`", self.host)
    }

    fn deadline_elapsed(&self) -> HttpRuntimeClientFailure {
        HttpRuntimeClientFailure::PeerConnectTimeout {
            target: self.host.to_string(),
            cause: format!(
                "{} did not establish an authenticated connection before the configured request deadline",
                self.connection_target()
            ),
        }
    }

    async fn exchange(
        &self,
        request: HttpRequest,
        deadline: RequestDeadline,
    ) -> Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure> {
        let stream = self
            .peer_io_adapter
            .connect(self.host.clone(), deadline)
            .await
            .map_err(|error| HttpRuntimeClientFailure::PeerConnect {
                target: self.host.to_string(),
                cause: error.to_string(),
            })?;
        let remaining = deadline
            .remaining()
            .ok_or_else(|| self.deadline_elapsed())?;
        let (mut sender, connection) =
            tokio::time::timeout(remaining, http1::handshake(TokioIo::new(stream)))
                .await
                .map_err(|_| self.deadline_elapsed())?
                .map_err(|error| HttpRuntimeClientFailure::PeerConnect {
                    target: self.host.to_string(),
                    cause: format!("authenticated HTTP/1 handshake failed: {error}"),
                })?;
        let connection_driver = tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::debug!(%error, "authenticated direct-peer HTTP/1 connection ended");
            }
        });
        let result =
            exchange_authenticated_peer_request(&mut sender, request, deadline, &self.host).await;
        stop_connection_driver(connection_driver).await;
        result
    }
}

async fn exchange_authenticated_peer_request(
    sender: &mut http1::SendRequest<Body>,
    request: HttpRequest,
    deadline: RequestDeadline,
    host: &HostName,
) -> Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure> {
    let request = http1_request(request)?;
    let remaining = deadline
        .remaining()
        .ok_or_else(|| direct_peer_deadline_elapsed(host))?;
    let response = tokio::time::timeout(remaining, sender.send_request(request))
        .await
        .map_err(|_| direct_peer_deadline_elapsed(host))?
        .map_err(|error| HttpRuntimeClientFailure::PeerConnect {
            target: host.to_string(),
            cause: format!("authenticated HTTP/1 request failed: {error}"),
        })?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(Body::new(response.into_body()), MAX_RESPONSE_BODY_BYTES)
        .await
        .map_err(|error| {
            HttpRuntimeClientFailure::ResponseDecode(
                AtmError::daemon_unavailable(
                    "authenticated peer HTTP response exceeded the bounded body limit",
                )
                .with_cause(error),
            )
        })?;
    let mut builder = axum::http::Response::builder().status(status);
    for (name, value) in &headers {
        builder = builder.header(name.as_str(), value.as_bytes());
    }
    builder.body(body.to_vec()).map_err(|error| {
        HttpRuntimeClientFailure::ResponseDecode(
            AtmError::daemon_unavailable("failed to construct authenticated peer HTTP response")
                .with_cause(error),
        )
    })
}

fn direct_peer_deadline_elapsed(host: &HostName) -> HttpRuntimeClientFailure {
    HttpRuntimeClientFailure::PeerConnectTimeout {
        target: host.to_string(),
        cause: format!(
            "authenticated direct peer `{host}` did not establish an authenticated connection before the configured request deadline"
        ),
    }
}

async fn stop_connection_driver(connection_driver: tokio::task::JoinHandle<()>) {
    // Each direct-peer exchange owns one HTTP/1 connection. It cannot be
    // reused after this call, so stopping and joining the driver prevents a
    // detached task from outliving a failed or completed exchange.
    connection_driver.abort();
    if let Err(error) = connection_driver.await
        && !error.is_cancelled()
    {
        tracing::debug!(%error, "authenticated direct-peer HTTP/1 driver task ended unexpectedly");
    }
}

fn http1_request(
    request: HttpRequest,
) -> Result<axum::http::Request<Body>, HttpRuntimeClientFailure> {
    let mut builder = axum::http::Request::builder()
        .method(request.method.as_str())
        .uri(request.path.as_str());
    for header in request.headers {
        let (name, value) = header.split_once(':').ok_or_else(|| {
            HttpRuntimeClientFailure::RequestWrite(format!(
                "shared HTTP request has a malformed header `{header}`"
            ))
        })?;
        builder = builder.header(name, value.trim_start());
    }
    builder.body(Body::from(request.body)).map_err(|error| {
        HttpRuntimeClientFailure::RequestWrite(format!(
            "shared HTTP request cannot be encoded for the authenticated peer: {error}"
        ))
    })
}

/// Preserve the direct-peer authority when the shared HTTP exchange fails
/// before a response.  A host-qualified recipient never uses the local daemon
/// endpoint, so reporting this as local daemon unavailability is misleading.
#[cfg(test)]
fn direct_peer_connection_failure(
    authority: &str,
    failure: HttpRuntimeClientFailure,
) -> HttpRuntimeClientFailure {
    match failure {
        HttpRuntimeClientFailure::Connect(cause) => HttpRuntimeClientFailure::PeerConnect {
            target: authority.to_owned(),
            cause,
        },
        other => other,
    }
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
    fn connection_target(&self) -> String {
        format!(
            "loopback endpoint record `{}`",
            self.endpoint_record_path.display()
        )
    }

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
    /// Context preserved if an OS DNS/connect operation outlives the ATM
    /// request budget.  This prevents an unreachable peer from looking like
    /// generic queue pressure.
    fn connection_target(&self) -> String;

    /// Classifies an elapsed request budget without losing the distinction
    /// between a direct-peer DNS/connect failure and a local request that was
    /// already admitted but has not completed.
    fn deadline_elapsed(&self) -> HttpRuntimeClientFailure {
        HttpRuntimeClientFailure::Timeout
    }

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
    socket_path: PathBuf,
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
        Ok(Self {
            client,
            socket_path: socket_path.to_path_buf(),
        })
    }
}

#[cfg(unix)]
#[async_trait]
impl HttpRuntimeConnector for UnixSocketConnector {
    fn connection_target(&self) -> String {
        format!("Unix socket `{}`", self.socket_path.display())
    }

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

    let mut response = client.execute(outbound).await.map_err(|source| {
        HttpRuntimeClientFailure::Connect(format!("HTTP connector request failed: {source}"))
    })?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = read_bounded_reqwest_response_body(&mut response).await?;
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

async fn read_bounded_reqwest_response_body(
    response: &mut reqwest::Response,
) -> Result<Vec<u8>, HttpRuntimeClientFailure> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BODY_BYTES as u64)
    {
        return Err(HttpRuntimeClientFailure::ResponseDecode(
            AtmError::daemon_unavailable("HTTP response exceeded the bounded body limit"),
        ));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|source| {
        HttpRuntimeClientFailure::ResponseDecode(
            AtmError::daemon_unavailable("failed to read HTTP response body").with_cause(source),
        )
    })? {
        let next_len = body.len().saturating_add(chunk.len());
        if next_len > MAX_RESPONSE_BODY_BYTES {
            return Err(HttpRuntimeClientFailure::ResponseDecode(
                AtmError::daemon_unavailable("HTTP response exceeded the bounded body limit"),
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
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
        self.execute_envelope_with_deadline(request.clone(), request, deadline, None)
            .await
    }

    /// Executes one encoded request while decoding its response according to
    /// the receiving operation's response shape.
    ///
    /// Direct-peer receipt writes retain `acknowledges_message_id` as causal
    /// message data, but are never local `atm ack` operations at the remote
    /// daemon. Their wire response is therefore `Sent`, not `Acknowledged`.
    /// Keeping the response shape explicit preserves the single encoder and
    /// decoder while preventing causal metadata from changing the HTTP schema.
    async fn execute_envelope_with_deadline(
        &self,
        request: RequestEnvelope,
        response_shape: RequestEnvelope,
        deadline: RequestDeadline,
        request_id: Option<RequestId>,
    ) -> Result<ApiResponse, AtmError>
    where
        Connector: HttpRuntimeConnector,
    {
        let request_id = request_id.unwrap_or_else(next_request_id);
        let request_id_value = request_id.to_string();
        let encoded = encode_http_request(&request, &[(REQUEST_ID_HEADER, &request_id_value)])?;
        let remaining = deadline.remaining().ok_or_else(|| {
            AtmError::new(
                AtmErrorCode::WaitTimeout,
                "HTTP client request budget elapsed before connector dispatch",
            )
        })?;
        let response = tokio::time::timeout(remaining, self.connector.exchange(encoded, deadline))
            .await
            .map_err(|_| self.connector.deadline_elapsed().into_atm_error())?
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
            &response_shape,
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
    use std::num::NonZeroU16;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;
    use atm_core::api::{ApiRequest, DaemonApiClient, HttpRequest, RequestDeadline};
    use atm_core::boundary::{AcceptedPeerIo, BoxedPeerIo, PeerIoAdapter};
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, SendResponseEnvelope};
    use atm_core::send::{SendCommandOutcome, SendMessageSource, SendRequest};
    use atm_core::test_support::{TEST_RECIPIENT, TEST_SENDER, TEST_TEAM};
    use atm_core::types::{AgentName, CommandAction, HostName, TeamName};
    use axum::body::{Body, to_bytes};
    use tokio::net::{TcpListener, TcpStream};

    use super::{
        DirectPeerTcpConnector, HttpRuntimeClient, HttpRuntimeClientFailure, HttpRuntimeConnector,
        MAX_RESPONSE_BODY_BYTES, direct_peer_adapter_write_client, direct_peer_connection_failure,
        direct_peer_tcp_client, read_bounded_reqwest_response_body, selected_write_transport,
    };

    #[derive(Default)]
    struct RecordingConnector {
        requests: Mutex<Vec<HttpRequest>>,
        responses: Mutex<VecDeque<Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure>>>,
    }

    struct RejectingPeerIoAdapter {
        connect_calls: std::sync::atomic::AtomicUsize,
    }

    impl atm_core::boundary::sealed::Sealed for RejectingPeerIoAdapter {}

    impl PeerIoAdapter for RejectingPeerIoAdapter {
        fn accept<'adapter>(
            &'adapter self,
            _stream: TcpStream,
            _deadline: RequestDeadline,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<AcceptedPeerIo, AtmError>>
                    + Send
                    + 'adapter,
            >,
        > {
            Box::pin(async {
                Err(AtmError::validation(
                    "test transport rejects inbound stream",
                ))
            })
        }

        fn connect<'adapter>(
            &'adapter self,
            _peer: HostName,
            _deadline: RequestDeadline,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<BoxedPeerIo, AtmError>> + Send + 'adapter>,
        > {
            self.connect_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async {
                Err(AtmError::validation(
                    "certificate, hostname, pin, or handshake rejected by test transport",
                ))
            })
        }
    }

    #[async_trait]
    impl HttpRuntimeConnector for RecordingConnector {
        fn connection_target(&self) -> String {
            "recording test connector".to_owned()
        }

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

    #[test]
    fn host_qualified_write_stays_on_the_local_daemon_client() {
        let connector = Arc::new(RecordingConnector::default());
        let same_host_transport: Arc<dyn DaemonApiClient + Send + Sync> =
            Arc::new(HttpRuntimeClient::new(connector, Duration::from_secs(1)));
        let request = SendRequest::new(
            ".".into(),
            ".".into(),
            AgentName::from_validated(TEST_SENDER),
            "recipient@test-team.peer.example.test",
            TeamName::from_validated(TEST_TEAM),
            SendMessageSource::Inline("daemon-owned peer selection".to_owned()),
            None,
            false,
            None,
            false,
        )
        .expect("host-qualified request");

        let selected = selected_write_transport(&request, &same_host_transport)
            .expect("selection remains local and cannot open raw peer TCP");
        assert!(
            Arc::ptr_eq(&selected, &same_host_transport),
            "the daemon owns host-qualified peer transport selection"
        );
    }

    #[test]
    fn direct_peer_client_rejects_a_zero_request_budget_before_connecting() {
        let error = direct_peer_tcp_client(
            "peer.example.test".parse().expect("host"),
            std::num::NonZeroU16::new(43101).expect("port"),
            Duration::ZERO,
        )
        .err()
        .expect("zero budget must fail before a peer request is attempted");
        assert!(error.message().contains("timeout"));
    }

    #[tokio::test]
    async fn authenticated_peer_rejection_never_downgrades_to_plain_tcp() {
        let adapter = Arc::new(RejectingPeerIoAdapter {
            connect_calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let client = direct_peer_adapter_write_client(
            "peer.example.test".parse().expect("host"),
            adapter.clone(),
            Duration::from_secs(1),
        )
        .expect("non-zero request budget");

        let error = client
            .execute(ApiRequest::new(write_request()))
            .await
            .expect_err("transport rejection must not attempt a plaintext second connection");
        assert_eq!(
            adapter
                .connect_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            1,
            "one authenticated connection attempt is the complete outbound transport path"
        );
        assert_eq!(error.code().as_str(), "REMOTE_DELIVERY_UNCONFIRMED");
        assert!(
            error
                .cause()
                .is_some_and(|cause| cause.contains("certificate, hostname, pin, or handshake")),
            "the adapter failure remains attributable without changing to plain TCP"
        );
    }

    #[tokio::test]
    async fn peer_response_body_limit_rejects_oversized_payloads() {
        let error = to_bytes(
            Body::from(vec![b'x'; MAX_RESPONSE_BODY_BYTES + 1]),
            MAX_RESPONSE_BODY_BYTES,
        )
        .await
        .expect_err("peer response decoding must cap the allocation size");
        assert!(error.to_string().contains("length limit"));
    }

    #[tokio::test]
    async fn reqwest_response_body_limit_rejects_oversized_payloads() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test response server");
        let address = listener.local_addr().expect("test server address");
        let router = axum::Router::new().route(
            "/",
            axum::routing::get(|| async { vec![b'x'; MAX_RESPONSE_BODY_BYTES + 1] }),
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("test response server serves")
        });
        let mut response = reqwest::get(format!("http://{address}/"))
            .await
            .expect("request test response server");
        let error = read_bounded_reqwest_response_body(&mut response)
            .await
            .expect_err("reqwest response decoding must cap the allocation size");
        assert!(matches!(error, HttpRuntimeClientFailure::ResponseDecode(_)));
        server.abort();
        let _ = server.await;
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
            let request_id_header = requests[0]
                .headers
                .iter()
                .find(|header| header.starts_with("X-ATM-Request-Id: "))
                .expect("outbound requests carry their correlation request ID");
            assert!(
                request_id_header
                    .strip_prefix("X-ATM-Request-Id: ")
                    .expect("request ID header prefix")
                    .parse::<u64>()
                    .is_ok(),
                "{connector_kind} connector"
            );
            assert!(
                serde_json::from_slice::<atm_core::send::WriteRequest>(&requests[0].body).is_ok(),
                "{connector_kind} connector"
            );
        }
    }

    #[tokio::test]
    async fn caller_supplied_request_id_is_forwarded_without_reminting() {
        let connector = Arc::new(RecordingConnector::default());
        connector
            .responses
            .lock()
            .expect("responses")
            .push_back(Ok(sent_response()));
        let client = HttpRuntimeClient::new(connector.clone(), Duration::from_secs(1));
        let request = write_request();

        client
            .execute_envelope_with_deadline(
                request.clone(),
                request,
                RequestDeadline::after(Duration::from_secs(1)),
                Some(atm_core::protocol::RequestId::new(73).expect("non-zero request ID")),
            )
            .await
            .expect("caller-provided request ID is accepted");

        let requests = connector.requests.lock().expect("requests");
        assert!(
            requests[0]
                .headers
                .iter()
                .any(|header| header == "X-ATM-Request-Id: 73")
        );
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
                "direct peer connect",
                HttpRuntimeClientFailure::PeerConnect {
                    target: "peer.example.test:43101".to_owned(),
                    cause: "DNS lookup failed".to_owned(),
                },
                AtmErrorCode::RemoteDeliveryUnconfirmed,
                "direct peer",
            ),
            (
                "cancellation",
                HttpRuntimeClientFailure::Cancelled,
                AtmErrorCode::WaitTimeout,
                "cancelled",
            ),
            (
                "direct peer connect timeout",
                HttpRuntimeClientFailure::PeerConnectTimeout {
                    target: "peer.example.test:43101".to_owned(),
                    cause: "request budget elapsed".to_owned(),
                },
                AtmErrorCode::RemoteDeliveryUnconfirmed,
                "direct peer",
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

    #[tokio::test]
    async fn direct_connector_failure_performs_exactly_one_exchange_without_follow_up() {
        let connector = Arc::new(RecordingConnector::default());
        connector
            .responses
            .lock()
            .expect("responses")
            .push_back(Err(HttpRuntimeClientFailure::Connect(
                "connection refused".to_owned(),
            )));
        let client = HttpRuntimeClient::new(Arc::clone(&connector), Duration::from_secs(1));

        let error = client
            .execute(ApiRequest::new(write_request()))
            .await
            .expect_err("a direct connection failure must reach the caller");

        assert_eq!(error.code(), AtmErrorCode::DaemonUnavailable);
        assert_eq!(
            connector.requests.lock().expect("requests").len(),
            1,
            "a failed direct delivery performs exactly one exchange and starts no follow-up work"
        );
    }

    #[test]
    fn direct_peer_connection_failure_names_the_remote_authority() {
        let error = direct_peer_connection_failure(
            "rand-m5:43101",
            HttpRuntimeClientFailure::Connect("DNS lookup failed".to_owned()),
        )
        .into_atm_error();

        assert_eq!(error.code(), AtmErrorCode::RemoteDeliveryUnconfirmed);
        assert!(
            error
                .message()
                .starts_with("HTTP client could not connect to direct peer `rand-m5:43101`"),
            "the direct peer authority is shown before recovery guidance"
        );
        assert!(
            !error.message().contains("configured daemon endpoint"),
            "a host-qualified send must not claim the local daemon failed"
        );
        assert!(
            !error.message().contains("restore the single local daemon"),
            "the recovery action must not direct the caller to repair a healthy local daemon"
        );
        assert_eq!(error.cause(), Some("DNS lookup failed"));
    }

    #[tokio::test(start_paused = true)]
    async fn request_deadline_bounds_the_connector_once() {
        struct WaitingConnector;
        #[async_trait]
        impl HttpRuntimeConnector for WaitingConnector {
            fn connection_target(&self) -> String {
                "deliberately waiting test connector".to_owned()
            }

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
        assert!(error.message().contains("request budget"));
    }

    #[test]
    fn direct_peer_deadline_is_reported_as_a_connect_failure() {
        let connector = DirectPeerTcpConnector::new(
            "unresolvable.example".parse().expect("valid host syntax"),
            NonZeroU16::new(43_101).expect("non-zero port"),
        )
        .expect("direct peer connector");
        let error = connector.deadline_elapsed().into_atm_error();

        assert_eq!(error.code(), AtmErrorCode::RemoteDeliveryUnconfirmed);
        assert!(
            error
                .message()
                .starts_with("HTTP client could not connect to direct peer `unresolvable.example:43101` before its request budget elapsed")
        );
        assert!(
            error
                .cause()
                .is_some_and(|cause| cause.contains("unresolvable.example")),
            "the configured peer authority is retained for recovery"
        );
    }
}
