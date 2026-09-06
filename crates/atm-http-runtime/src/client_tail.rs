use super::*;

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

    /// Refreshes connector-owned generation state after a failure that is
    /// proven to have happened before request bytes were written. The default
    /// is intentionally a no-op: direct-peer and test connectors must never
    /// turn a remote write into an automatic second attempt.
    async fn prepare_pre_send_reconnect(&self) -> Result<bool, HttpRuntimeClientFailure> {
        Ok(false)
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
pub(crate) struct UnixSocketConnector {
    socket_path: PathBuf,
    transport: RwLock<Option<Arc<GenerationalReqwestClient>>>,
}

#[cfg(unix)]
impl UnixSocketConnector {
    pub(super) fn new(socket_path: &Path) -> Result<Self, AtmError> {
        build_unix_socket_reqwest_client(socket_path)
            .map_err(HttpRuntimeClientFailure::into_atm_error)?;
        Ok(Self {
            socket_path: socket_path.to_path_buf(),
            transport: RwLock::new(None),
        })
    }

    pub(super) fn socket_generation(
        &self,
    ) -> Result<TransportGeneration, HttpRuntimeClientFailure> {
        unix_socket_generation(&self.socket_path)
    }

    pub(super) fn transport_for_generation(
        &self,
        generation: TransportGeneration,
    ) -> Result<Arc<GenerationalReqwestClient>, HttpRuntimeClientFailure> {
        let mut guard = self
            .transport
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(current) = guard
            .as_ref()
            .filter(|cached| cached.generation == generation)
            .map(Arc::clone)
        {
            return Ok(current);
        }
        let client = build_unix_socket_reqwest_client(&self.socket_path)?;
        let fresh = Arc::new(GenerationalReqwestClient { client, generation });
        *guard = Some(Arc::clone(&fresh));
        Ok(fresh)
    }
}

#[cfg(unix)]
fn build_unix_socket_reqwest_client(
    socket_path: &Path,
) -> Result<reqwest::Client, HttpRuntimeClientFailure> {
    reqwest::Client::builder()
        .unix_socket(socket_path)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(LOOPBACK_CONNECT_TIMEOUT)
        .build()
        .map_err(|source| {
            HttpRuntimeClientFailure::Connect(format!("failed to build Unix HTTP client: {source}"))
        })
}

#[cfg(unix)]
fn unix_socket_generation(
    socket_path: &Path,
) -> Result<TransportGeneration, HttpRuntimeClientFailure> {
    let metadata = std::fs::symlink_metadata(socket_path).map_err(|source| {
        HttpRuntimeClientFailure::EndpointRecord(
            AtmError::daemon_unavailable("failed to inspect the Unix HTTP socket")
                .with_cause(source),
        )
    })?;
    Ok(TransportGeneration::UnixSocket {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(unix)]
#[async_trait]
impl HttpRuntimeConnector for UnixSocketConnector {
    fn connection_target(&self) -> String {
        format!("Unix socket `{}`", self.socket_path.display())
    }

    fn deadline_elapsed(&self) -> HttpRuntimeClientFailure {
        local_request_timeout_failure(&self.connection_target())
    }

    async fn exchange(
        &self,
        request: HttpRequest,
        deadline: RequestDeadline,
    ) -> Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure> {
        self.exchange_once(request, deadline).await
    }

    async fn prepare_pre_send_reconnect(&self) -> Result<bool, HttpRuntimeClientFailure> {
        let generation = self.socket_generation()?;
        self.transport_for_generation(generation)?;
        Ok(true)
    }
}

#[cfg(unix)]
impl UnixSocketConnector {
    async fn exchange_once(
        &self,
        request: HttpRequest,
        deadline: RequestDeadline,
    ) -> Result<axum::http::Response<Vec<u8>>, HttpRuntimeClientFailure> {
        let generation = self.socket_generation()?;
        let transport = self.transport_for_generation(generation)?;
        let url = reqwest::Url::parse(&format!("http://localhost{}", request.path)).map_err(
            |source| {
                HttpRuntimeClientFailure::RequestWrite(format!(
                    "shared HTTP request has an invalid route `{}`: {source}",
                    request.path
                ))
            },
        )?;
        execute_reqwest_request(&transport.client, url, request, deadline, None).await
    }
}

pub(crate) async fn execute_reqwest_request(
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

pub(crate) fn local_request_timeout_failure(connection_target: &str) -> HttpRuntimeClientFailure {
    HttpRuntimeClientFailure::RequestWrite(format!(
        "{connection_target} request exceeded its absolute request budget after dispatch"
    ))
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
/// partially or fully transmitted, so it must stay in a non-reconnect-eligible
/// variant -- see `HttpRuntimeClientFailure::is_safe_to_reconnect`.
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
    pub(super) request_timeout: Duration,
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
    pub(super) async fn execute_envelope_with_deadline(
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
        let response = tokio::time::timeout(remaining, async {
            match self.connector.exchange(encoded.clone(), deadline).await {
                Ok(response) => Ok(response),
                Err(failure) if failure.is_safe_to_reconnect() => {
                    if self.connector.prepare_pre_send_reconnect().await? {
                        self.connector.exchange(encoded, deadline).await
                    } else {
                        Err(failure)
                    }
                }
                Err(failure) => Err(failure),
            }
        })
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
