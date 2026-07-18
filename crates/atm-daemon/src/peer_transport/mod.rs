use std::net::SocketAddr;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[cfg(test)]
use atm_core::boundary::RemoteReplayStateRecord;
use atm_core::boundary::{
    self, AtmProtocol, ClientTransport, MessageKey, RemoteReplayStore, RequestDispatcher,
};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::protocol::{
    JsonAtmProtocolCodec, RequestEnvelope, ResponseEnvelope, TeamMemberHeartbeatRequest,
};
use atm_core::schema::AtmMessageId;
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use atm_storage::{AllowedHostName, AllowedHostStore, PeerSecurityMode, PeerSecurityStore};

use crate::runtime_status_cache::RuntimeStatusCache;
use crate::{DaemonSubsystem, SubsystemObservability};

mod client_helpers;
mod config_helpers;
pub(crate) mod delivery;
mod replay_persistence;
mod replay_resume_worker;
mod security;
mod server;
#[cfg(test)]
mod tests;
mod types;

use client_helpers::{
    classify_io_error, daemon_terminate_flag, jitter_seed, jittered_backoff,
    peer_closed_before_response_error, peer_flush_error, peer_read_deadline_error,
    peer_response_decode_error, peer_response_id_mismatch_error, peer_write_deadline_error,
    wait_for_retry_backoff,
};
use config_helpers::{
    remote_peer_endpoint_not_configured_error, remote_replay_persistence_failed_error,
    remote_replay_store_not_configured_error, remote_retry_budget_expiry_error,
};
use replay_persistence::{
    complete_replay_record, expire_replay_record, fail_replay_record_terminal,
    persist_outcome_unknown_request, replay_error_is_terminal, retain_replay_record,
};
use security::load_peer_security_mode;
use server::PeerServerTransport;
pub(crate) use types::ReplayResumeWorkerHandle;
use types::{DeliveryLoopDecision, DeliveryRetryState, ReplayResumeSummary};

// Architecture authority: docs/architecture.md §21.6.4 daemon operational
// defaults and remote peer transport rules.
// These deadlines are intentionally fixed operational constants. Phase Y keeps
// connect and I/O timeouts non-configurable so remote peer delivery behavior
// stays bounded and auditable across every host.
const PEER_CONNECT_DEADLINE: Duration = Duration::from_secs(5);
const PEER_IO_DEADLINE: Duration = Duration::from_secs(5);
const FOREGROUND_REMOTE_WAIT_BUDGET: Duration = Duration::from_secs(10);
const DEFAULT_REMOTE_RETRY_BUDGET: Duration = Duration::from_secs(60);
const INITIAL_RETRY_BACKOFF: Duration = Duration::from_secs(5);
const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(30);
const MAX_REMOTE_RETRY_ATTEMPTS: u32 = 6;
const REPLAY_RESUME_POLL_INTERVAL: Duration = Duration::from_secs(5);
const MAX_REMOTE_REPLAY_RESUME_RECORDS: usize = 10_000;
const PEER_LISTENER_ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const PEER_REQUEST_DEADLINE: Duration = Duration::from_secs(3);
const PEER_CONNECTION_IO_SLICE: Duration = Duration::from_millis(200);
const PEER_LISTENER_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(3);
const PEER_ACCEPT_ERROR_RETRY_BACKOFF: Duration = Duration::from_millis(200);
const MAX_CONCURRENT_PEER_CONNECTIONS: usize = 64;
const MAX_TRACKED_PEER_DISPATCH_HANDLES: usize = 256;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PeerTransportConfig {
    pub(crate) remote_retry_budget: Duration,
    pub(crate) peer_listen_addr: Option<SocketAddr>,
}

impl Default for PeerTransportConfig {
    fn default() -> Self {
        Self {
            remote_retry_budget: DEFAULT_REMOTE_RETRY_BUDGET,
            peer_listen_addr: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttemptFailureKind {
    Retryable,
    NonRetryable,
    OutcomeUnknown,
}

#[derive(Debug)]
struct AttemptFailure {
    kind: AttemptFailureKind,
    error: AtmError,
}

#[derive(Clone)]
struct PeerClientTransport {
    endpoint: Option<SocketAddr>,
    config: PeerTransportConfig,
    replay_store: Option<Arc<dyn RemoteReplayStore>>,
    peer_security_store: Option<Arc<dyn PeerSecurityStore + Send + Sync>>,
    codec: JsonAtmProtocolCodec,
    observability: SubsystemObservability,
}

impl std::fmt::Debug for PeerClientTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PeerClientTransport")
            .field("endpoint", &self.endpoint)
            .field("config", &self.config)
            .field(
                "replay_store",
                &self.replay_store.as_ref().map(|_| "dyn RemoteReplayStore"),
            )
            .field(
                "peer_security_store",
                &self
                    .peer_security_store
                    .as_ref()
                    .map(|_| "dyn PeerSecurityStore"),
            )
            .field("codec", &"JsonAtmProtocolCodec")
            .field("observability", &self.observability)
            .finish()
    }
}

impl PeerClientTransport {
    fn new_with_observability(
        replay_store: Option<Arc<dyn RemoteReplayStore>>,
        peer_security_store: Option<Arc<dyn PeerSecurityStore + Send + Sync>>,
        config: PeerTransportConfig,
        observability: SubsystemObservability,
    ) -> Self {
        Self {
            endpoint: None,
            config,
            replay_store,
            peer_security_store,
            codec: JsonAtmProtocolCodec,
            observability,
        }
    }

    #[cfg(test)]
    fn new_for_test(
        endpoint: SocketAddr,
        config: PeerTransportConfig,
        replay_db_path: PathBuf,
    ) -> Self {
        let replay_store =
            atm_runtime::sqlite_remote_replay_store_for_test(replay_db_path).expect("replay store");
        Self {
            endpoint: Some(endpoint),
            config,
            replay_store: Some(replay_store),
            peer_security_store: None,
            codec: JsonAtmProtocolCodec,
            observability: SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
        }
    }

    fn resume_pending_replay(&self) -> Result<ReplayResumeSummary, AtmError> {
        let Some(replay_store) = &self.replay_store else {
            return Ok(ReplayResumeSummary {
                delivered: 0,
                retained: 0,
                purged_expired: 0,
                receipt_updates: 0,
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
        let mut receipt_updates = 0usize;
        let now = IsoTimestamp::now();
        for mut record in records {
            if record.expires_at <= now {
                if expire_replay_record(replay_store.as_ref(), &record)? {
                    receipt_updates = receipt_updates.saturating_add(1);
                }
                purged_expired = purged_expired.saturating_add(1);
                continue;
            }
            match self.send_to_endpoint(record.peer_addr, record.request.clone()) {
                Ok(_) => {
                    if complete_replay_record(replay_store.as_ref(), &self.observability, &record)?
                    {
                        receipt_updates = receipt_updates.saturating_add(1);
                    }
                    delivered += 1;
                }
                Err(error) => {
                    if replay_error_is_terminal(&error) {
                        if fail_replay_record_terminal(
                            replay_store.as_ref(),
                            &record,
                            &error.message,
                        )? {
                            receipt_updates = receipt_updates.saturating_add(1);
                        }
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
            receipt_updates,
        })
    }

    #[allow(
        dead_code,
        reason = "retained for legacy replay-persistence contract tests while the explicit-endpoint path is used in production dispatch"
    )]
    fn persist_replay_request(
        &self,
        team: TeamName,
        agent: AgentName,
        message_key: MessageKey,
        request: RequestEnvelope,
    ) -> Result<(), AtmError> {
        let endpoint = self
            .endpoint
            .ok_or_else(remote_peer_endpoint_not_configured_error)?;
        self.persist_replay_request_to_endpoint(
            endpoint,
            team,
            agent,
            message_key,
            request,
            None,
            None,
            None,
            None,
            None,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "cross-host retry persistence needs explicit sender and receipt metadata at the peer-transport boundary"
    )]
    fn persist_replay_request_to_endpoint(
        &self,
        endpoint: SocketAddr,
        team: TeamName,
        agent: AgentName,
        message_key: MessageKey,
        request: RequestEnvelope,
        receipt_sender_team: Option<TeamName>,
        receipt_sender_agent: Option<AgentName>,
        receipt_message_id: Option<AtmMessageId>,
        receipt_target: Option<String>,
        receipt_remote_host: Option<String>,
    ) -> Result<(), AtmError> {
        replay_persistence::persist_replay_request(
            &self.config,
            self.replay_store.as_ref(),
            endpoint,
            team,
            agent,
            message_key,
            request,
            receipt_sender_team,
            receipt_sender_agent,
            receipt_message_id,
            receipt_target,
            receipt_remote_host,
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
        let deadline = Instant::now() + self.config.remote_retry_budget;
        let terminate = daemon_terminate_flag()?;
        let mut backoff = INITIAL_RETRY_BACKOFF;
        let mut attempt = 0u32;

        loop {
            self.ensure_retry_not_terminated(
                &terminate,
                "daemon shutdown interrupted remote peer delivery before the next network attempt",
            )?;
            let current_attempt = attempt;
            let mut retry_state = DeliveryRetryState {
                deadline,
                terminate: &terminate,
                backoff: &mut backoff,
                next_attempt: &mut attempt,
                attempt_cap: MAX_REMOTE_RETRY_ATTEMPTS,
            };
            match self.send_once(endpoint, &frame) {
                Ok(response) => {
                    return Ok(self.record_send_success(endpoint, current_attempt, response));
                }
                Err(failure) => match self.handle_send_failure(
                    endpoint,
                    current_attempt,
                    &mut retry_state,
                    *failure,
                ) {
                    DeliveryLoopDecision::Retry => {}
                    DeliveryLoopDecision::Return(error) => return Err(error),
                },
            }
        }
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
        let started = Instant::now();
        let response = self
            .send_once(endpoint, &frame)
            .map(|response| self.record_send_success(endpoint, 0, response))
            .map_err(|failure| failure.error)?;
        if started.elapsed() > FOREGROUND_REMOTE_WAIT_BUDGET {
            return Err(AtmError::daemon_unavailable(
                "foreground remote delivery exceeded the healthy immediate-wait ceiling",
            )
            .with_recovery(
                "Retry the remote send after the peer path is healthy again, or let the daemon retain it for bounded replay.",
            ));
        }
        Ok(response)
    }

    fn send_once(
        &self,
        endpoint: SocketAddr,
        request_frame: &atm_core::protocol::FramePayload,
    ) -> Result<ResponseEnvelope, Box<AttemptFailure>> {
        let mut stream = self.connect_peer_stream(endpoint)?;
        self.apply_peer_io_deadlines(&stream)?;
        match load_peer_security_mode(self.peer_security_store.as_ref()).map_err(|error| {
            Box::new(AttemptFailure {
                kind: AttemptFailureKind::NonRetryable,
                error,
            })
        })? {
            PeerSecurityMode::SecureRequired => {
                let store = self.peer_security_store.as_ref().ok_or_else(|| {
                    Box::new(AttemptFailure {
                        kind: AttemptFailureKind::NonRetryable,
                        error: AtmError::daemon_unavailable(
                            "daemon peer security store is unavailable in secure-required mode",
                        )
                        .with_recovery(
                            "Restore the daemon peer security store before retrying secure cross-host delivery.",
                        ),
                    })
                })?;
                let mut tls =
                    security::open_client_tls_stream(stream, endpoint, store).map_err(|error| {
                        Box::new(AttemptFailure {
                            kind: AttemptFailureKind::NonRetryable,
                            error,
                        })
                    })?;
                self.publish_request_frame(&mut tls, request_frame)?;
                match self.decode_response_frame(
                    request_frame.request_id,
                    self.read_response_frame(&mut tls)?,
                )? {
                    ResponseEnvelope::Error(error) => Err(Box::new(AttemptFailure {
                        kind: AttemptFailureKind::NonRetryable,
                        error: error.into_atm_error(),
                    })),
                    response => Ok(response),
                }
            }
            PeerSecurityMode::InsecureAllowed => {
                self.publish_request_frame(&mut stream, request_frame)?;
                match self.decode_response_frame(
                    request_frame.request_id,
                    self.read_response_frame(&mut stream)?,
                )? {
                    ResponseEnvelope::Error(error) => Err(Box::new(AttemptFailure {
                        kind: AttemptFailureKind::NonRetryable,
                        error: error.into_atm_error(),
                    })),
                    response => Ok(response),
                }
            }
        }
    }

    fn connect_peer_stream(
        &self,
        endpoint: SocketAddr,
    ) -> Result<std::net::TcpStream, Box<AttemptFailure>> {
        std::net::TcpStream::connect_timeout(&endpoint, PEER_CONNECT_DEADLINE).map_err(|source| {
            Box::new(AttemptFailure {
                kind: classify_io_error(&source),
                error: AtmError::daemon_unavailable(format!(
                    "failed to connect to remote daemon peer at {endpoint}"
                ))
                .with_recovery(
                    "Confirm the remote daemon is reachable at the configured peer endpoint, then retry. If the remote daemon is intentionally offline, let durable replay resume the handoff after it recovers.",
                )
                .with_source(source),
            })
        })
    }

    fn apply_peer_io_deadlines(
        &self,
        stream: &std::net::TcpStream,
    ) -> Result<(), Box<AttemptFailure>> {
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
    ) -> Result<(), Box<AttemptFailure>> {
        atm_core::protocol::write_frame(
            stream,
            request_frame,
            "failed to write remote peer request frame",
        )
        .map_err(|error| {
            Box::new(AttemptFailure {
                kind: AttemptFailureKind::Retryable,
                error,
            })
        })?;
        std::io::Write::flush(stream).map_err(peer_flush_error)?;
        Ok(())
    }

    fn read_response_frame(
        &self,
        stream: &mut impl std::io::Read,
    ) -> Result<atm_core::protocol::FramePayload, Box<AttemptFailure>> {
        atm_core::protocol::read_frame(
            stream,
            "failed to read remote peer response frame",
            "remote peer response frame exceeded the maximum supported size",
        )
        .map_err(|error| {
            Box::new(AttemptFailure {
                kind: AttemptFailureKind::OutcomeUnknown,
                error,
            })
        })?
        .ok_or_else(peer_closed_before_response_error)
    }

    fn decode_response_frame(
        &self,
        request_id: atm_core::protocol::RequestId,
        response_frame: atm_core::protocol::FramePayload,
    ) -> Result<ResponseEnvelope, Box<AttemptFailure>> {
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

    fn handle_send_failure(
        &self,
        endpoint: SocketAddr,
        attempt: u32,
        retry_state: &mut DeliveryRetryState<'_>,
        failure: AttemptFailure,
    ) -> DeliveryLoopDecision {
        match failure.kind {
            AttemptFailureKind::Retryable => {
                self.handle_retryable_failure(endpoint, attempt, retry_state, failure.error)
            }
            AttemptFailureKind::NonRetryable | AttemptFailureKind::OutcomeUnknown => {
                self.handle_terminal_failure(endpoint, attempt, failure)
            }
        }
    }

    fn handle_retryable_failure(
        &self,
        endpoint: SocketAddr,
        attempt: u32,
        retry_state: &mut DeliveryRetryState<'_>,
        error: AtmError,
    ) -> DeliveryLoopDecision {
        let now = Instant::now();
        if now >= retry_state.deadline {
            tracing::error!(
                subsystem = "peer_transport",
                action = "send_to_endpoint",
                outcome = "retry_exhausted",
                peer_addr = %endpoint,
                attempt,
                error_code = %error.code,
                error_message = %error.message,
                "daemon peer delivery exhausted retry budget"
            );
            self.observability.emit_or_warn(
                "send_to_endpoint",
                "failed",
                "daemon peer delivery exhausted its retry budget",
            );
            return DeliveryLoopDecision::Return(error);
        }
        if attempt.saturating_add(1) >= retry_state.attempt_cap {
            tracing::error!(
                subsystem = "peer_transport",
                action = "send_to_endpoint",
                outcome = "retry_attempt_cap_exhausted",
                peer_addr = %endpoint,
                attempt,
                attempt_cap = retry_state.attempt_cap,
                error_code = %error.code,
                error_message = %error.message,
                "daemon peer delivery exhausted the bounded retry-attempt cap"
            );
            self.observability.emit_or_warn(
                "send_to_endpoint",
                "failed",
                "daemon peer delivery exhausted the bounded retry-attempt cap",
            );
            return DeliveryLoopDecision::Return(error);
        }
        let remaining = retry_state.deadline.saturating_duration_since(now);
        let sleep_for =
            jittered_backoff(*retry_state.backoff, jitter_seed(endpoint, attempt)).min(remaining);
        tracing::warn!(
            subsystem = "peer_transport",
            action = "retry",
            outcome = "retrying",
            peer_addr = %endpoint,
            attempt,
            sleep_ms = sleep_for.as_millis(),
            error_code = %error.code,
            error_message = %error.message,
            "daemon peer delivery hit retryable failure"
        );
        self.observability.emit_or_warn(
            "send_to_endpoint",
            "degraded",
            "daemon peer delivery hit a retryable failure",
        );
        if wait_for_retry_backoff(retry_state.terminate, sleep_for) {
            return DeliveryLoopDecision::Return(
                AtmError::daemon_unavailable(
                    "daemon shutdown interrupted remote peer retry backoff",
                )
                .with_recovery(
                    "Retry the daemon operation after atm-daemon restarts and resumes pending remote replay work.",
                ),
            );
        }
        *retry_state.backoff = retry_state.backoff.saturating_mul(2).min(MAX_RETRY_BACKOFF);
        *retry_state.next_attempt = retry_state.next_attempt.saturating_add(1);
        DeliveryLoopDecision::Retry
    }

    fn handle_terminal_failure(
        &self,
        endpoint: SocketAddr,
        attempt: u32,
        failure: AttemptFailure,
    ) -> DeliveryLoopDecision {
        let failure_kind = match failure.kind {
            AttemptFailureKind::OutcomeUnknown => "outcome_unknown",
            AttemptFailureKind::NonRetryable => "non_retryable",
            AttemptFailureKind::Retryable => "retryable",
        };
        tracing::error!(
            subsystem = "peer_transport",
            action = "send_to_endpoint",
            outcome = "terminal_failure",
            peer_addr = %endpoint,
            attempt,
            failure_kind,
            error_code = %failure.error.code,
            error_message = %failure.error.message,
            "daemon peer delivery failed"
        );
        self.observability.emit_or_warn(
            "send_to_endpoint",
            "failed",
            "daemon peer delivery failed with a non-retryable or outcome-unknown error",
        );
        DeliveryLoopDecision::Return(failure.error)
    }

    fn send_with_outcome_persistence(
        &self,
        endpoint: SocketAddr,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, AtmError> {
        match self.send_to_endpoint(endpoint, request.clone()) {
            Ok(response) => Ok(response),
            Err(error) if error.code == AtmErrorCode::RemoteDeliveryOutcomeUnknown => {
                persist_outcome_unknown_request(self, &request)
                    .map_err(remote_replay_persistence_failed_error)?;
                Err(error)
            }
            Err(error) => Err(error),
        }
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
                peer_security_store,
                config,
                observability,
            ),
        }
    }

    #[cfg(test)]
    pub(crate) fn client_transport(&self) -> &dyn ClientTransport {
        &self.client
    }

    pub(crate) fn resume_pending_replay(&self) -> Result<ReplayResumeSummary, AtmError> {
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
        self.client.send_with_outcome_persistence(endpoint, request)
    }

    pub(crate) fn send_to_endpoint_immediate_wait(
        &self,
        endpoint: SocketAddr,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, AtmError> {
        self.client
            .send_to_endpoint_immediate_wait(endpoint, request)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "cross-host retry persistence needs explicit sender and receipt metadata at the peer-transport boundary"
    )]
    pub(crate) fn persist_remote_request_for_retry(
        &self,
        endpoint: SocketAddr,
        team: TeamName,
        agent: AgentName,
        message_key: MessageKey,
        request: RequestEnvelope,
        receipt_sender_team: Option<TeamName>,
        receipt_sender_agent: Option<AgentName>,
        receipt_message_id: Option<AtmMessageId>,
        receipt_target: Option<String>,
        receipt_remote_host: Option<String>,
    ) -> Result<(), AtmError> {
        self.client.persist_replay_request_to_endpoint(
            endpoint,
            team,
            agent,
            message_key,
            request,
            receipt_sender_team,
            receipt_sender_agent,
            receipt_message_id,
            receipt_target,
            receipt_remote_host,
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
        config: PeerTransportConfig,
        replay_db_path: PathBuf,
    ) -> Self {
        Self {
            server: Arc::new(PeerServerTransport::new(
                None,
                SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
                RuntimeStatusCache::new(),
                Arc::new(AllowAllPeerAuthorizationPolicy),
                None,
            )),
            client: PeerClientTransport::new_for_test(endpoint, config, replay_db_path),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test_with_security_store(
        endpoint: SocketAddr,
        config: PeerTransportConfig,
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
                endpoint: Some(endpoint),
                config,
                replay_store: Some(replay_store),
                peer_security_store: Some(peer_security_store),
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
        let config = PeerTransportConfig {
            remote_retry_budget: DEFAULT_REMOTE_RETRY_BUDGET,
            peer_listen_addr: Some(listen_addr),
        };
        Self {
            server: Arc::new(PeerServerTransport::new(
                Some(listen_addr),
                observability.clone(),
                status_cache,
                Arc::new(AllowAllPeerAuthorizationPolicy),
                None,
            )),
            client: PeerClientTransport::new_with_observability(None, None, config, observability),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_server_for_test_with_allowed_host_store(
        listen_addr: SocketAddr,
        observability: SubsystemObservability,
        status_cache: RuntimeStatusCache,
        allowed_host_store: Arc<dyn AllowedHostStore + Send + Sync>,
    ) -> Self {
        let config = PeerTransportConfig {
            remote_retry_budget: DEFAULT_REMOTE_RETRY_BUDGET,
            peer_listen_addr: Some(listen_addr),
        };
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
            client: PeerClientTransport::new_with_observability(None, None, config, observability),
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
        let config = PeerTransportConfig {
            remote_retry_budget: DEFAULT_REMOTE_RETRY_BUDGET,
            peer_listen_addr: Some(listen_addr),
        };
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
            client: PeerClientTransport::new_with_observability(None, None, config, observability),
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
        self.client
            .persist_replay_request(team, agent, message_key, request)
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
