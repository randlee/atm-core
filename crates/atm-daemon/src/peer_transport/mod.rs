use std::io;
use std::net::SocketAddr;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use atm_core::AtmConfig;
use atm_core::boundary::{
    self, AtmProtocol, ClientTransport, MessageKey, RemoteReplayStateRecord, RemoteReplayStore,
    RequestDispatcher,
};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::protocol::{
    JsonAtmProtocolCodec, RequestEnvelope, ResponseEnvelope, TeamMemberHeartbeatRequest,
};
use atm_core::types::{AgentName, IsoTimestamp, TeamName};

use crate::runtime_status_cache::RuntimeStatusCache;
use crate::{DaemonSubsystem, SubsystemObservability};

mod server;
#[cfg(test)]
mod tests;

use server::PeerServerTransport;

// Architecture authority: docs/architecture.md §21.6.4 daemon operational
// defaults and remote peer transport rules.
// These deadlines are intentionally fixed operational constants. Phase Y keeps
// connect and I/O timeouts non-configurable so remote peer delivery behavior
// stays bounded and auditable across every host.
const PEER_CONNECT_DEADLINE: Duration = Duration::from_secs(5);
const PEER_IO_DEADLINE: Duration = Duration::from_secs(5);
const DEFAULT_REMOTE_RETRY_BUDGET: Duration = Duration::from_secs(30);
const MIN_REMOTE_RETRY_BUDGET: Duration = Duration::from_secs(1);
const MAX_REMOTE_RETRY_BUDGET: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const INITIAL_RETRY_BACKOFF: Duration = Duration::from_millis(250);
const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(5);
const MAX_REMOTE_REPLAY_RESUME_RECORDS: usize = 10_000;
const PEER_LISTENER_ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const PEER_REQUEST_DEADLINE: Duration = Duration::from_secs(3);
const PEER_CONNECTION_IO_SLICE: Duration = Duration::from_millis(200);
const PEER_LISTENER_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(3);
const PEER_ACCEPT_ERROR_RETRY_BACKOFF: Duration = Duration::from_millis(200);
const MAX_CONCURRENT_PEER_CONNECTIONS: usize = 64;
const MAX_TRACKED_PEER_DISPATCH_HANDLES: usize = 256;

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

impl PeerTransportConfig {
    pub(crate) fn from_config(config: Option<&AtmConfig>) -> Result<Self, AtmError> {
        if config.is_none() {
            tracing::warn!(
                subsystem = "peer_transport",
                action = "config_default",
                outcome = "default",
                "no AtmConfig provided to PeerTransportConfig::from_config; using default remote_retry_budget"
            );
        }
        let remote_retry_budget = config
            .map(|config| config.daemon.remote_retry_budget)
            .unwrap_or(DEFAULT_REMOTE_RETRY_BUDGET);
        if remote_retry_budget < MIN_REMOTE_RETRY_BUDGET {
            return Err(AtmError::validation(format!(
                "daemon.remote_retry_budget must be at least {} second(s)",
                MIN_REMOTE_RETRY_BUDGET.as_secs()
            ))
            .with_recovery(
                "Raise daemon.remote_retry_budget to at least one second before starting atm-daemon.",
            ));
        }
        if remote_retry_budget > MAX_REMOTE_RETRY_BUDGET {
            return Err(AtmError::validation(format!(
                "daemon.remote_retry_budget must not exceed {} second(s)",
                MAX_REMOTE_RETRY_BUDGET.as_secs()
            ))
            .with_recovery(
                "Lower daemon.remote_retry_budget to one week or less before starting atm-daemon.",
            ));
        }
        Ok(Self {
            remote_retry_budget,
            peer_listen_addr: config.and_then(|config| config.daemon.peer_listen_addr),
        })
    }
}

fn remote_replay_store_not_configured_error() -> AtmError {
    AtmError::daemon_unavailable("remote replay store is not configured").with_recovery(
        "Restore the host-scoped ATM durable replay store before retrying remote delivery so atm-daemon can resume unknown peer handoffs safely.",
    )
}

fn remote_peer_endpoint_not_configured_error() -> AtmError {
    AtmError::daemon_unavailable("remote peer endpoint is not configured").with_recovery(
        "Set ATM_DAEMON_PEER_ADDR or configure the daemon peer transport before retrying remote delivery or replay persistence.",
    )
}

fn remote_retry_budget_expiry_error(
    source: impl std::error::Error + Send + Sync + 'static,
) -> AtmError {
    AtmError::daemon_unavailable("failed to convert remote retry budget into a replay expiry")
        .with_recovery(
            "Fix the daemon remote retry budget configuration and restart atm-daemon before retrying remote delivery so replay expiry can be computed deterministically.",
        )
        .with_source(source)
}

fn remote_replay_persistence_failed_error(source: AtmError) -> AtmError {
    AtmError::remote_delivery_outcome_unknown(
        "remote peer delivery outcome is unknown and replay persistence failed",
    )
    .with_source(source)
}

fn daemon_peer_endpoint_from_env() -> Option<SocketAddr> {
    match std::env::var("ATM_DAEMON_PEER_ADDR") {
        Ok(raw) => parse_peer_endpoint(&raw),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            tracing::warn!(
                subsystem = "peer_transport",
                action = "env_parse",
                outcome = "ignored",
                "ignoring non-unicode ATM_DAEMON_PEER_ADDR value"
            );
            None
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

struct DeliveryRetryState<'a> {
    deadline: Instant,
    terminate: &'a Arc<AtomicBool>,
    backoff: &'a mut Duration,
    next_attempt: &'a mut u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReplayResumeSummary {
    pub(crate) delivered: usize,
    pub(crate) retained: usize,
    pub(crate) purged_expired: usize,
}

#[derive(Clone)]
struct PeerClientTransport {
    endpoint: Option<SocketAddr>,
    config: PeerTransportConfig,
    replay_store: Option<Arc<dyn RemoteReplayStore>>,
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
            .field("codec", &"JsonAtmProtocolCodec")
            .field("observability", &self.observability)
            .finish()
    }
}

impl PeerClientTransport {
    fn new_with_observability(
        replay_store: Option<Arc<dyn RemoteReplayStore>>,
        config: PeerTransportConfig,
        observability: SubsystemObservability,
    ) -> Self {
        let endpoint = daemon_peer_endpoint_from_env();
        Self {
            endpoint,
            config,
            replay_store,
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
            });
        };

        let now = IsoTimestamp::now();
        let purged_expired = replay_store.purge_expired(now)?;
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
        for mut record in records {
            match self.send_to_endpoint(record.peer_addr, record.request.clone()) {
                Ok(_) => {
                    replay_store.delete(&record.team, &record.agent, &record.message_key)?;
                    tracing::info!(
                        message_key = %record.message_key,
                        peer_addr = %record.peer_addr,
                        replay_attempt_count = record.attempt_count,
                        "daemon remote replay delivered successfully"
                    );
                    self.observability.emit_or_warn(
                        "resume_pending_replay",
                        "ok",
                        "daemon remote replay delivered a retained record",
                    );
                    delivered += 1;
                }
                Err(error) => {
                    record.attempt_count = record.attempt_count.saturating_add(1);
                    record.last_attempt_at = Some(IsoTimestamp::now());
                    record.last_error = Some(error.code);
                    tracing::warn!(
                        subsystem = "peer_transport",
                        action = "resume_replay",
                        outcome = "skipped",
                        message_key = %record.message_key,
                        peer_addr = %record.peer_addr,
                        replay_attempt_count = record.attempt_count,
                        error_code = %error.code,
                        error_message = %error.message,
                        "daemon remote replay delivery attempt failed; retaining record"
                    );
                    self.observability.emit_or_warn(
                        "resume_pending_replay",
                        "degraded",
                        "daemon remote replay delivery failed and retained the record for retry",
                    );
                    replay_store.enqueue(record)?;
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

    fn persist_replay_request(
        &self,
        team: TeamName,
        agent: AgentName,
        message_key: MessageKey,
        request: RequestEnvelope,
    ) -> Result<(), AtmError> {
        let Some(replay_store) = &self.replay_store else {
            return Err(remote_replay_store_not_configured_error());
        };
        let endpoint = self
            .endpoint
            .ok_or_else(remote_peer_endpoint_not_configured_error)?;
        let recorded_at = IsoTimestamp::now();
        let expires_at = IsoTimestamp::from_datetime(
            recorded_at.into_inner()
                + chrono::Duration::from_std(self.config.remote_retry_budget)
                    .map_err(remote_retry_budget_expiry_error)?,
        );
        replay_store.enqueue(RemoteReplayStateRecord {
            team,
            agent,
            message_key,
            peer_addr: endpoint,
            request,
            recorded_at,
            expires_at,
            attempt_count: 0,
            last_attempt_at: None,
            last_error: None,
        })
    }

    fn persist_outcome_unknown_request(&self, request: &RequestEnvelope) -> Result<(), AtmError> {
        let Some((team, agent, message_key)) = replay_metadata_for_request(request) else {
            tracing::warn!(
                subsystem = "peer_transport",
                action = "persist",
                outcome = "unknown",
                request = ?request,
                "remote delivery outcome is unknown but this request family does not support durable replay persistence",
            );
            return Ok(());
        };
        self.persist_replay_request(team, agent, message_key, request.clone())
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

    fn send_once(
        &self,
        endpoint: SocketAddr,
        request_frame: &atm_core::protocol::FramePayload,
    ) -> Result<ResponseEnvelope, Box<AttemptFailure>> {
        let mut stream = self.connect_peer_stream(endpoint)?;
        self.apply_peer_io_deadlines(&stream)?;
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
        stream: &mut std::net::TcpStream,
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
        stream: &mut std::net::TcpStream,
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
                self.persist_outcome_unknown_request(&request)
                    .map_err(remote_replay_persistence_failed_error)?;
                Err(error)
            }
            Err(error) => Err(error),
        }
    }
}

enum DeliveryLoopDecision {
    Retry,
    Return(AtmError),
}

fn peer_read_deadline_error(source: io::Error) -> Box<AttemptFailure> {
    Box::new(AttemptFailure {
        kind: AttemptFailureKind::Retryable,
        error: AtmError::daemon_unavailable("failed to apply remote peer read deadline")
            .with_recovery(
                "Restart atm-daemon and retry after the peer socket can accept bounded read deadlines again.",
            )
            .with_source(source),
    })
}

fn peer_write_deadline_error(source: io::Error) -> Box<AttemptFailure> {
    Box::new(AttemptFailure {
        kind: AttemptFailureKind::Retryable,
        error: AtmError::daemon_unavailable("failed to apply remote peer write deadline")
            .with_recovery(
                "Restart atm-daemon and retry after the peer socket can accept bounded write deadlines again.",
            )
            .with_source(source),
    })
}

fn peer_flush_error(source: io::Error) -> Box<AttemptFailure> {
    Box::new(AttemptFailure {
        kind: AttemptFailureKind::OutcomeUnknown,
        error: AtmError::remote_delivery_outcome_unknown(
            "failed to flush the remote peer request frame before waiting for a response",
        )
        .with_source(source),
    })
}

fn peer_closed_before_response_error() -> Box<AttemptFailure> {
    Box::new(AttemptFailure {
        kind: AttemptFailureKind::OutcomeUnknown,
        error: AtmError::remote_delivery_outcome_unknown(
            "remote peer closed the connection before returning a response frame",
        )
        .with_recovery(
            "Check the destination daemon or mailbox before retrying. If local durable replay is enabled, let the daemon resume the pending handoff rather than guessing success.",
        ),
    })
}

fn peer_response_decode_error(error: AtmError) -> Box<AttemptFailure> {
    Box::new(AttemptFailure {
        kind: AttemptFailureKind::NonRetryable,
        error: AtmError::daemon_unavailable("failed to decode remote peer response frame")
            .with_recovery(
                "Align the peer daemon builds so both sides speak the same ATM daemon protocol before retrying the remote delivery.",
            )
            .with_source(error),
    })
}

fn peer_response_id_mismatch_error(
    response_id: atm_core::protocol::RequestId,
    request_id: atm_core::protocol::RequestId,
) -> Box<AttemptFailure> {
    Box::new(AttemptFailure {
        kind: AttemptFailureKind::NonRetryable,
        error: AtmError::daemon_unavailable(format!(
            "remote peer response request_id {} did not match request_id {}",
            response_id, request_id
        ))
        .with_recovery(
            "Align the peer daemon builds so both sides use the same ATM daemon protocol contract before retrying.",
        ),
    })
}

fn wait_for_retry_backoff(terminate: &Arc<AtomicBool>, sleep_for: Duration) -> bool {
    const RETRY_POLL_INTERVAL: Duration = Duration::from_millis(25);

    let started = Instant::now();
    loop {
        if terminate.load(Ordering::SeqCst) {
            return true;
        }
        let elapsed = started.elapsed();
        if elapsed >= sleep_for {
            return false;
        }
        let remaining = sleep_for.saturating_sub(elapsed);
        thread::sleep(remaining.min(RETRY_POLL_INTERVAL));
    }
}

fn daemon_terminate_flag() -> Result<Arc<AtomicBool>, AtmError> {
    static DAEMON_TERMINATE_FLAG: OnceLock<Arc<AtomicBool>> = OnceLock::new();

    if let Some(flag) = DAEMON_TERMINATE_FLAG.get() {
        return Ok(Arc::clone(flag));
    }

    let flag = crate::lifecycle_control::LifecycleControlSourceAdapter::install()?.terminate_flag();
    match DAEMON_TERMINATE_FLAG.set(Arc::clone(&flag)) {
        Ok(()) => Ok(flag),
        Err(existing) => Ok(existing),
    }
}

fn classify_io_error(error: &io::Error) -> AttemptFailureKind {
    match error.kind() {
        io::ErrorKind::TimedOut
        | io::ErrorKind::Interrupted
        | io::ErrorKind::ConnectionRefused
        | io::ErrorKind::ConnectionReset
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::BrokenPipe
        | io::ErrorKind::AddrNotAvailable
        | io::ErrorKind::NotConnected
        | io::ErrorKind::HostUnreachable
        | io::ErrorKind::NetworkUnreachable => AttemptFailureKind::Retryable,
        _ => AttemptFailureKind::NonRetryable,
    }
}

fn replay_metadata_for_request(
    request: &RequestEnvelope,
) -> Option<(TeamName, AgentName, MessageKey)> {
    match request {
        RequestEnvelope::Heartbeat(heartbeat) => Some((
            heartbeat.team.clone(),
            heartbeat.member.clone(),
            heartbeat_message_key(heartbeat),
        )),
        _ => None,
    }
}

fn heartbeat_message_key(request: &TeamMemberHeartbeatRequest) -> MessageKey {
    MessageKey::new(format!(
        "remote-heartbeat:{}:{}:{}:{}",
        request.team.as_str(),
        request.member.as_str(),
        request.pid,
        request.observed_at.into_inner().to_rfc3339(),
    ))
    .expect("validated heartbeat replay keys are never blank")
}

fn jittered_backoff(base: Duration, seed: u64) -> Duration {
    let base_nanos = base.as_nanos();
    if base_nanos == 0 {
        return Duration::ZERO;
    }
    let offset_basis_points = (seed % 4001) as i128 - 2000;
    let scaled = (base_nanos as i128 * (10_000 + offset_basis_points))
        .div_euclid(10_000)
        .max(1);
    Duration::from_nanos(u64::try_from(scaled).unwrap_or(u64::MAX))
}

fn jitter_seed(endpoint: SocketAddr, attempt: u32) -> u64 {
    let now = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos() as u64,
        Err(error) => {
            tracing::warn!(
                subsystem = "peer_transport",
                action = "jitter_seed",
                outcome = "default",
                %error,
                "system clock is before the unix epoch; using deterministic jitter fallback"
            );
            0
        }
    };
    now ^ u64::from(endpoint.port()) ^ (u64::from(attempt) << 32)
}

fn parse_peer_endpoint(raw: &str) -> Option<SocketAddr> {
    match raw.parse::<SocketAddr>() {
        Ok(endpoint) => Some(endpoint),
        Err(error) => {
            tracing::warn!(
                subsystem = "peer_transport",
                action = "parse_endpoint",
                outcome = "skipped",
                %raw,
                %error,
                "parse_peer_endpoint: invalid address format"
            );
            None
        }
    }
}

impl boundary::sealed::Sealed for PeerClientTransport {}

impl ClientTransport for PeerClientTransport {
    fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        let endpoint = self
            .endpoint
            .ok_or_else(remote_peer_endpoint_not_configured_error)?;
        self.send_with_outcome_persistence(endpoint, request)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PeerTransportRuntime {
    client: PeerClientTransport,
    server: Arc<PeerServerTransport>,
}

impl Default for PeerTransportRuntime {
    fn default() -> Self {
        Self::new(None)
    }
}

impl PeerTransportRuntime {
    pub(crate) fn new(replay_store: Option<Arc<dyn RemoteReplayStore>>) -> Self {
        Self::new_with_observability(
            replay_store,
            PeerTransportConfig::default(),
            SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
            RuntimeStatusCache::new(),
        )
    }

    pub(crate) fn new_with_observability(
        replay_store: Option<Arc<dyn RemoteReplayStore>>,
        config: PeerTransportConfig,
        observability: SubsystemObservability,
        status_cache: RuntimeStatusCache,
    ) -> Self {
        Self {
            server: Arc::new(PeerServerTransport::new(
                config.peer_listen_addr,
                observability.clone(),
                status_cache,
            )),
            client: PeerClientTransport::new_with_observability(
                replay_store,
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

    pub(crate) fn send_to_endpoint(
        &self,
        endpoint: SocketAddr,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, AtmError> {
        self.client.send_with_outcome_persistence(endpoint, request)
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
        self.server.bound_addr()
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
            )),
            client: PeerClientTransport::new_for_test(endpoint, config, replay_db_path),
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
            )),
            client: PeerClientTransport::new_with_observability(None, config, observability),
        }
    }

    #[cfg(test)]
    pub(crate) fn bound_addr_for_test(&self) -> Option<SocketAddr> {
        self.server.bound_addr_for_test()
    }

    #[cfg(test)]
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
