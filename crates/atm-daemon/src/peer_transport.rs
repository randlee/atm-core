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

// Architecture authority: docs/architecture.md §21.6.4 daemon operational
// defaults and remote peer transport rules.
const PEER_CONNECT_DEADLINE: Duration = Duration::from_secs(5);
const PEER_IO_DEADLINE: Duration = Duration::from_secs(5);
const DEFAULT_REMOTE_RETRY_BUDGET: Duration = Duration::from_secs(30);
const INITIAL_RETRY_BACKOFF: Duration = Duration::from_millis(250);
const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(5);

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
    pub(crate) fn from_config(config: Option<&AtmConfig>) -> Self {
        config
            .map(|config| Self {
                remote_retry_budget: config.daemon.remote_retry_budget,
            })
            .unwrap_or_default()
    }
}

fn daemon_peer_endpoint_from_env() -> Option<SocketAddr> {
    match std::env::var("ATM_DAEMON_PEER_ADDR") {
        Ok(raw) => parse_peer_endpoint(&raw),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            tracing::warn!("ignoring non-unicode ATM_DAEMON_PEER_ADDR value");
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
}

impl PeerClientTransport {
    fn new(replay_store: Option<Arc<dyn RemoteReplayStore>>) -> Self {
        let endpoint = daemon_peer_endpoint_from_env();
        let config = std::env::current_dir()
            .ok()
            .and_then(|current_dir| {
                atm_core::boundary_support::load_workspace_config(ConfigLoadRequest { current_dir })
                    .ok()
                    .and_then(|response| response.config)
            })
            .map(|config| PeerTransportConfig::from_config(Some(&config)))
            .unwrap_or_default();
        Self {
            endpoint,
            config,
            replay_store,
            codec: JsonAtmProtocolCodec,
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
            return Err(AtmError::daemon_unavailable(
                "remote replay store is not configured",
            ));
        };
        let endpoint = self.endpoint.ok_or_else(|| {
            AtmError::daemon_unavailable("remote peer endpoint is not configured")
        })?;
        let recorded_at = IsoTimestamp::now();
        let expires_at = IsoTimestamp::from_datetime(
            recorded_at.into_inner()
                + chrono::Duration::from_std(self.config.remote_retry_budget).map_err(|error| {
                    AtmError::daemon_unavailable(
                        "failed to convert remote retry budget into a replay expiry",
                    )
                    .with_source(error)
                })?,
        );
        replay_store.enqueue(RemoteReplayStateRecord {
            team,
            agent,
            message_key,
            peer_addr: persisted_peer_addr(endpoint),
            request,
            recorded_at,
            expires_at,
            attempt_count: 0,
            last_attempt_at: None,
            last_error: None,
        })
    }

    fn persist_outcome_unknown_request(&self, request: &RequestEnvelope) -> Result<(), AtmError> {
        let Some((team, agent, message_key)) = replay_metadata_for_request(request)? else {
            tracing::warn!(
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
        let request_id = atm_core::protocol::next_request_id();
        let frame = self.codec.request_to_frame(request_id, request)?;
        let started = Instant::now();
        let deadline = started + self.config.remote_retry_budget;
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
        let mut stream =
            TcpStream::connect_timeout(&endpoint, PEER_CONNECT_DEADLINE).map_err(|source| {
                Box::new(AttemptFailure {
                    kind: classify_io_error(&source),
                    error: AtmError::daemon_unavailable(format!(
                        "failed to connect to remote daemon peer at {endpoint}"
                    ))
                    .with_source(source),
                })
            })?;
        stream
            .set_read_timeout(Some(PEER_IO_DEADLINE))
            .map_err(|source| {
                Box::new(AttemptFailure {
                    kind: AttemptFailureKind::Retryable,
                    error: AtmError::daemon_unavailable(
                        "failed to apply remote peer read deadline",
                    )
                    .with_source(source),
                })
            })?;
        stream
            .set_write_timeout(Some(PEER_IO_DEADLINE))
            .map_err(|source| {
                Box::new(AttemptFailure {
                    kind: AttemptFailureKind::Retryable,
                    error: AtmError::daemon_unavailable(
                        "failed to apply remote peer write deadline",
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
        let endpoint = self.endpoint.ok_or_else(|| {
            AtmError::daemon_unavailable("remote peer endpoint is not configured")
                .with_recovery("Set ATM_DAEMON_PEER_ADDR or configure the daemon peer transport before retrying remote delivery.")
        })?;
        match self.send_to_endpoint(endpoint, request.clone()) {
            Ok(response) => Ok(response),
            Err(error) if error.code == AtmErrorCode::RemoteDeliveryOutcomeUnknown => {
                self.persist_outcome_unknown_request(&request)
                    .map_err(|persist_error| {
                        AtmError::remote_delivery_outcome_unknown(
                            "remote peer delivery outcome is unknown and replay persistence failed",
                        )
                        .with_source(persist_error)
                    })?;
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
        Self::new(None)
    }
}

impl PeerTransportRuntime {
    pub(crate) fn new(replay_store: Option<Arc<dyn RemoteReplayStore>>) -> Self {
        Self {
            client: PeerClientTransport::new(replay_store),
        }
    }

    #[cfg(test)]
    #[allow(dead_code)]
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
) -> Result<Option<(TeamName, AgentName, MessageKey)>, AtmError> {
    match request {
        RequestEnvelope::Heartbeat(heartbeat) => Ok(Some((
            heartbeat.team.clone(),
            heartbeat.member.clone(),
            heartbeat_message_key(heartbeat)?,
        ))),
        _ => Ok(None),
    }
}

fn heartbeat_message_key(request: &TeamMemberHeartbeatRequest) -> Result<MessageKey, AtmError> {
    MessageKey::new(format!(
        "remote-heartbeat:{}:{}:{}:{}",
        request.team.as_str(),
        request.member.as_str(),
        request.pid,
        request.observed_at.into_inner().to_rfc3339(),
    ))
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

fn persisted_peer_addr(endpoint: SocketAddr) -> SocketAddr {
    endpoint
}

#[cfg(test)]
mod tests {
    use super::{
        AttemptFailureKind, PeerTransportConfig, PeerTransportRuntime, classify_io_error,
        jittered_backoff,
    };
    use crate::lifecycle_control::LifecycleControlSourceAdapter;
    use crate::test_support::LifecycleFlagResetGuard;
    use atm_core::boundary::{AtmProtocol, ClientTransport, MessageKey};
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
        let transport = PeerTransportRuntime::new_for_test(
            SocketAddr::from((Ipv4Addr::LOCALHOST, 9)),
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
            .recv()
            .expect("response delivered")
            .expect("response");
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
