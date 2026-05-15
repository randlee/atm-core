use std::io::{self, Write};
use std::net::{SocketAddr, TcpStream};
#[cfg(test)]
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::RemoteReplayStateRecord;
use atm_core::AtmConfig;
use atm_core::boundary::{self, AtmProtocol, ClientTransport, ConfigLoadRequest, MessageKey};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::protocol::{
    JsonAtmProtocolCodec, RequestEnvelope, ResponseEnvelope, TeamMemberHeartbeatRequest,
};
use atm_core::types::{AgentName, IsoTimestamp, TeamName};

use crate::{DaemonSubsystem, SubsystemObservability};

// Architecture authority: docs/architecture.md §21.6.4 daemon operational
// defaults and remote peer transport rules.
const PEER_CONNECT_DEADLINE: Duration = Duration::from_secs(5);
const PEER_IO_DEADLINE: Duration = Duration::from_secs(5);
const DEFAULT_REMOTE_RETRY_BUDGET: Duration = Duration::from_secs(30);
const MIN_REMOTE_RETRY_BUDGET: Duration = Duration::from_secs(1);
const INITIAL_RETRY_BACKOFF: Duration = Duration::from_millis(250);
const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(5);
const MAX_REPLAY_RESUME_SWEEP_BUDGET: Duration = Duration::from_secs(30);
const PEER_BLOCKING_SLICE_DEADLINE: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PeerTransportConfig {
    pub(crate) remote_retry_budget: Duration,
}

impl Default for PeerTransportConfig {
    fn default() -> Self {
        Self {
            remote_retry_budget: DEFAULT_REMOTE_RETRY_BUDGET,
        }
    }
}

impl PeerTransportConfig {
    pub(crate) fn from_config(config: Option<&AtmConfig>) -> Result<Self, AtmError> {
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
        Ok(Self {
            remote_retry_budget,
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
                "ignoring non-unicode ATM_DAEMON_PEER_ADDR value"
            );
            None
        }
    }
}

pub(crate) trait RemoteReplayStore: Send + Sync + std::fmt::Debug {
    fn enqueue(&self, record: RemoteReplayStateRecord) -> Result<(), AtmError>;

    fn load_all(&self) -> Result<Vec<RemoteReplayStateRecord>, AtmError>;

    fn delete(
        &self,
        team: &TeamName,
        agent: &AgentName,
        message_key: &MessageKey,
    ) -> Result<(), AtmError>;

    fn purge_expired(&self, now: IsoTimestamp) -> Result<usize, AtmError>;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReplayResumeSummary {
    pub(crate) delivered: usize,
    pub(crate) retained: usize,
    pub(crate) purged_expired: usize,
}

#[derive(Debug, Clone)]
struct PeerClientTransport {
    endpoint: Option<SocketAddr>,
    config: PeerTransportConfig,
    replay_store: Option<Arc<dyn RemoteReplayStore>>,
    codec: JsonAtmProtocolCodec,
    observability: SubsystemObservability,
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
            crate::sqlite_remote_replay_store_from_path(replay_db_path).expect("replay store");
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
        let mut delivered = 0usize;
        let mut retained = 0usize;
        let sweep_deadline = Instant::now() + MAX_REPLAY_RESUME_SWEEP_BUDGET;
        let total_records = records.len();
        for (index, mut record) in records.into_iter().enumerate() {
            if Instant::now() >= sweep_deadline {
                let deferred = total_records.saturating_sub(index);
                retained += deferred;
                tracing::warn!(
                    remaining_records = deferred,
                    sweep_budget_secs = MAX_REPLAY_RESUME_SWEEP_BUDGET.as_secs(),
                    "daemon remote replay resume sweep hit its total startup budget; remaining records stay queued"
                );
                self.observability.emit_or_warn(
                    "resume_pending_replay",
                    "degraded",
                    "daemon remote replay resume sweep hit its total startup budget and left queued records for later retry",
                );
                break;
            }
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
                    record.last_error = Some(error.code.to_string());
                    tracing::warn!(
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
                request = ?request,
                "remote delivery outcome is unknown but this request family does not support durable replay persistence",
            );
            // The original RemoteDeliveryOutcomeUnknown already carries the shared operator
            // guidance for retry-vs-resume, so unsupported request families intentionally avoid
            // fabricating a second replay-specific wrapper here.
            return Ok(());
        };
        self.persist_replay_request(team, agent, message_key, request.clone())
    }

    fn send_to_endpoint(
        &self,
        endpoint: SocketAddr,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, AtmError> {
        let request_id = atm_core::protocol::next_request_id();
        let frame = self.codec.request_to_frame(request_id, request)?;
        let deadline = Instant::now() + self.config.remote_retry_budget;
        let terminate = daemon_terminate_flag()?;
        let mut backoff = INITIAL_RETRY_BACKOFF;
        let mut attempt = 0u32;

        loop {
            if terminate.load(Ordering::SeqCst) {
                return Err(
                    AtmError::daemon_unavailable(
                        "daemon shutdown interrupted remote peer delivery before the next network attempt",
                    )
                    .with_recovery(
                        "Retry the daemon operation after atm-daemon restarts and resumes pending remote replay work.",
                    ),
                );
            }
            match self.send_once(endpoint, &frame) {
                Ok(response) => {
                    tracing::info!(
                        peer_addr = %endpoint,
                        attempt,
                        "daemon peer delivery succeeded"
                    );
                    self.observability.emit_or_warn(
                        "send_to_endpoint",
                        "ok",
                        "daemon peer delivery succeeded",
                    );
                    return Ok(response);
                }
                Err(failure) if failure.kind == AttemptFailureKind::Retryable => {
                    let now = Instant::now();
                    if now >= deadline {
                        tracing::error!(
                            peer_addr = %endpoint,
                            attempt,
                            error_code = %failure.error.code,
                            error_message = %failure.error.message,
                            "daemon peer delivery exhausted retry budget"
                        );
                        self.observability.emit_or_warn(
                            "send_to_endpoint",
                            "failed",
                            "daemon peer delivery exhausted its retry budget",
                        );
                        return Err(failure.error);
                    }
                    let remaining = deadline.saturating_duration_since(now);
                    let sleep_for =
                        jittered_backoff(backoff, jitter_seed(endpoint, attempt)).min(remaining);
                    tracing::warn!(
                        peer_addr = %endpoint,
                        attempt,
                        sleep_ms = sleep_for.as_millis(),
                        error_code = %failure.error.code,
                        error_message = %failure.error.message,
                        "daemon peer delivery hit retryable failure"
                    );
                    self.observability.emit_or_warn(
                        "send_to_endpoint",
                        "degraded",
                        "daemon peer delivery hit a retryable failure",
                    );
                    if wait_for_retry_backoff(&terminate, sleep_for) {
                        return Err(
                            AtmError::daemon_unavailable(
                                "daemon shutdown interrupted remote peer retry backoff",
                            )
                            .with_recovery(
                                "Retry the daemon operation after atm-daemon restarts and resumes pending remote replay work.",
                            ),
                        );
                    }
                    backoff = backoff.saturating_mul(2).min(MAX_RETRY_BACKOFF);
                    attempt = attempt.saturating_add(1);
                }
                Err(failure) => {
                    let level = if failure.kind == AttemptFailureKind::OutcomeUnknown {
                        "outcome_unknown"
                    } else {
                        "non_retryable"
                    };
                    tracing::error!(
                        peer_addr = %endpoint,
                        attempt,
                        failure_kind = level,
                        error_code = %failure.error.code,
                        error_message = %failure.error.message,
                        "daemon peer delivery failed"
                    );
                    self.observability.emit_or_warn(
                        "send_to_endpoint",
                        "failed",
                        "daemon peer delivery failed with a non-retryable or outcome-unknown error",
                    );
                    return Err(failure.error);
                }
            }
        }
    }

    fn send_once(
        &self,
        endpoint: SocketAddr,
        request_frame: &atm_core::protocol::FramePayload,
    ) -> Result<ResponseEnvelope, Box<AttemptFailure>> {
        let mut stream = TcpStream::connect_timeout(
            &endpoint,
            PEER_CONNECT_DEADLINE.min(PEER_BLOCKING_SLICE_DEADLINE),
        )
        .map_err(|source| {
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
            })?;
        stream
            .set_read_timeout(Some(PEER_IO_DEADLINE.min(PEER_BLOCKING_SLICE_DEADLINE)))
            .map_err(|source| {
                Box::new(AttemptFailure {
                    kind: AttemptFailureKind::Retryable,
                    error: AtmError::daemon_unavailable(
                        "failed to apply remote peer read deadline",
                    )
                    .with_recovery(
                        "Restart atm-daemon and retry after the peer socket can accept bounded read deadlines again.",
                    )
                    .with_source(source),
                })
            })?;
        stream
            .set_write_timeout(Some(PEER_IO_DEADLINE.min(PEER_BLOCKING_SLICE_DEADLINE)))
            .map_err(|source| {
                Box::new(AttemptFailure {
                    kind: AttemptFailureKind::Retryable,
                    error: AtmError::daemon_unavailable(
                        "failed to apply remote peer write deadline",
                    )
                    .with_recovery(
                        "Restart atm-daemon and retry after the peer socket can accept bounded write deadlines again.",
                    )
                    .with_source(source),
                })
            })?;
        atm_core::protocol::write_frame(
            &mut stream,
            request_frame,
            "failed to write remote peer request frame",
        )
        .map_err(|error| {
            Box::new(AttemptFailure {
                kind: AttemptFailureKind::Retryable,
                error,
            })
        })?;
        stream.flush().map_err(|source| {
            Box::new(AttemptFailure {
                kind: AttemptFailureKind::OutcomeUnknown,
                error: AtmError::remote_delivery_outcome_unknown(
                    "failed to flush the remote peer request frame before waiting for a response",
                )
                .with_source(source),
            })
        })?;
        let Some(response_frame) = atm_core::protocol::read_frame(
            &mut stream,
            "failed to read remote peer response frame",
            "remote peer response frame exceeded the maximum supported size",
        )
        .map_err(|error| {
            Box::new(AttemptFailure {
                kind: AttemptFailureKind::OutcomeUnknown,
                error,
            })
        })?
        else {
            return Err(Box::new(AttemptFailure {
                kind: AttemptFailureKind::OutcomeUnknown,
                error: AtmError::remote_delivery_outcome_unknown(
                    "remote peer closed the connection before returning a response frame",
                )
                .with_recovery(
                    "Check the destination daemon or mailbox before retrying. If local durable replay is enabled, let the daemon resume the pending handoff rather than guessing success.",
                ),
            }));
        };
        let (response_id, response) =
            self.codec
                .response_from_frame(response_frame)
                .map_err(|error| {
                    Box::new(AttemptFailure {
                        kind: AttemptFailureKind::NonRetryable,
                        error: AtmError::daemon_unavailable(
                            "failed to decode remote peer response frame",
                        )
                        .with_recovery(
                            "Align the peer daemon builds so both sides speak the same ATM daemon protocol before retrying the remote delivery.",
                        )
                        .with_source(error),
                    })
                })?;
        if response_id != request_frame.request_id {
            return Err(Box::new(AttemptFailure {
                kind: AttemptFailureKind::NonRetryable,
                error: AtmError::daemon_unavailable(format!(
                    "remote peer response request_id {} did not match request_id {}",
                    response_id, request_frame.request_id
                ))
                .with_recovery(
                    "Align the peer daemon builds so both sides use the same ATM daemon protocol contract before retrying.",
                ),
            }));
        }
        match response {
            ResponseEnvelope::Error(error) => Err(Box::new(AttemptFailure {
                kind: AttemptFailureKind::NonRetryable,
                error: error.into_atm_error(),
            })),
            response => Ok(response),
        }
    }
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
    Ok(crate::lifecycle_control::LifecycleControlSourceAdapter::install()?.terminate_flag())
}

impl boundary::sealed::Sealed for PeerClientTransport {}

impl ClientTransport for PeerClientTransport {
    fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        let endpoint = self
            .endpoint
            .ok_or_else(remote_peer_endpoint_not_configured_error)?;
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

#[derive(Debug, Clone)]
pub(crate) struct PeerTransportRuntime {
    client: PeerClientTransport,
}

impl Default for PeerTransportRuntime {
    fn default() -> Self {
        Self::new_for_default()
    }
}

impl PeerTransportRuntime {
    fn new_for_default() -> Self {
        Self {
            client: PeerClientTransport {
                endpoint: daemon_peer_endpoint_from_env(),
                config: PeerTransportConfig::default(),
                replay_store: None,
                codec: JsonAtmProtocolCodec,
                observability: SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
            },
        }
    }

    pub(crate) fn new_with_observability(
        replay_store: Option<Arc<dyn RemoteReplayStore>>,
        observability: SubsystemObservability,
    ) -> Result<Self, AtmError> {
        let config = std::env::current_dir()
            .ok()
            .and_then(|current_dir| match atm_core::load_workspace_config(ConfigLoadRequest {
                current_dir,
            }) {
                Ok(response) => response.config,
                Err(error) => {
                    tracing::warn!(
                        %error,
                        "failed to load workspace config while constructing peer transport; using default remote retry budget"
                    );
                    None
                }
            })
            .map(|config| PeerTransportConfig::from_config(Some(&config)))
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            client: PeerClientTransport::new_with_observability(
                replay_store,
                config,
                observability,
            ),
        })
    }

    #[cfg(test)]
    pub(crate) fn client_transport(&self) -> &dyn ClientTransport {
        &self.client
    }

    pub(crate) fn resume_pending_replay(&self) -> Result<ReplayResumeSummary, AtmError> {
        self.client.resume_pending_replay()
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        endpoint: SocketAddr,
        config: PeerTransportConfig,
        replay_db_path: PathBuf,
    ) -> Self {
        Self {
            client: PeerClientTransport::new_for_test(endpoint, config, replay_db_path),
        }
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
    // Team/member names are already validated ATM identifiers, pid is numeric, and RFC3339
    // timestamps are never blank, so this synthetic replay key cannot violate MessageKey's
    // non-empty invariant.
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
                %raw,
                %error,
                "parse_peer_endpoint: invalid address format"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AttemptFailureKind, PeerClientTransport, PeerTransportConfig, PeerTransportRuntime,
        classify_io_error, jittered_backoff, remote_peer_endpoint_not_configured_error,
        remote_replay_persistence_failed_error, remote_replay_store_not_configured_error,
    };
    use crate::lifecycle_control::LifecycleControlSourceAdapter;
    use crate::test_support::LifecycleFlagResetGuard;
    use crate::{DaemonSubsystem, SubsystemObservability};
    use atm_core::boundary::{AtmProtocol, ClientTransport, MessageKey};
    use atm_core::doctor::DoctorQuery;
    use atm_core::error::AtmErrorCode;
    use atm_core::protocol::{
        HeartbeatActivity, ProtocolErrorEnvelope, RequestEnvelope, ResponseEnvelope,
        RuntimeMemberState, TeamMemberHeartbeatRequest, TeamMemberHeartbeatResponse,
    };
    use atm_core::types::{AgentName, IsoTimestamp, TeamName};
    use serial_test::serial;
    use std::io::{self, Read, Write};
    use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    fn read_request_frame(
        stream: &mut TcpStream,
    ) -> (
        atm_core::protocol::RequestId,
        RequestEnvelope,
        super::JsonAtmProtocolCodec,
    ) {
        let codec = super::JsonAtmProtocolCodec;
        let frame = atm_core::protocol::read_frame(
            stream,
            "read request",
            "request frame exceeded frame limit",
        )
        .expect("read frame")
        .expect("request frame");
        let (request_id, request) = codec.request_from_frame(frame).expect("decode request");
        (request_id, request, codec)
    }

    fn write_response_frame(
        stream: &mut TcpStream,
        codec: &super::JsonAtmProtocolCodec,
        request_id: atm_core::protocol::RequestId,
        response: ResponseEnvelope,
    ) {
        let frame = codec
            .response_to_frame(request_id, response)
            .expect("response frame");
        atm_core::protocol::write_frame(stream, &frame, "write response").expect("write response");
        stream.flush().expect("flush response");
    }

    fn install_shared_lifecycle_reset_guard() -> LifecycleFlagResetGuard {
        let lifecycle = LifecycleControlSourceAdapter::install().expect("install lifecycle");
        LifecycleFlagResetGuard::install(lifecycle)
    }

    #[test]
    fn jittered_backoff_stays_within_twenty_percent_window() {
        let base = Duration::from_millis(250);
        let low = jittered_backoff(base, 0);
        let high = jittered_backoff(base, 4_000);
        assert_eq!(low, Duration::from_millis(200));
        assert_eq!(high, Duration::from_millis(300));
    }

    #[test]
    fn classify_io_error_covers_retryable_and_non_retryable_variants() {
        for kind in [
            io::ErrorKind::TimedOut,
            io::ErrorKind::Interrupted,
            io::ErrorKind::ConnectionRefused,
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::ConnectionAborted,
            io::ErrorKind::BrokenPipe,
            io::ErrorKind::HostUnreachable,
        ] {
            assert_eq!(
                classify_io_error(&io::Error::new(kind, "retryable")),
                AttemptFailureKind::Retryable
            );
        }
        assert_eq!(
            classify_io_error(&io::Error::other("non-retryable")),
            AttemptFailureKind::NonRetryable
        );
    }

    #[test]
    fn persist_replay_request_requires_configured_replay_store_with_recovery() {
        let team: TeamName = "test-team".parse().expect("team");
        let agent: AgentName = "test-agent".parse().expect("agent");
        let client = PeerClientTransport {
            endpoint: Some(SocketAddr::from((Ipv4Addr::LOCALHOST, 7001))),
            config: PeerTransportConfig::default(),
            replay_store: None,
            codec: super::JsonAtmProtocolCodec,
            observability: SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
        };
        let error = client
            .persist_replay_request(
                team.clone(),
                agent.clone(),
                MessageKey::new("atm:test-remote-replay-store-missing").expect("message key"),
                RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
                    team,
                    member: agent,
                    pid: 42,
                    observed_at: IsoTimestamp::now(),
                    activity: HeartbeatActivity::Idle,
                }),
            )
            .expect_err("missing replay store should fail closed");
        assert_eq!(error.code, AtmErrorCode::DaemonUnavailable);
        assert!(
            error
                .recovery
                .as_deref()
                .expect("recovery guidance")
                .contains("host-scoped ATM durable replay store")
        );
    }

    #[test]
    fn persist_replay_request_missing_endpoint_matches_send_surface_contract() {
        let tempdir = TempDir::new().expect("tempdir");
        let team: TeamName = "test-team".parse().expect("team");
        let agent: AgentName = "test-agent".parse().expect("agent");
        let replay_store =
            crate::sqlite_remote_replay_store_from_path(tempdir.path().join("mail.db"))
                .expect("replay store");
        let client = PeerClientTransport {
            endpoint: None,
            config: PeerTransportConfig::default(),
            replay_store: Some(replay_store),
            codec: super::JsonAtmProtocolCodec,
            observability: SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
        };
        let persist_error = client
            .persist_replay_request(
                team.clone(),
                agent.clone(),
                MessageKey::new("atm:test-remote-peer-endpoint-missing").expect("message key"),
                RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
                    team,
                    member: agent,
                    pid: 43,
                    observed_at: IsoTimestamp::now(),
                    activity: HeartbeatActivity::Idle,
                }),
            )
            .expect_err("missing endpoint should fail closed");
        let send_error = remote_peer_endpoint_not_configured_error();
        assert_eq!(persist_error.code, send_error.code);
        assert_eq!(persist_error.recovery, send_error.recovery);
    }

    #[test]
    fn persist_replay_request_invalid_retry_budget_reports_actionable_recovery() {
        let tempdir = TempDir::new().expect("tempdir");
        let team: TeamName = "test-team".parse().expect("team");
        let agent: AgentName = "test-agent".parse().expect("agent");
        let replay_store =
            crate::sqlite_remote_replay_store_from_path(tempdir.path().join("mail.db"))
                .expect("replay store");
        let client = PeerClientTransport {
            endpoint: Some(SocketAddr::from((Ipv4Addr::LOCALHOST, 7002))),
            config: PeerTransportConfig {
                remote_retry_budget: Duration::MAX,
            },
            replay_store: Some(replay_store),
            codec: super::JsonAtmProtocolCodec,
            observability: SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
        };
        let error = client
            .persist_replay_request(
                team.clone(),
                agent.clone(),
                MessageKey::new("atm:test-remote-retry-budget-invalid").expect("message key"),
                RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
                    team,
                    member: agent,
                    pid: 44,
                    observed_at: IsoTimestamp::now(),
                    activity: HeartbeatActivity::Idle,
                }),
            )
            .expect_err("invalid retry budget should fail closed");
        assert_eq!(error.code, AtmErrorCode::DaemonUnavailable);
        assert!(
            error
                .recovery
                .as_deref()
                .expect("recovery guidance")
                .contains("remote retry budget configuration")
        );
    }

    #[test]
    fn protocol_envelope_preserves_replay_persistence_failure_contract() {
        let error =
            remote_replay_persistence_failed_error(remote_replay_store_not_configured_error());
        let envelope = ProtocolErrorEnvelope::from_error(&error);
        let round_trip = envelope.into_atm_error();
        assert_eq!(round_trip.code, AtmErrorCode::RemoteDeliveryOutcomeUnknown);
        assert_eq!(round_trip.message, error.message);
        assert_eq!(round_trip.recovery, error.recovery);
    }

    #[test]
    fn wildcard_bindings_survive_connection_churn_and_explicit_binds_require_reload() {
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("wildcard listener");
        let endpoint = SocketAddr::from((
            Ipv4Addr::LOCALHOST,
            listener.local_addr().expect("addr").port(),
        ));

        let server = thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut buffer = [0u8; 1];
                stream.read_exact(&mut buffer).expect("read");
            }
        });

        for _ in 0..2 {
            let mut stream = TcpStream::connect(endpoint).expect("connect");
            stream.write_all(&[1]).expect("write");
        }
        server.join().expect("server join");

        let bind_error = TcpListener::bind((Ipv4Addr::new(198, 51, 100, 10), 0))
            .expect_err("explicit bind failure");
        assert_eq!(bind_error.kind(), io::ErrorKind::AddrNotAvailable);
    }

    #[test]
    #[serial]
    fn peer_transport_round_trips_one_heartbeat_request() {
        let _reset = install_shared_lifecycle_reset_guard();
        let tempdir = TempDir::new().expect("tempdir");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("listener");
        let endpoint = listener.local_addr().expect("addr");
        let transport = PeerTransportRuntime::new_for_test(
            endpoint,
            PeerTransportConfig::default(),
            tempdir.path().join("mail.db"),
        );
        let team: TeamName = "test-team".parse().expect("team");
        let member: AgentName = "test-member".parse().expect("member");
        let request = RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: team.clone(),
            member: member.clone(),
            pid: 42,
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::Idle,
        });
        let expected = ResponseEnvelope::Heartbeat(TeamMemberHeartbeatResponse {
            team,
            member,
            pid: 42,
            pid_changed: false,
            state: RuntimeMemberState::Idle,
            last_active_at: Some(IsoTimestamp::now()),
        });

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let (request_id, _request, codec) = read_request_frame(&mut stream);
            write_response_frame(&mut stream, &codec, request_id, expected);
        });

        let response = transport
            .client_transport()
            .send(request)
            .expect("response");
        match response {
            ResponseEnvelope::Heartbeat(response) => {
                assert_eq!(response.state, RuntimeMemberState::Idle)
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[test]
    #[serial]
    fn peer_transport_aborts_before_connect_when_terminate_is_requested() {
        const TEST_TEAM: &str = "test-team";
        const TEST_MEMBER: &str = "test-sender";

        let _reset = install_shared_lifecycle_reset_guard();
        let lifecycle = LifecycleControlSourceAdapter::install().expect("install lifecycle");
        lifecycle.set_terminate_for_test(true);

        let tempdir = TempDir::new().expect("tempdir");
        let endpoint = {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("listener");
            let endpoint = listener.local_addr().expect("addr");
            drop(listener);
            endpoint
        };
        let transport = PeerTransportRuntime::new_for_test(
            endpoint,
            PeerTransportConfig {
                remote_retry_budget: Duration::from_secs(1),
            },
            tempdir.path().join("replay.db"),
        );

        let error = transport
            .client_transport()
            .send(RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
                team: TEST_TEAM.parse().expect("team"),
                member: TEST_MEMBER.parse().expect("member"),
                pid: std::process::id(),
                observed_at: IsoTimestamp::now(),
                activity: HeartbeatActivity::ActiveToolUse,
            }))
            .expect_err("terminate should short-circuit before connect");
        assert_eq!(error.code, AtmErrorCode::DaemonUnavailable);
        assert!(
            error.message.contains("before the next network attempt"),
            "unexpected error message: {}",
            error.message
        );
    }

    #[test]
    #[serial]
    fn peer_transport_uses_port_zero_listener_handoff_without_rebind_race() {
        let _reset = install_shared_lifecycle_reset_guard();
        let tempdir = TempDir::new().expect("tempdir");
        let (endpoint_tx, endpoint_rx) = mpsc::channel();
        let team: TeamName = "test-team".parse().expect("team");
        let member: AgentName = "test-member".parse().expect("member");
        let server_team = team.clone();
        let server_member = member.clone();

        thread::spawn(move || {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("listener");
            endpoint_tx
                .send(listener.local_addr().expect("addr"))
                .expect("endpoint");
            let (mut stream, _) = listener.accept().expect("accept");
            let (request_id, _request, codec) = read_request_frame(&mut stream);
            let response = ResponseEnvelope::Heartbeat(TeamMemberHeartbeatResponse {
                team: server_team,
                member: server_member,
                pid: 7,
                pid_changed: false,
                state: RuntimeMemberState::Active,
                last_active_at: Some(IsoTimestamp::now()),
            });
            write_response_frame(&mut stream, &codec, request_id, response);
        });

        let endpoint = endpoint_rx.recv().expect("endpoint");

        let transport = PeerTransportRuntime::new_for_test(
            endpoint,
            PeerTransportConfig {
                remote_retry_budget: Duration::from_millis(600),
            },
            tempdir.path().join("mail.db"),
        );
        let request = RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: team.clone(),
            member: member.clone(),
            pid: 7,
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::ActiveToolUse,
        });
        let (send_started_tx, send_started_rx) = mpsc::channel();
        let (response_tx, response_rx) = mpsc::channel();

        let transport_for_thread = transport.clone();
        thread::spawn(move || {
            send_started_tx.send(()).expect("send started");
            response_tx
                .send(transport_for_thread.client.send(request))
                .expect("response sent");
        });

        send_started_rx.recv().expect("send started");
        let response = response_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("response wait")
            .expect("response delivered");
        assert!(matches!(response, ResponseEnvelope::Heartbeat(_)));
    }

    #[test]
    #[serial]
    fn peer_transport_reports_outcome_unknown_after_send_without_response() {
        let _reset = install_shared_lifecycle_reset_guard();
        let tempdir = TempDir::new().expect("tempdir");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("listener");
        let endpoint = listener.local_addr().expect("addr");
        let transport = PeerTransportRuntime::new_for_test(
            endpoint,
            PeerTransportConfig::default(),
            tempdir.path().join("mail.db"),
        );
        let team: TeamName = "test-team".parse().expect("team");
        let member: AgentName = "test-member".parse().expect("member");
        let request = RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team,
            member,
            pid: 11,
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::Idle,
        });

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _ = read_request_frame(&mut stream);
        });

        let error = transport
            .client_transport()
            .send(request)
            .expect_err("outcome unknown");
        assert_eq!(error.code, AtmErrorCode::RemoteDeliveryOutcomeUnknown);
    }

    #[test]
    #[serial]
    fn peer_transport_treats_remote_error_envelope_as_non_retryable() {
        let _reset = install_shared_lifecycle_reset_guard();
        let tempdir = TempDir::new().expect("tempdir");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("listener");
        let endpoint = listener.local_addr().expect("addr");
        let transport = PeerTransportRuntime::new_for_test(
            endpoint,
            PeerTransportConfig::default(),
            tempdir.path().join("mail.db"),
        );
        let team: TeamName = "test-team".parse().expect("team");
        let member: AgentName = "test-member".parse().expect("member");
        let request = RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team,
            member,
            pid: 12,
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::Idle,
        });

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let (request_id, _request, codec) = read_request_frame(&mut stream);
            let response = ResponseEnvelope::Error(ProtocolErrorEnvelope {
                code: AtmErrorCode::DaemonUnavailable,
                message: "remote rejected request".to_string(),
                recovery: None,
            });
            write_response_frame(&mut stream, &codec, request_id, response);
        });

        let error = transport
            .client_transport()
            .send(request)
            .expect_err("remote reject");
        assert_eq!(error.code, AtmErrorCode::DaemonUnavailable);
        assert!(error.message.contains("remote rejected request"));
    }

    #[test]
    #[serial]
    fn replay_resume_replays_and_deletes_delivered_rows() {
        let _reset = install_shared_lifecycle_reset_guard();
        let tempdir = TempDir::new().expect("tempdir");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("listener");
        let endpoint = listener.local_addr().expect("addr");
        let db_path = tempdir.path().join("mail.db");
        let transport = PeerTransportRuntime::new_for_test(
            endpoint,
            PeerTransportConfig::default(),
            db_path.clone(),
        );
        let team: TeamName = "test-team".parse().expect("team");
        let member: AgentName = "test-member".parse().expect("member");
        let request = RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: team.clone(),
            member: member.clone(),
            pid: 21,
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::Idle,
        });
        transport
            .persist_replay_request(
                team.clone(),
                member.clone(),
                MessageKey::new("atm:test-remote-replay").expect("message key"),
                request,
            )
            .expect("persist");

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let (request_id, _request, codec) = read_request_frame(&mut stream);
            let response = ResponseEnvelope::Heartbeat(TeamMemberHeartbeatResponse {
                team,
                member,
                pid: 21,
                pid_changed: false,
                state: RuntimeMemberState::Idle,
                last_active_at: Some(IsoTimestamp::now()),
            });
            write_response_frame(&mut stream, &codec, request_id, response);
        });

        let summary = transport.resume_pending_replay().expect("resume");
        assert_eq!(summary.delivered, 1);
        assert_eq!(summary.retained, 0);
        let pending = transport
            .load_pending_replay_records()
            .expect("load pending");
        assert!(pending.is_empty());
    }

    #[test]
    #[serial]
    fn outcome_unknown_persists_replay_request_for_restart_resume() {
        let _reset = install_shared_lifecycle_reset_guard();
        let tempdir = TempDir::new().expect("tempdir");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("listener");
        let endpoint = listener.local_addr().expect("addr");
        let db_path = tempdir.path().join("mail.db");
        let transport = PeerTransportRuntime::new_for_test(
            endpoint,
            PeerTransportConfig::default(),
            db_path.clone(),
        );
        let team: TeamName = "test-team".parse().expect("team");
        let member: AgentName = "test-member".parse().expect("member");
        let request = RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: team.clone(),
            member: member.clone(),
            pid: 77,
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::Idle,
        });

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _ = read_request_frame(&mut stream);
        });

        let error = transport
            .client_transport()
            .send(request)
            .expect_err("outcome unknown");
        assert_eq!(error.code, AtmErrorCode::RemoteDeliveryOutcomeUnknown);

        let pending = transport
            .load_pending_replay_records()
            .expect("load pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].team, team);
        assert_eq!(pending[0].agent, member);
    }

    #[test]
    #[serial]
    fn unsupported_request_family_keeps_shared_outcome_unknown_recovery() {
        let _reset = install_shared_lifecycle_reset_guard();
        let tempdir = TempDir::new().expect("tempdir");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("listener");
        let endpoint = listener.local_addr().expect("addr");
        let db_path = tempdir.path().join("mail.db");
        let transport = PeerTransportRuntime::new_for_test(
            endpoint,
            PeerTransportConfig::default(),
            db_path.clone(),
        );
        let request = RequestEnvelope::Doctor(DoctorQuery {
            home_dir: tempdir.path().to_path_buf(),
            current_dir: tempdir.path().to_path_buf(),
            team_override: None,
        });

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _ = read_request_frame(&mut stream);
        });

        let error = transport
            .client_transport()
            .send(request)
            .expect_err("outcome unknown");
        assert_eq!(error.code, AtmErrorCode::RemoteDeliveryOutcomeUnknown);
        assert!(
            error
                .recovery
                .as_deref()
                .expect("recovery guidance")
                .contains("let the daemon resume the pending handoff")
        );
        let pending = transport
            .load_pending_replay_records()
            .expect("load pending");
        assert!(pending.is_empty());
    }

    #[test]
    #[serial]
    fn replay_resume_after_restart_delivers_once_and_clears_duplicate_delivery() {
        let _reset = install_shared_lifecycle_reset_guard();
        let tempdir = TempDir::new().expect("tempdir");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("listener");
        let endpoint = listener.local_addr().expect("addr");
        let db_path = tempdir.path().join("mail.db");
        let first = PeerTransportRuntime::new_for_test(
            endpoint,
            PeerTransportConfig::default(),
            db_path.clone(),
        );
        let second = PeerTransportRuntime::new_for_test(
            endpoint,
            PeerTransportConfig::default(),
            db_path.clone(),
        );
        let team: TeamName = "test-team".parse().expect("team");
        let member: AgentName = "test-member".parse().expect("member");
        let request = RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: team.clone(),
            member: member.clone(),
            pid: 32,
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::Idle,
        });
        first
            .persist_replay_request(
                team.clone(),
                member.clone(),
                MessageKey::new("atm:test-remote-replay-restart").expect("message key"),
                request,
            )
            .expect("persist");

        let (deliveries_tx, deliveries_rx) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let (request_id, _request, codec) = read_request_frame(&mut stream);
            deliveries_tx.send(()).expect("delivery sent");
            let response = ResponseEnvelope::Heartbeat(TeamMemberHeartbeatResponse {
                team,
                member,
                pid: 32,
                pid_changed: false,
                state: RuntimeMemberState::Idle,
                last_active_at: Some(IsoTimestamp::now()),
            });
            write_response_frame(&mut stream, &codec, request_id, response);
        });

        let summary = second.resume_pending_replay().expect("resume");
        assert_eq!(summary.delivered, 1);
        deliveries_rx.recv().expect("delivery");

        let pending = second.load_pending_replay_records().expect("load pending");
        assert!(pending.is_empty());

        let summary = second.resume_pending_replay().expect("second resume");
        assert_eq!(summary.delivered, 0);
        assert_eq!(summary.retained, 0);
    }

    #[test]
    fn replay_store_upsert_deduplicates_same_message_key() {
        let tempdir = TempDir::new().expect("tempdir");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("listener");
        let endpoint = listener.local_addr().expect("addr");
        drop(listener);
        let transport = PeerTransportRuntime::new_for_test(
            endpoint,
            PeerTransportConfig::default(),
            tempdir.path().join("mail.db"),
        );
        let team: TeamName = "test-team".parse().expect("team");
        let member: AgentName = "test-member".parse().expect("member");
        let message_key = MessageKey::new("atm:test-remote-replay-dedup").expect("message key");
        let first_request = RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: team.clone(),
            member: member.clone(),
            pid: 11,
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::Idle,
        });
        let second_request = RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: team.clone(),
            member: member.clone(),
            pid: 12,
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::ActiveToolUse,
        });

        transport
            .persist_replay_request(
                team.clone(),
                member.clone(),
                message_key.clone(),
                first_request,
            )
            .expect("persist first");
        transport
            .persist_replay_request(team, member, message_key, second_request)
            .expect("persist second");

        let pending = transport
            .load_pending_replay_records()
            .expect("load pending");
        assert_eq!(pending.len(), 1);
        let RequestEnvelope::Heartbeat(heartbeat) = &pending[0].request else {
            panic!("expected heartbeat replay record");
        };
        assert_eq!(heartbeat.pid, 12);
    }
}
