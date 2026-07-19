use std::net::SocketAddr;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[cfg(test)]
use atm_core::boundary::ClientTransport;
#[cfg(test)]
use atm_core::boundary::RemoteReplayStateRecord;
use atm_core::boundary::{self, AtmProtocol, MessageKey, RemoteReplayStore, RequestDispatcher};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::protocol::{JsonAtmProtocolCodec, RequestEnvelope, ResponseEnvelope};
use atm_core::send::RemoteTargetHost;
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use atm_storage::{AllowedHostName, AllowedHostStore, PeerSecurityMode, PeerSecurityStore};

use crate::runtime_status_cache::RuntimeStatusCache;
use crate::{DaemonSubsystem, SubsystemObservability};

mod client_helpers;
mod config_helpers;
pub(crate) mod delivery;
mod replay_resume_worker;
mod security;
mod server;
#[cfg(test)]
mod tests;
mod types;

use crate::remote_replay::{
    complete_replay_record, expire_replay_record, fail_replay_record_terminal,
    replay_error_is_terminal, retain_replay_record,
};
use client_helpers::{
    daemon_terminate_flag, peer_closed_before_response_error, peer_flush_error,
    peer_read_deadline_error, peer_response_decode_error, peer_response_id_mismatch_error,
    peer_write_deadline_error,
};
use delivery::RemoteEndpointResolver;
use security::load_peer_security_mode;
use server::PeerServerTransport;
use types::ReplayResumeSummary;
pub(crate) use types::ReplayResumeWorkerHandle;

// Architecture authority: docs/architecture.md §21.6.4 daemon operational
// defaults and remote peer transport rules.
// These deadlines are intentionally fixed operational constants. Phase Y keeps
// connect and I/O timeouts non-configurable so remote peer delivery behavior
// stays bounded and auditable across every host.
const PEER_CONNECT_DEADLINE: Duration = Duration::from_secs(5);
const PEER_IO_DEADLINE: Duration = Duration::from_secs(5);
const FOREGROUND_REMOTE_WAIT_BUDGET: Duration = Duration::from_secs(10);
const REPLAY_RESUME_POLL_INTERVAL: Duration = Duration::from_secs(5);
const MAX_REMOTE_REPLAY_RESUME_RECORDS: usize = 10_000;
const PEER_LISTENER_ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const PEER_REQUEST_DEADLINE: Duration = Duration::from_secs(3);
const PEER_CONNECTION_IO_SLICE: Duration = Duration::from_millis(200);
const PEER_LISTENER_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(3);
const PEER_ACCEPT_ERROR_RETRY_BACKOFF: Duration = Duration::from_millis(200);
const MAX_CONCURRENT_PEER_CONNECTIONS: usize = 64;
const MAX_TRACKED_PEER_DISPATCH_HANDLES: usize = 256;

#[cfg(test)]
#[derive(Debug)]
struct StaticRemoteEndpointResolver {
    endpoint: SocketAddr,
}

#[cfg(test)]
impl boundary::sealed::Sealed for StaticRemoteEndpointResolver {}

#[cfg(test)]
impl RemoteEndpointResolver for StaticRemoteEndpointResolver {
    fn resolve_endpoint(
        &self,
        _remote_host: &RemoteTargetHost,
        _bound_addr_hint: Option<SocketAddr>,
    ) -> Result<SocketAddr, delivery::CrossHostDeliveryInfraError> {
        Ok(self.endpoint)
    }
}

trait PeerAuthorizationPolicy: Send + Sync {
    fn authorize(&self, peer_addr: SocketAddr) -> Result<(), AtmError>;
}

#[derive(Debug, Default)]
struct AllowAllPeerAuthorizationPolicy;

impl PeerAuthorizationPolicy for AllowAllPeerAuthorizationPolicy {
    fn authorize(&self, _peer_addr: SocketAddr) -> Result<(), AtmError> {
        Ok(())
    }
}

struct SqliteAllowedHostAuthorizationPolicy {
    store: Arc<dyn AllowedHostStore + Send + Sync>,
}

impl SqliteAllowedHostAuthorizationPolicy {
    fn new(store: Arc<dyn AllowedHostStore + Send + Sync>) -> Self {
        Self { store }
    }
}

impl std::fmt::Debug for SqliteAllowedHostAuthorizationPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteAllowedHostAuthorizationPolicy")
            .field("store", &"dyn AllowedHostStore")
            .finish()
    }
}

impl PeerAuthorizationPolicy for SqliteAllowedHostAuthorizationPolicy {
    fn authorize(&self, peer_addr: SocketAddr) -> Result<(), AtmError> {
        let host_name = AllowedHostName::new(peer_addr.ip().to_string()).map_err(|error| {
            AtmError::new_with_code(
                AtmErrorCode::MessageValidationFailed,
                atm_core::error::AtmErrorKind::Validation,
                format!(
                    "remote peer {} presented an invalid host token: {}",
                    peer_addr, error.message
                ),
            )
            .with_recovery(
                "Retry from a peer with a valid literal host token or repair the host authorization policy before retrying cross-host delivery.",
            )
        })?;
        match self.store.load_host(&host_name)? {
            Some(row) if row.enabled => Ok(()),
            Some(_) => Err(
                AtmError::new_with_code(
                    AtmErrorCode::MessageValidationFailed,
                    atm_core::error::AtmErrorKind::Validation,
                    format!(
                        "remote peer {} presented host `{}` but that host is disabled",
                        peer_addr, host_name
                    ),
                )
                .with_recovery(format!(
                    "Run `atm daemon hosts allow {host_name}` on the receiving host before retrying cross-host delivery."
                )),
            ),
            None => Err(
                AtmError::new_with_code(
                    AtmErrorCode::MessageValidationFailed,
                    atm_core::error::AtmErrorKind::Validation,
                    format!(
                        "remote peer {} presented host `{}` but no enabled daemon host row authorizes it",
                        peer_addr, host_name
                    ),
                )
                .with_recovery(format!(
                    "Run `atm daemon hosts allow {host_name}` on the receiving host before retrying cross-host delivery."
                )),
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct PeerTransportConfig {
    pub(crate) peer_listen_addr: Option<SocketAddr>,
}

#[derive(Clone)]
struct PeerClientTransport {
    replay_store: Option<Arc<dyn RemoteReplayStore>>,
    endpoint_resolver: Option<Arc<dyn RemoteEndpointResolver + Send + Sync>>,
    peer_security_store: Option<Arc<dyn PeerSecurityStore + Send + Sync>>,
    #[cfg(test)]
    test_connection_target: Option<SocketAddr>,
    codec: JsonAtmProtocolCodec,
    observability: SubsystemObservability,
}

impl std::fmt::Debug for PeerClientTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PeerClientTransport")
            .field(
                "replay_store",
                &self.replay_store.as_ref().map(|_| "dyn RemoteReplayStore"),
            )
            .field(
                "endpoint_resolver",
                &self
                    .endpoint_resolver
                    .as_ref()
                    .map(|_| "dyn RemoteEndpointResolver"),
            )
            .field(
                "peer_security_store",
                &self
                    .peer_security_store
                    .as_ref()
                    .map(|_| "dyn PeerSecurityStore"),
            )
            .field("test_connection_target", &"<cfg(test)>")
            .field("codec", &"JsonAtmProtocolCodec")
            .field("observability", &self.observability)
            .finish()
    }
}

impl PeerClientTransport {
    fn new_with_observability(
        replay_store: Option<Arc<dyn RemoteReplayStore>>,
        endpoint_resolver: Option<Arc<dyn RemoteEndpointResolver + Send + Sync>>,
        peer_security_store: Option<Arc<dyn PeerSecurityStore + Send + Sync>>,
        observability: SubsystemObservability,
    ) -> Self {
        Self {
            replay_store,
            endpoint_resolver,
            peer_security_store,
            #[cfg(test)]
            test_connection_target: None,
            codec: JsonAtmProtocolCodec,
            observability,
        }
    }

    #[cfg(test)]
    fn new_for_test(
        replay_db_path: PathBuf,
        endpoint_resolver: Option<Arc<dyn RemoteEndpointResolver + Send + Sync>>,
    ) -> Self {
        let replay_store =
            atm_runtime::sqlite_remote_replay_store_for_test(replay_db_path).expect("replay store");
        Self {
            replay_store: Some(replay_store),
            endpoint_resolver,
            peer_security_store: None,
            test_connection_target: None,
            codec: JsonAtmProtocolCodec,
            observability: SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
        }
    }

    #[deprecated(note = "AG.23 deletion target; remove this symbol and all call sites")]
    fn resume_pending_replay(&self) -> Result<ReplayResumeSummary, AtmError> {
        let Some(replay_store) = &self.replay_store else {
            return Ok(ReplayResumeSummary {
                delivered: 0,
                retained: 0,
                purged_expired: 0,
            });
        };

        let records = replay_store.load_all()?;
        if records.len() > MAX_REMOTE_REPLAY_RESUME_RECORDS {
            return Err(AtmError::daemon_unavailable(format!(
                "daemon remote replay resume exceeded the bounded record cap ({MAX_REMOTE_REPLAY_RESUME_RECORDS})"
            ))
            .with_recovery(
                "Drain or delete retained remote replay rows until the bounded startup replay cap is back under control, then restart atm-daemon.",
            ));
        }
        let mut delivered = 0usize;
        let mut retained = 0usize;
        let mut purged_expired = 0usize;
        let now = IsoTimestamp::now();
        for mut record in records {
            if record.expires_at <= now {
                expire_replay_record(replay_store.as_ref(), &record)?;
                purged_expired = purged_expired.saturating_add(1);
                continue;
            }
            let endpoint = self.resolve_replay_endpoint(&record.remote_host)?;
            match self.send_to_endpoint(endpoint, record.request.clone()) {
                Ok(_) => {
                    complete_replay_record(replay_store.as_ref(), &self.observability, &record)?;
                    delivered += 1;
                }
                Err(error) => {
                    if replay_error_is_terminal(&error) {
                        fail_replay_record_terminal(replay_store.as_ref(), &record)?;
                        continue;
                    }
                    retain_replay_record(
                        replay_store.as_ref(),
                        &self.observability,
                        &mut record,
                        &error,
                    )?;
                    retained += 1;
                }
            }
        }

        Ok(ReplayResumeSummary {
            delivered,
            retained,
            purged_expired,
        })
    }

    fn resolve_replay_endpoint(
        &self,
        remote_host: &RemoteTargetHost,
    ) -> Result<SocketAddr, AtmError> {
        let resolver = self.endpoint_resolver.as_ref().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "remote endpoint resolution is unavailable because no resolver boundary is configured",
            )
            .with_recovery(
                "Repair the daemon runtime assembly so remote endpoint resolution is available before retrying deferred remote delivery.",
            )
        })?;
        resolver
            .resolve_endpoint(remote_host, None)
            .map_err(|error| error.into_atm_error())
    }

    fn persist_replay_request_to_endpoint(
        &self,
        retry_budget: Duration,
        remote_host: RemoteTargetHost,
        team: TeamName,
        agent: AgentName,
        message_key: MessageKey,
        request: RequestEnvelope,
    ) -> Result<(), AtmError> {
        crate::remote_replay::persist_replay_request(
            retry_budget,
            self.replay_store.as_ref(),
            remote_host,
            team,
            agent,
            message_key,
            request,
        )
    }

    fn send_to_endpoint(
        &self,
        endpoint: SocketAddr,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, AtmError> {
        let frame = self
            .codec
            .request_to_frame(atm_core::protocol::next_request_id(), request)?;
        let terminate = daemon_terminate_flag()?;
        self.ensure_retry_not_terminated(
            &terminate,
            "daemon shutdown interrupted remote peer delivery before the network attempt",
        )?;
        self.send_once(endpoint, &frame)
            .map(|response| self.record_send_success(endpoint, 0, response))
    }

    fn send_to_endpoint_immediate_wait(
        &self,
        endpoint: SocketAddr,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, AtmError> {
        let frame = self
            .codec
            .request_to_frame(atm_core::protocol::next_request_id(), request)?;
        let terminate = daemon_terminate_flag()?;
        self.ensure_retry_not_terminated(
            &terminate,
            "daemon shutdown interrupted remote peer delivery before the network attempt",
        )?;
        let _ = FOREGROUND_REMOTE_WAIT_BUDGET;
        self.send_once(endpoint, &frame)
            .map(|response| self.record_send_success(endpoint, 0, response))
    }

    fn send_once(
        &self,
        endpoint: SocketAddr,
        request_frame: &atm_core::protocol::FramePayload,
    ) -> Result<ResponseEnvelope, AtmError> {
        let mut stream = self.connect_peer_stream(endpoint)?;
        self.apply_peer_io_deadlines(&stream)?;
        match load_peer_security_mode(self.peer_security_store.as_ref())? {
            PeerSecurityMode::SecureRequired => {
                let store = self.peer_security_store.as_ref().ok_or_else(|| {
                    AtmError::daemon_unavailable(
                        "daemon peer security store is unavailable in secure-required mode",
                    )
                    .with_recovery(
                        "Restore the daemon peer security store before retrying secure cross-host delivery.",
                    )
                })?;
                let mut tls = security::open_client_tls_stream(stream, endpoint, store)?;
                self.publish_request_frame(&mut tls, request_frame)?;
                match self.decode_response_frame(
                    request_frame.request_id,
                    self.read_response_frame(&mut tls)?,
                )? {
                    ResponseEnvelope::Error(error) => Err(error.into_atm_error()),
                    response => Ok(response),
                }
            }
            PeerSecurityMode::InsecureAllowed => {
                self.publish_request_frame(&mut stream, request_frame)?;
                match self.decode_response_frame(
                    request_frame.request_id,
                    self.read_response_frame(&mut stream)?,
                )? {
                    ResponseEnvelope::Error(error) => Err(error.into_atm_error()),
                    response => Ok(response),
                }
            }
        }
    }

    fn connect_peer_stream(&self, endpoint: SocketAddr) -> Result<std::net::TcpStream, AtmError> {
        std::net::TcpStream::connect_timeout(&endpoint, PEER_CONNECT_DEADLINE).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to connect to remote daemon peer at {endpoint}"
            ))
            .with_recovery(
                "Confirm the remote daemon is reachable at the configured peer endpoint, then retry. If the remote daemon is intentionally offline, let durable replay resume the handoff after it recovers.",
            )
            .with_source(source)
        })
    }

    fn apply_peer_io_deadlines(&self, stream: &std::net::TcpStream) -> Result<(), AtmError> {
        stream
            .set_read_timeout(Some(PEER_IO_DEADLINE))
            .map_err(peer_read_deadline_error)?;
        stream
            .set_write_timeout(Some(PEER_IO_DEADLINE))
            .map_err(peer_write_deadline_error)?;
        Ok(())
    }

    fn publish_request_frame(
        &self,
        stream: &mut impl std::io::Write,
        request_frame: &atm_core::protocol::FramePayload,
    ) -> Result<(), AtmError> {
        atm_core::protocol::write_frame(
            stream,
            request_frame,
            "failed to write remote peer request frame",
        )?;
        std::io::Write::flush(stream).map_err(peer_flush_error)?;
        Ok(())
    }

    fn read_response_frame(
        &self,
        stream: &mut impl std::io::Read,
    ) -> Result<atm_core::protocol::FramePayload, AtmError> {
        atm_core::protocol::read_frame(
            stream,
            "failed to read remote peer response frame",
            "remote peer response frame exceeded the maximum supported size",
        )?
        .ok_or_else(peer_closed_before_response_error)
    }

    fn decode_response_frame(
        &self,
        request_id: atm_core::protocol::RequestId,
        response_frame: atm_core::protocol::FramePayload,
    ) -> Result<ResponseEnvelope, AtmError> {
        let (response_id, response) = self
            .codec
            .response_from_frame(response_frame)
            .map_err(peer_response_decode_error)?;
        if response_id != request_id {
            return Err(peer_response_id_mismatch_error(response_id, request_id));
        }
        Ok(response)
    }

    fn ensure_retry_not_terminated(
        &self,
        terminate: &Arc<AtomicBool>,
        message: &'static str,
    ) -> Result<(), AtmError> {
        if terminate.load(Ordering::SeqCst) {
            return Err(AtmError::daemon_unavailable(message).with_recovery(
                "Retry the daemon operation after atm-daemon restarts and resumes pending remote replay work.",
            ));
        }
        Ok(())
    }

    fn record_send_success(
        &self,
        endpoint: SocketAddr,
        attempt: u32,
        response: ResponseEnvelope,
    ) -> ResponseEnvelope {
        tracing::info!(
            peer_addr = %endpoint,
            attempt,
            "daemon peer delivery succeeded"
        );
        self.observability
            .emit_or_warn("send_to_endpoint", "ok", "daemon peer delivery succeeded");
        response
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PeerTransportRuntime {
    client: PeerClientTransport,
    server: Arc<PeerServerTransport>,
}

impl PeerTransportRuntime {
    pub(crate) fn new(replay_store: Option<Arc<dyn RemoteReplayStore>>) -> Self {
        Self::new_with_observability(
            replay_store,
            None,
            None,
            PeerTransportConfig::default(),
            SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
            RuntimeStatusCache::new(),
        )
    }

    pub(crate) fn new_with_observability(
        replay_store: Option<Arc<dyn RemoteReplayStore>>,
        allowed_host_store: Option<Arc<dyn AllowedHostStore + Send + Sync>>,
        peer_security_store: Option<Arc<dyn PeerSecurityStore + Send + Sync>>,
        config: PeerTransportConfig,
        observability: SubsystemObservability,
        status_cache: RuntimeStatusCache,
    ) -> Self {
        #[cfg(test)]
        let endpoint_resolver = config.peer_listen_addr.map(|endpoint| {
            Arc::new(StaticRemoteEndpointResolver { endpoint })
                as Arc<dyn RemoteEndpointResolver + Send + Sync>
        });
        #[cfg(not(test))]
        let endpoint_resolver = None;
        Self::new_with_endpoint_resolver(
            replay_store,
            endpoint_resolver,
            allowed_host_store,
            peer_security_store,
            config,
            observability,
            status_cache,
        )
    }

    pub(crate) fn new_with_endpoint_resolver(
        replay_store: Option<Arc<dyn RemoteReplayStore>>,
        endpoint_resolver: Option<Arc<dyn RemoteEndpointResolver + Send + Sync>>,
        allowed_host_store: Option<Arc<dyn AllowedHostStore + Send + Sync>>,
        peer_security_store: Option<Arc<dyn PeerSecurityStore + Send + Sync>>,
        config: PeerTransportConfig,
        observability: SubsystemObservability,
        status_cache: RuntimeStatusCache,
    ) -> Self {
        let authorization_policy: Arc<dyn PeerAuthorizationPolicy> = allowed_host_store
            .map(|store| {
                Arc::new(SqliteAllowedHostAuthorizationPolicy::new(store))
                    as Arc<dyn PeerAuthorizationPolicy>
            })
            .unwrap_or_else(|| Arc::new(AllowAllPeerAuthorizationPolicy));
        Self {
            server: Arc::new(PeerServerTransport::new(
                config.peer_listen_addr,
                observability.clone(),
                status_cache,
                authorization_policy,
                peer_security_store.clone(),
            )),
            client: PeerClientTransport::new_with_observability(
                replay_store,
                endpoint_resolver,
                peer_security_store,
                observability,
            ),
        }
    }

    #[cfg(test)]
    pub(crate) fn client_transport(&self) -> &dyn ClientTransport {
        &self.client
    }

    #[deprecated(note = "AG.23 deletion target; remove this symbol and all call sites")]
    pub(crate) fn resume_pending_replay(&self) -> Result<ReplayResumeSummary, AtmError> {
        // AG.23 migration: delete this forwarding call with the deprecated
        // runtime replay API; no replacement delivery path is permitted.
        self.client.resume_pending_replay()
    }

    #[allow(
        dead_code,
        reason = "retained for existing test helpers and transitional peer-runtime entrypoints"
    )]
    pub(crate) fn send_to_endpoint(
        &self,
        endpoint: SocketAddr,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, AtmError> {
        self.client.send_to_endpoint(endpoint, request)
    }

    pub(crate) fn send_to_endpoint_immediate_wait(
        &self,
        endpoint: SocketAddr,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, AtmError> {
        self.client
            .send_to_endpoint_immediate_wait(endpoint, request)
    }

    pub(crate) fn persist_remote_request_for_retry(
        &self,
        retry_budget: Duration,
        remote_host: RemoteTargetHost,
        team: TeamName,
        agent: AgentName,
        message_key: MessageKey,
        request: RequestEnvelope,
    ) -> Result<(), AtmError> {
        self.client.persist_replay_request_to_endpoint(
            retry_budget,
            remote_host,
            team,
            agent,
            message_key,
            request,
        )
    }

    #[allow(
        dead_code,
        reason = "retained for tests and transitional peer-runtime entrypoints"
    )]
    pub(crate) fn start(
        &self,
        dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
    ) -> Result<(), AtmError> {
        self.server.start(dispatcher).map(|_| ())
    }

    pub(crate) fn shutdown(&self) -> Result<(), AtmError> {
        self.server.shutdown()
    }

    pub(crate) fn start_replay_resume_worker(&self) -> Result<ReplayResumeWorkerHandle, AtmError> {
        replay_resume_worker::start_replay_resume_worker(self)
    }

    #[allow(dead_code, reason = "retained for existing peer-transport tests")]
    pub(crate) fn reload_listener(
        &self,
        listen_addr: Option<SocketAddr>,
        dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
    ) -> Result<(), AtmError> {
        self.server
            .reload(listen_addr.into_iter().collect::<Vec<_>>(), dispatcher)?;
        Ok(())
    }

    pub(crate) fn reload_listeners(
        &self,
        listen_addrs: Vec<SocketAddr>,
        dispatcher: Arc<dyn RequestDispatcher + Send + Sync>,
    ) -> Result<Vec<server::PeerListenerOutcome>, AtmError> {
        self.server.reload(listen_addrs, dispatcher)
    }

    pub(crate) fn bound_addr(&self) -> Result<Option<SocketAddr>, AtmError> {
        server::bound_addr(&self.server)
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        endpoint: SocketAddr,
        _config: PeerTransportConfig,
        replay_db_path: PathBuf,
    ) -> Self {
        let endpoint_resolver: Arc<dyn RemoteEndpointResolver + Send + Sync> =
            Arc::new(StaticRemoteEndpointResolver { endpoint });
        Self {
            server: Arc::new(PeerServerTransport::new(
                None,
                SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
                RuntimeStatusCache::new(),
                Arc::new(AllowAllPeerAuthorizationPolicy),
                None,
            )),
            client: PeerClientTransport::new_for_test(replay_db_path, Some(endpoint_resolver)),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test_with_security_store(
        endpoint: SocketAddr,
        _config: PeerTransportConfig,
        replay_db_path: PathBuf,
        peer_security_store: Arc<dyn PeerSecurityStore + Send + Sync>,
    ) -> Self {
        let replay_store =
            atm_runtime::sqlite_remote_replay_store_for_test(replay_db_path).expect("replay store");
        Self {
            server: Arc::new(PeerServerTransport::new(
                None,
                SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
                RuntimeStatusCache::new(),
                Arc::new(AllowAllPeerAuthorizationPolicy),
                Some(peer_security_store.clone()),
            )),
            client: PeerClientTransport {
                replay_store: Some(replay_store),
                endpoint_resolver: Some(Arc::new(StaticRemoteEndpointResolver { endpoint })),
                peer_security_store: Some(peer_security_store),
                test_connection_target: Some(endpoint),
                codec: JsonAtmProtocolCodec,
                observability: SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
            },
        }
    }

    #[cfg(test)]
    pub(crate) fn new_server_for_test(
        listen_addr: SocketAddr,
        observability: SubsystemObservability,
    ) -> Self {
        Self::new_server_for_test_with_status_cache(
            listen_addr,
            observability,
            RuntimeStatusCache::new(),
        )
    }

    #[cfg(test)]
    pub(crate) fn new_server_for_test_with_status_cache(
        listen_addr: SocketAddr,
        observability: SubsystemObservability,
        status_cache: RuntimeStatusCache,
    ) -> Self {
        Self {
            server: Arc::new(PeerServerTransport::new(
                Some(listen_addr),
                observability.clone(),
                status_cache,
                Arc::new(AllowAllPeerAuthorizationPolicy),
                None,
            )),
            client: PeerClientTransport::new_with_observability(None, None, None, observability),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_server_for_test_with_allowed_host_store(
        listen_addr: SocketAddr,
        observability: SubsystemObservability,
        status_cache: RuntimeStatusCache,
        allowed_host_store: Arc<dyn AllowedHostStore + Send + Sync>,
    ) -> Self {
        Self {
            server: Arc::new(PeerServerTransport::new(
                Some(listen_addr),
                observability.clone(),
                status_cache,
                Arc::new(SqliteAllowedHostAuthorizationPolicy::new(
                    allowed_host_store,
                )),
                None,
            )),
            client: PeerClientTransport::new_with_observability(None, None, None, observability),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_server_for_test_with_security_and_allowed_host_store(
        listen_addr: SocketAddr,
        observability: SubsystemObservability,
        status_cache: RuntimeStatusCache,
        allowed_host_store: Arc<dyn AllowedHostStore + Send + Sync>,
        peer_security_store: Arc<dyn PeerSecurityStore + Send + Sync>,
    ) -> Self {
        Self {
            server: Arc::new(PeerServerTransport::new(
                Some(listen_addr),
                observability.clone(),
                status_cache,
                Arc::new(SqliteAllowedHostAuthorizationPolicy::new(
                    allowed_host_store,
                )),
                Some(peer_security_store),
            )),
            client: PeerClientTransport::new_with_observability(None, None, None, observability),
        }
    }

    #[cfg(test)]
    pub(crate) fn bound_addr_for_test(&self) -> Option<SocketAddr> {
        self.server.bound_addr_for_test()
    }

    #[cfg(test)]
    #[allow(
        dead_code,
        reason = "retained for direct replay-store tests on the runtime wrapper"
    )]
    pub(crate) fn persist_replay_request(
        &self,
        team: TeamName,
        agent: AgentName,
        message_key: MessageKey,
        request: RequestEnvelope,
    ) -> Result<(), AtmError> {
        self.client.persist_replay_request_to_endpoint(
            Duration::from_secs(60),
            RemoteTargetHost::parse("127.0.0.1").expect("static test remote host"),
            team,
            agent,
            message_key,
            request,
        )
    }

    #[cfg(test)]
    pub(crate) fn load_pending_replay_records(
        &self,
    ) -> Result<Vec<RemoteReplayStateRecord>, AtmError> {
        match &self.client.replay_store {
            Some(replay_store) => replay_store.load_all(),
            None => Ok(Vec::new()),
        }
    }
}
