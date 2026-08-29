//! Connector-neutral client-side HTTP translation.
//!
//! This module owns no socket, DNS, TLS, or listener setup.  Physical
//! connectors are deliberately introduced by AL.5--AL.7.  It owns the one
//! route-body encoder and result decoder used after a connector is selected.

use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

use async_trait::async_trait;
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
use axum::body::Body;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::client::conn::http1;

use reqwest::header::{HeaderName, HeaderValue};
use ulid::Ulid;

use crate::PeerConnectionPool;

/// Fixed plain-TCP port for direct peer writes.
///
/// A host-qualified recipient supplies the remote authority from peer
/// storage.  Local bind addresses and ports are never operator configuration:
/// every replacement daemon owns this protocol port on its active IPv4
/// interfaces.
pub const DIRECT_PEER_TCP_PORT: u16 = 43_101;
const REQUEST_ID_HEADER: &str = "X-ATM-Request-Id";

/// One bounded budget for the selected write connector.
///
/// The daemon owns this direct-plaintext budget after the CLI or graft
/// submits the canonical request through its ordinary local transport.
///
/// This is `atm_core::request_budget::SAME_HOST_REQUEST_DEADLINE`, the
/// shared same-host client budget derived from the server's own request
/// budget plus its response handoff grace. See that module for the full
/// client/server budget contract and why the two must never be equal.
pub const SAME_HOST_REQUEST_DEADLINE: Duration =
    atm_core::request_budget::SAME_HOST_REQUEST_DEADLINE;

/// Returns the fixed direct-peer protocol port.
#[must_use]
pub fn direct_peer_port() -> NonZeroU16 {
    NonZeroU16::new(DIRECT_PEER_TCP_PORT).expect("direct peer port is non-zero")
}

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
    /// Returns `true` when this failure provably occurred before any bytes
    /// of the request could have left the client, so retrying it exactly
    /// once against a freshly re-resolved, freshly connected transport can
    /// never duplicate a write the server may already have received.
    ///
    /// Only [`Self::Connect`] qualifies: it is raised solely for a
    /// connection-establishment failure (including a bounded
    /// `connect_timeout` expiry), which by construction happens strictly
    /// before any request bytes are written. Every other variant means a
    /// request was at least partially transmitted, or a response was
    /// already read, and must never be retried -- see the no-duplicate-write
    /// contract in [`HttpRuntimeConnector::exchange`].
    fn is_safe_to_retry_after_reconnect(&self) -> bool {
        matches!(self, Self::Connect(_))
    }

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

/// Builds the shared typed client for one explicitly configured direct peer.
///
/// The physical authority is the only peer-specific input. The existing
/// [`HttpRuntimeClient`] remains responsible for request encoding, the
/// canonical route, deadline enforcement, response decoding, and errors.
/// This adapter deliberately adds neither a peer DTO nor delivery recovery.
pub fn direct_peer_tcp_client(
    host: HostName,
    port: NonZeroU16,
    request_timeout: Duration,
) -> Result<Arc<dyn DaemonApiClient + Send + Sync>, AtmError> {
    direct_peer_tcp_client_with_shared_client(
        host,
        port,
        request_timeout,
        shared_direct_peer_client()?,
    )
}

/// Builds the process-owned plaintext peer client. Cloning this handle is
/// cheap and keeps reqwest's connection pool alive across canonical writes.
pub fn shared_direct_peer_client() -> Result<reqwest::Client, AtmError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|source| {
            AtmError::config("failed to build direct peer HTTP client").with_cause(source)
        })
}

/// Builds one typed peer-write facade using the daemon-owned plaintext client.
pub(crate) fn direct_peer_tcp_client_with_shared_client(
    host: HostName,
    port: NonZeroU16,
    request_timeout: Duration,
    client: reqwest::Client,
) -> Result<Arc<dyn DaemonApiClient + Send + Sync>, AtmError> {
    Ok(Arc::new(direct_peer_write_client(
        host,
        port,
        request_timeout,
        client,
    )?))
}

/// Builds the typed peer-write client over the daemon-owned opaque-stream
/// pool. The pool key is the configured host authority; TLS remains outside
/// this crate behind the established-stream adapter seam.
pub(crate) fn pooled_peer_stream_write_client(
    host: HostName,
    port: NonZeroU16,
    request_timeout: Duration,
    pool: PeerConnectionPool,
) -> Result<Arc<dyn DaemonApiClient + Send + Sync>, AtmError> {
    if request_timeout.is_zero() {
        return Err(AtmError::config(
            "direct peer HTTP client request timeout must be greater than zero",
        ));
    }
    let connector = PooledPeerStreamConnector::new(host, port, pool);
    let client = Arc::new(HttpRuntimeClient::new(Arc::new(connector), request_timeout));
    Ok(Arc::new(PeerStreamWriteClient { client }))
}

pub(crate) fn direct_peer_write_client(
    host: HostName,
    port: NonZeroU16,
    request_timeout: Duration,
    client: reqwest::Client,
) -> Result<DirectPeerWriteClient, AtmError> {
    if request_timeout.is_zero() {
        return Err(AtmError::config(
            "direct peer HTTP client request timeout must be greater than zero",
        ));
    }
    let connector = DirectPeerTcpConnector::new(client, host, port);
    let client = Arc::new(HttpRuntimeClient::new(Arc::new(connector), request_timeout));
    Ok(DirectPeerWriteClient { client })
}

/// Selects the owner-authorized local daemon for every canonical write.
///
/// Host-qualified addressing remains part of the canonical request, but the
/// CLI and graft never select a peer socket themselves. The selected daemon
/// owns the one peer-wire mode and forwards the admitted write through either
/// the unchanged plaintext connector or its bootstrap-composed opaque stream
/// adapter.
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
///
/// A managed daemon restart (`atm daemon restart`/switch) always rebinds a
/// fresh ephemeral loopback listener and publishes a new
/// [`LocalHttpEndpointRecord::daemon_instance_id`] for it. A long-lived
/// embedded client (for example the Python graft binding's `PyGraftSession`)
/// keeps this connector, and therefore its underlying `reqwest::Client`
/// connection pool, alive across that restart. Without a generation check, a
/// pooled connection established against the pre-restart daemon can outlive
/// the process it was connected to; on Windows, writing to (or waiting on a
/// response from) that stale connection has been observed to silently stall
/// instead of failing fast, consuming the entire same-host request budget.
/// `transport` therefore caches the client alongside the daemon generation
/// it was built for, and is rebuilt -- discarding any pooled connections --
/// whenever the freshly read record names a different generation.
#[derive(Debug)]
struct LoopbackTcpConnector {
    endpoint_record_path: PathBuf,
    transport: RwLock<Option<Arc<GenerationalReqwestClient>>>,
}

/// A same-host `reqwest::Client` bound to the daemon generation it was built
/// for. See [`LoopbackTcpConnector`] for why this binding matters.
#[derive(Debug)]
struct GenerationalReqwestClient {
    client: reqwest::Client,
    daemon_instance_id: Ulid,
}

/// Upper bound on establishing the loopback TCP connection to the daemon.
///
/// Same-host `connect()` to an actually-listening daemon completes in well
/// under a millisecond. This bound exists only to fail a connect attempt
/// fast when the client is racing a managed daemon restart, rather than
/// silently consuming the rest of [`SAME_HOST_REQUEST_DEADLINE`] on a
/// connect that a stale pooled route can never complete. It is kept an
/// order of magnitude below [`atm_core::request_budget::SERVER_REQUEST_BUDGET`]
/// so even a legitimately slow (but real) connect leaves most of the
/// request budget for the server itself.
const LOOPBACK_CONNECT_TIMEOUT: Duration = Duration::from_millis(750);

/// Builds one loopback `reqwest::Client` bound to no particular daemon
/// generation yet. Callers must pair the result with the
/// [`Ulid`] read from the same [`LocalHttpEndpointRecord`] exchange that
/// motivated the build.
fn build_loopback_reqwest_client() -> Result<reqwest::Client, HttpRuntimeClientFailure> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(LOOPBACK_CONNECT_TIMEOUT)
        .build()
        .map_err(|source| {
            HttpRuntimeClientFailure::Connect(format!(
                "failed to build loopback HTTP client: {source}"
            ))
        })
}

/// Reqwest owns DNS, connection and HTTP. This adapter owns only the
/// configured peer authority; it does not duplicate ATM request processing.
#[derive(Debug)]
struct DirectPeerTcpConnector {
    client: reqwest::Client,
    authority: String,
}

/// Connector facade which keeps request translation and public error mapping
/// in the existing shared `HttpRuntimeClient` boundary.
struct PooledPeerStreamConnector {
    authority: String,
    host: HostName,
    port: NonZeroU16,
    pool: PeerConnectionPool,
}

/// Send-only peer adapter over the shared typed HTTP client.
///
/// The client owns only sender-side provenance completion.  It never changes
/// the encoded DTO shape or performs a second request: the one
/// [`HttpRuntimeClient`] below still encodes, sends, and decodes the complete
/// write.  Supplying a caller-originated provenance pair is supported for
/// handoff from an origin writer; otherwise the first peer boundary creates
/// the immutable pair once.
pub(crate) struct DirectPeerWriteClient {
    client: Arc<HttpRuntimeClient<DirectPeerTcpConnector>>,
}

/// Mirrors the direct peer's one-write provenance completion while replacing
/// only connection establishment with the selected opaque stream adapter.
struct PeerStreamWriteClient<Connector> {
    client: Arc<HttpRuntimeClient<Connector>>,
}

impl<Connector> boundary::sealed::Sealed for PeerStreamWriteClient<Connector> where
    Connector: Send + Sync
{
}

#[async_trait]
impl<Connector> DaemonApiClient for PeerStreamWriteClient<Connector>
where
    Connector: HttpRuntimeConnector + 'static,
{
    async fn execute(&self, request: ApiRequest) -> Result<ApiResponse, AtmError> {
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
        let mut peer_response_shape = write.clone();
        peer_response_shape.acknowledges_message_id = None;
        self.client
            .execute_envelope_with_deadline(
                RequestEnvelope::Write(Box::new(write)),
                RequestEnvelope::Write(Box::new(peer_response_shape)),
                RequestDeadline::after(self.client.request_timeout),
                None,
            )
            .await
    }
}

impl boundary::sealed::Sealed for DirectPeerWriteClient {}

#[async_trait]
impl DaemonApiClient for DirectPeerWriteClient {
    async fn execute(&self, request: ApiRequest) -> Result<ApiResponse, AtmError> {
        self.execute_with_optional_request_id(request, None).await
    }
}

impl DirectPeerWriteClient {
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

impl DirectPeerTcpConnector {
    fn new(client: reqwest::Client, host: HostName, port: NonZeroU16) -> Self {
        Self {
            authority: if host.as_str().contains(':') {
                format!("[{}]:{}", host.as_str(), port)
            } else {
                format!("{}:{}", host.as_str(), port)
            },
            client,
        }
    }
}

impl PooledPeerStreamConnector {
    fn new(host: HostName, port: NonZeroU16, pool: PeerConnectionPool) -> Self {
        let authority = direct_peer_authority(&host, port);
        Self {
            authority,
            host,
            port,
            pool,
        }
    }
}

#[async_trait]
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
impl HttpRuntimeConnector for PooledPeerStreamConnector {
    fn connection_target(&self) -> String {
        format!("direct peer `{}`", self.authority)
    }

    fn deadline_elapsed(&self) -> HttpRuntimeClientFailure {
        peer_connect_deadline_failure(&self.authority, &self.connection_target())
    }

    async fn exchange(
        &self,
        request: HttpRequest,
        deadline: RequestDeadline,
    ) -> Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure> {
        let mut connection = self.pool.acquire(&self.host, self.port, deadline).await?;
        connection.exchange(request, deadline).await
    }
}

/// Executes one canonical HTTP request over an already-negotiated HTTP/1
/// sender. This is the pooled counterpart to `execute_opaque_peer_request`:
/// it deliberately reuses the exact request encoding and response decoding
/// behavior while skipping only TCP/TLS and HTTP/1 establishment.
pub(crate) async fn execute_opaque_peer_request_with_sender(
    sender: &mut http1::SendRequest<Body>,
    request: HttpRequest,
    deadline: RequestDeadline,
) -> Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure> {
    let request = build_opaque_peer_request(request)?;
    let remaining = deadline
        .remaining()
        .ok_or(HttpRuntimeClientFailure::Cancelled)?;
    let response = tokio::time::timeout(remaining, sender.send_request(request))
        .await
        .map_err(|_| HttpRuntimeClientFailure::Timeout)?
        .map_err(|source| HttpRuntimeClientFailure::Connect(source.to_string()))?;
    collect_opaque_peer_response(response).await
}

fn build_opaque_peer_request(
    request: HttpRequest,
) -> Result<axum::http::Request<Body>, HttpRuntimeClientFailure> {
    let method: axum::http::Method = request.method.parse().map_err(|source| {
        HttpRuntimeClientFailure::RequestWrite(format!(
            "shared HTTP request has an invalid method `{}`: {source}",
            request.method
        ))
    })?;
    let mut builder = axum::http::Request::builder()
        .method(method)
        .uri(&request.path);
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
        builder = builder.header(name, value);
    }
    builder.body(Body::from(request.body)).map_err(|source| {
        HttpRuntimeClientFailure::RequestWrite(format!(
            "shared HTTP request could not be built for opaque peer stream: {source}"
        ))
    })
}

async fn collect_opaque_peer_response(
    response: axum::http::Response<Incoming>,
) -> Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure> {
    let (parts, body) = response.into_parts();
    let body = body
        .collect()
        .await
        .map_err(|source| {
            HttpRuntimeClientFailure::ResponseDecode(
                AtmError::daemon_unavailable("failed to read opaque peer HTTP response body")
                    .with_cause(source),
            )
        })?
        .to_bytes();
    Ok(axum::http::Response::from_parts(parts, body.to_vec()))
}

/// Preserve the direct-peer authority when the shared HTTP exchange fails
/// before a response.  A host-qualified recipient never uses the local daemon
/// endpoint, so reporting this as local daemon unavailability is misleading.
pub(crate) fn direct_peer_connection_failure(
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

pub(crate) fn direct_peer_authority(host: &HostName, port: NonZeroU16) -> String {
    if host.as_str().contains(':') {
        format!("[{}]:{}", host.as_str(), port)
    } else {
        format!("{}:{}", host.as_str(), port)
    }
}

pub(crate) fn peer_connect_deadline_failure(
    authority: &str,
    connection_target: &str,
) -> HttpRuntimeClientFailure {
    HttpRuntimeClientFailure::PeerConnectTimeout {
        target: authority.to_owned(),
        cause: format!(
            "{connection_target} did not resolve or establish a connection before the configured request deadline"
        ),
    }
}

impl LoopbackTcpConnector {
    fn new(endpoint_record_path: &Path) -> Result<Self, AtmError> {
        // Build-and-discard once so a client construction error (for
        // example an unsupported TLS backend) still fails fast at connector
        // construction time instead of surfacing from the first `exchange`
        // call. The cached transport starts empty; it is built for real,
        // and bound to a daemon generation, on first use.
        build_loopback_reqwest_client().map_err(HttpRuntimeClientFailure::into_atm_error)?;
        Ok(Self {
            endpoint_record_path: endpoint_record_path.to_path_buf(),
            transport: RwLock::new(None),
        })
    }

    /// Returns the cached transport if it was built for `daemon_instance_id`,
    /// otherwise builds and caches a fresh one, discarding any pooled
    /// connections the previous transport held open.
    ///
    /// This is the proactive half of the stale-connection fix: it guarantees
    /// a request is never issued over a connection pool that may still hold
    /// a socket connected to a since-replaced daemon process.
    fn transport_for_generation(
        &self,
        daemon_instance_id: Ulid,
    ) -> Result<Arc<GenerationalReqwestClient>, HttpRuntimeClientFailure> {
        if let Some(current) = self.cached_transport_if_matching(daemon_instance_id) {
            return Ok(current);
        }
        self.rebuild_transport(daemon_instance_id)
    }

    fn cached_transport_if_matching(
        &self,
        daemon_instance_id: Ulid,
    ) -> Option<Arc<GenerationalReqwestClient>> {
        let guard = self
            .transport
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard
            .as_ref()
            .filter(|cached| cached.daemon_instance_id == daemon_instance_id)
            .map(Arc::clone)
    }

    /// Unconditionally builds and caches a fresh transport for
    /// `daemon_instance_id`, replacing whatever was cached before.
    ///
    /// Used both when the cached generation does not match (proactive
    /// refresh) and, once, after a provably pre-send connect failure
    /// (reactive refresh); see [`HttpRuntimeConnector::exchange`].
    fn rebuild_transport(
        &self,
        daemon_instance_id: Ulid,
    ) -> Result<Arc<GenerationalReqwestClient>, HttpRuntimeClientFailure> {
        let client = build_loopback_reqwest_client()?;
        let fresh = Arc::new(GenerationalReqwestClient {
            client,
            daemon_instance_id,
        });
        let mut guard = self
            .transport
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = Some(Arc::clone(&fresh));
        Ok(fresh)
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

    /// Exchanges one request, retrying exactly once after a provably
    /// pre-send connect failure.
    ///
    /// No-duplicate-write contract: the retry only happens for
    /// [`HttpRuntimeClientFailure::is_safe_to_retry_after_reconnect`]
    /// failures, i.e. failures raised strictly before any request byte left
    /// the client. A retry re-reads the endpoint record (picking up a
    /// concurrently completed daemon restart) and forces a fresh transport,
    /// so it can never reuse the connection that just failed.
    async fn exchange(
        &self,
        request: HttpRequest,
        deadline: RequestDeadline,
    ) -> Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure> {
        match self.exchange_once(request.clone(), deadline).await {
            Ok(response) => Ok(response),
            Err(failure) if failure.is_safe_to_retry_after_reconnect() => {
                let daemon_instance_id =
                    load_active_loopback_daemon_instance_id(&self.endpoint_record_path).await?;
                self.rebuild_transport(daemon_instance_id)?;
                self.exchange_once(request, deadline).await
            }
            Err(failure) => Err(failure),
        }
    }
}

impl LoopbackTcpConnector {
    async fn exchange_once(
        &self,
        request: HttpRequest,
        deadline: RequestDeadline,
    ) -> Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure> {
        let (endpoint, capability, daemon_instance_id) =
            load_active_loopback_endpoint(&self.endpoint_record_path).await?;
        let transport = self.transport_for_generation(daemon_instance_id)?;
        let url = reqwest::Url::parse(&format!("http://{endpoint}{}", request.path)).map_err(
            |source| {
                HttpRuntimeClientFailure::RequestWrite(format!(
                    "shared HTTP request has an invalid loopback route `{}`: {source}",
                    request.path
                ))
            },
        )?;
        execute_reqwest_request(
            &transport.client,
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
) -> Result<(std::net::SocketAddr, LocalCapability, Ulid), HttpRuntimeClientFailure> {
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

/// Re-reads only the daemon generation from the local endpoint record.
///
/// Used exclusively by the one post-connect-failure retry in
/// [`LoopbackTcpConnector::exchange`] so the forced transport rebuild binds
/// to whatever generation is current at retry time, not the generation that
/// just failed.
async fn load_active_loopback_daemon_instance_id(
    endpoint_record_path: &Path,
) -> Result<Ulid, HttpRuntimeClientFailure> {
    load_active_loopback_endpoint(endpoint_record_path)
        .await
        .map(|(_, _, daemon_instance_id)| daemon_instance_id)
}

fn load_active_loopback_endpoint_blocking(
    endpoint_record_path: &Path,
) -> Result<(std::net::SocketAddr, LocalCapability, Ulid), AtmError> {
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
    Ok((endpoint, capability, record.daemon_instance_id))
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
    let outbound = build_outbound_reqwest_request(url, request, additional_header)?;
    let response = client
        .execute(outbound)
        .await
        .map_err(classify_reqwest_execute_failure)?;
    decode_reqwest_response(response).await
}

/// Builds the outbound `reqwest::Request` from the shared, transport-agnostic
/// [`HttpRequest`], applying the connector-specific `additional_header` (for
/// example the loopback capability header) last.
fn build_outbound_reqwest_request(
    url: reqwest::Url,
    request: HttpRequest,
    additional_header: Option<(&'static str, String)>,
) -> Result<reqwest::Request, HttpRuntimeClientFailure> {
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
    Ok(outbound)
}

/// Classifies a `reqwest::Client::execute` failure for the no-duplicate-write
/// contract. `reqwest::Error::is_connect` is `true` only for connection
/// establishment failures (DNS/TCP/TLS, including a configured
/// `connect_timeout` expiry), which happen strictly before any request byte
/// is written. Every other `execute` failure means the request may have been
/// partially or fully transmitted, so it must stay in a non-retry-eligible
/// variant -- see `HttpRuntimeClientFailure::is_safe_to_retry_after_reconnect`.
fn classify_reqwest_execute_failure(source: reqwest::Error) -> HttpRuntimeClientFailure {
    if source.is_connect() {
        HttpRuntimeClientFailure::Connect(format!(
            "HTTP connector could not establish a connection: {source}"
        ))
    } else {
        HttpRuntimeClientFailure::RequestWrite(format!(
            "HTTP connector request failed after a connection was established: {source}"
        ))
    }
}

/// Decodes a successfully executed `reqwest::Response` into the shared,
/// transport-agnostic response type.
async fn decode_reqwest_response(
    response: reqwest::Response,
) -> Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure> {
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;
    use atm_core::api::{ApiRequest, DaemonApiClient, HttpRequest, RequestDeadline};
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, SendResponseEnvelope};
    use atm_core::send::{SendCommandOutcome, SendMessageSource, SendRequest};
    use atm_core::test_support::{TEST_RECIPIENT, TEST_SENDER, TEST_TEAM};
    use atm_core::types::{AgentName, CommandAction, TeamName};
    use axum::Router;
    use axum::http::StatusCode;
    use axum::routing::post;
    use tokio::net::TcpListener;
    use tokio::sync::watch;

    use super::{
        DirectPeerTcpConnector, HttpRuntimeClient, HttpRuntimeClientFailure, HttpRuntimeConnector,
        direct_peer_connection_failure, direct_peer_tcp_client, execute_reqwest_request,
        selected_write_transport, shared_direct_peer_client,
    };

    struct LocalOnlyClient;

    impl atm_core::boundary::sealed::Sealed for LocalOnlyClient {}

    #[async_trait]
    impl DaemonApiClient for LocalOnlyClient {
        async fn execute(
            &self,
            _request: ApiRequest,
        ) -> Result<atm_core::api::ApiResponse, AtmError> {
            Err(AtmError::daemon_unavailable(
                "test local transport is not invoked",
            ))
        }
    }

    #[derive(Default)]
    struct RecordingConnector {
        requests: Mutex<Vec<HttpRequest>>,
        responses: Mutex<VecDeque<Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure>>>,
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

    #[test]
    fn host_qualified_write_stays_on_the_same_host_daemon_transport() {
        let mut request = match write_request() {
            RequestEnvelope::Write(request) => *request,
            _ => unreachable!("fixture is a write"),
        };
        request.to = Some(
            "recipient@test-team.rand-m5"
                .parse()
                .expect("host-qualified fixture"),
        );
        let same_host: Arc<dyn DaemonApiClient + Send + Sync> = Arc::new(LocalOnlyClient);

        let selected = selected_write_transport(&request, &same_host)
            .expect("host qualification must not make the CLI or graft open a socket");
        assert!(
            Arc::ptr_eq(&selected, &same_host),
            "daemon composition, not the caller, selects the peer-wire transport"
        );
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

    async fn start_plaintext_peer_with_connection_counter() -> (u16, Arc<AtomicUsize>) {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("plaintext peer listener binds");
        let port = listener.local_addr().expect("listener address").port();
        let accepted = Arc::new(AtomicUsize::new(0));
        let accepted_for_task = Arc::clone(&accepted);
        tokio::spawn(async move {
            let router =
                Router::new().route("/v1/atm/messages", post(|| async { StatusCode::CREATED }));
            loop {
                let (stream, _) = listener.accept().await.expect("plaintext peer accepts");
                accepted_for_task.fetch_add(1, Ordering::SeqCst);
                let router = router.clone();
                let (shutdown_tx, shutdown_rx) = watch::channel(());
                tokio::spawn(async move {
                    let _shutdown_tx = shutdown_tx;
                    let _ = crate::http1_server::serve_connection(
                        hyper_util::rt::TokioIo::new(stream),
                        router,
                        Duration::from_secs(30),
                        shutdown_rx,
                    )
                    .await;
                });
            }
        });
        (port, accepted)
    }

    #[tokio::test]
    async fn shared_plaintext_client_reuses_the_daemon_lifetime_connection_pool() {
        let (port, accepted) = start_plaintext_peer_with_connection_counter().await;
        let connector = DirectPeerTcpConnector::new(
            shared_direct_peer_client().expect("shared plaintext client"),
            "127.0.0.1".parse().expect("peer host"),
            NonZeroU16::new(port).expect("non-zero test port"),
        );
        let request = HttpRequest {
            method: "POST".to_owned(),
            path: "/v1/atm/messages".to_owned(),
            headers: Vec::new(),
            body: Vec::new(),
        };
        for _ in 0..3 {
            let response = connector
                .exchange(
                    request.clone(),
                    RequestDeadline::after(Duration::from_secs(1)),
                )
                .await
                .expect("shared plaintext connection writes successfully");
            assert_eq!(response.status(), StatusCode::CREATED);
        }
        assert_eq!(
            accepted.load(Ordering::SeqCst),
            1,
            "one reqwest client keeps its established connection for sequential writes"
        );
    }

    #[test]
    fn only_connect_failures_are_safe_to_retry_after_reconnect() {
        let retry_eligible = HttpRuntimeClientFailure::Connect("refused".to_owned());
        assert!(retry_eligible.is_safe_to_retry_after_reconnect());

        let never_retried = [
            HttpRuntimeClientFailure::EndpointRecord(AtmError::daemon_unavailable("missing")),
            HttpRuntimeClientFailure::PeerConnect {
                target: "peer:1".to_owned(),
                cause: "refused".to_owned(),
            },
            HttpRuntimeClientFailure::RequestWrite("reset mid-write".to_owned()),
            HttpRuntimeClientFailure::ResponseDecode(AtmError::daemon_unavailable("bad body")),
            HttpRuntimeClientFailure::Cancelled,
            HttpRuntimeClientFailure::Timeout,
            HttpRuntimeClientFailure::PeerConnectTimeout {
                target: "peer:1".to_owned(),
                cause: "slow".to_owned(),
            },
        ];
        for failure in never_retried {
            assert!(
                !failure.is_safe_to_retry_after_reconnect(),
                "{failure:?} may mean request bytes already left the client and must never be retried"
            );
        }
    }

    /// A request against a port nobody listens on must fail during TCP
    /// connect, strictly before any request byte is written -- the one
    /// failure class the loopback connector is allowed to retry after
    /// forcing a fresh transport.
    #[tokio::test]
    async fn a_refused_connection_is_classified_as_a_safe_to_retry_connect_failure() {
        let refused_port = {
            let listener = TcpListener::bind(("127.0.0.1", 0))
                .await
                .expect("reserve an ephemeral port");
            listener.local_addr().expect("listener address").port()
            // The listener is dropped here without ever calling `accept`,
            // so the OS refuses the next connection attempt to this port.
        };
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_millis(500))
            .build()
            .expect("build a bounded test client");
        let url = reqwest::Url::parse(&format!("http://127.0.0.1:{refused_port}/v1/atm/messages"))
            .expect("valid loopback url");
        let request = HttpRequest {
            method: "POST".to_owned(),
            path: "/v1/atm/messages".to_owned(),
            headers: Vec::new(),
            body: Vec::new(),
        };

        let failure = execute_reqwest_request(
            &client,
            url,
            request,
            RequestDeadline::after(Duration::from_secs(1)),
            None,
        )
        .await
        .expect_err("nothing listens on the reserved port");

        assert!(
            matches!(failure, HttpRuntimeClientFailure::Connect(_)),
            "a refused connection must classify as Connect, got {failure:?}"
        );
        assert!(failure.is_safe_to_retry_after_reconnect());
    }

    /// A peer that accepts the TCP connection and then closes it without
    /// ever answering must NOT classify as a connect failure: by the time
    /// the connection was accepted, the request may already have been
    /// written, so retrying it could duplicate a write the peer already
    /// received.
    #[tokio::test]
    async fn a_connection_closed_after_accept_is_not_classified_as_a_safe_to_retry_failure() {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind a test listener");
        let port = listener.local_addr().expect("listener address").port();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept one connection");
            // Immediately drop the accepted stream without reading or
            // writing anything, simulating a peer that vanished the instant
            // after the TCP handshake completed.
            drop(stream);
        });
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_millis(500))
            .build()
            .expect("build a bounded test client");
        let url = reqwest::Url::parse(&format!("http://127.0.0.1:{port}/v1/atm/messages"))
            .expect("valid loopback url");
        let request = HttpRequest {
            method: "POST".to_owned(),
            path: "/v1/atm/messages".to_owned(),
            headers: Vec::new(),
            body: Vec::new(),
        };

        let failure = execute_reqwest_request(
            &client,
            url,
            request,
            RequestDeadline::after(Duration::from_secs(1)),
            None,
        )
        .await
        .expect_err("the peer closes the connection without responding");

        assert!(
            !matches!(failure, HttpRuntimeClientFailure::Connect(_)),
            "a post-accept closure must not classify as Connect, got {failure:?}"
        );
        assert!(!failure.is_safe_to_retry_after_reconnect());
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
                std::future::pending::<()>().await;
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
            shared_direct_peer_client().expect("shared peer client"),
            "unresolvable.example".parse().expect("valid host syntax"),
            NonZeroU16::new(43_101).expect("non-zero port"),
        );
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
