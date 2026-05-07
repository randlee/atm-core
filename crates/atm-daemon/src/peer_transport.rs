use std::io::{self, Write};
#[cfg(test)]
use std::net::IpAddr;
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use atm_core::AtmConfig;
use atm_core::boundary::{self, AtmProtocol, ClientTransport, ConfigLoadRequest, MessageKey};
use atm_core::error::AtmError;
use atm_core::protocol::{FramePayload, MAX_DAEMON_FRAME_BYTES, RequestEnvelope, ResponseEnvelope};
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use atm_rusqlite::{RemoteReplayStateRecord, assemble_boundary};

const PEER_CONNECT_DEADLINE: Duration = Duration::from_secs(5);
const PEER_IO_DEADLINE: Duration = Duration::from_secs(5);
const DEFAULT_REMOTE_RETRY_BUDGET: Duration = Duration::from_secs(30);
const INITIAL_RETRY_BACKOFF: Duration = Duration::from_millis(250);
const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(5);

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListenerBindingMode {
    Wildcard,
    Explicit(IpAddr),
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListenerInterfaceBehavior {
    SurvivesInterfaceChurn,
    RequiresReloadAndDegrades,
}

#[cfg(test)]
impl ListenerBindingMode {
    const fn interface_behavior(self) -> ListenerInterfaceBehavior {
        match self {
            Self::Wildcard => ListenerInterfaceBehavior::SurvivesInterfaceChurn,
            Self::Explicit(_) => ListenerInterfaceBehavior::RequiresReloadAndDegrades,
        }
    }
}

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

#[derive(Debug, Clone, Default)]
struct JsonAtmProtocolCodec;

impl boundary::sealed::Sealed for JsonAtmProtocolCodec {}

impl AtmProtocol for JsonAtmProtocolCodec {
    fn request_to_frame(&self, request: RequestEnvelope) -> Result<FramePayload, AtmError> {
        Ok(FramePayload {
            bytes: serde_json::to_vec(&request).map_err(AtmError::from)?,
        })
    }

    fn request_from_frame(&self, frame: FramePayload) -> Result<RequestEnvelope, AtmError> {
        serde_json::from_slice(&frame.bytes).map_err(AtmError::from)
    }

    fn response_to_frame(&self, response: ResponseEnvelope) -> Result<FramePayload, AtmError> {
        Ok(FramePayload {
            bytes: serde_json::to_vec(&response).map_err(AtmError::from)?,
        })
    }

    fn response_from_frame(&self, frame: FramePayload) -> Result<ResponseEnvelope, AtmError> {
        serde_json::from_slice(&frame.bytes).map_err(AtmError::from)
    }
}

#[derive(Debug, Clone)]
struct SqliteRemoteReplayStore {
    db_path: PathBuf,
}

impl SqliteRemoteReplayStore {
    fn new(db_path: PathBuf) -> Self {
        Self { db_path }
    }

    fn enqueue(&self, record: RemoteReplayStateRecord) -> Result<(), AtmError> {
        assemble_boundary(&self.db_path)?.record_remote_replay_state(record)
    }

    fn load_all(&self) -> Result<Vec<RemoteReplayStateRecord>, AtmError> {
        assemble_boundary(&self.db_path)?.load_remote_replay_states()
    }

    fn delete(
        &self,
        team: &TeamName,
        agent: &AgentName,
        message_key: &MessageKey,
    ) -> Result<(), AtmError> {
        assemble_boundary(&self.db_path)?.delete_remote_replay_state(team, agent, message_key)
    }

    fn purge_expired(&self, now: IsoTimestamp) -> Result<usize, AtmError> {
        assemble_boundary(&self.db_path)?.purge_expired_remote_replay_states(now)
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
    replay_store: Option<SqliteRemoteReplayStore>,
    codec: JsonAtmProtocolCodec,
}

impl PeerClientTransport {
    fn new() -> Self {
        let endpoint = std::env::var("ATM_DAEMON_PEER_ADDR")
            .ok()
            .and_then(|value| value.parse::<SocketAddr>().ok());
        let config = std::env::current_dir()
            .ok()
            .and_then(|current_dir| {
                atm_core::boundary_support::load_workspace_config(ConfigLoadRequest { current_dir })
                    .ok()
                    .and_then(|response| response.config)
            })
            .map(|config| PeerTransportConfig::from_config(Some(&config)))
            .unwrap_or_default();
        let replay_store = atm_core::home::host_mail_db_path()
            .ok()
            .map(SqliteRemoteReplayStore::new);
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
        Self {
            endpoint: Some(endpoint),
            config,
            replay_store: Some(SqliteRemoteReplayStore::new(replay_db_path)),
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
            if record.expires_at.into_inner() <= now.into_inner() {
                replay_store.delete(&record.team, &record.agent, &record.message_key)?;
                continue;
            }
            let peer_addr = match record.peer_addr.parse::<SocketAddr>() {
                Ok(peer_addr) => peer_addr,
                Err(error) => {
                    tracing::warn!(
                        message_key = %record.message_key,
                        peer_addr = %record.peer_addr,
                        %error,
                        "dropping invalid daemon remote replay entry"
                    );
                    replay_store.delete(&record.team, &record.agent, &record.message_key)?;
                    continue;
                }
            };

            match self.send_to_endpoint(peer_addr, record.request.clone()) {
                Ok(_) => {
                    replay_store.delete(&record.team, &record.agent, &record.message_key)?;
                    delivered += 1;
                }
                Err(error) => {
                    record.attempt_count = record.attempt_count.saturating_add(1);
                    record.last_attempt_at = Some(IsoTimestamp::now());
                    record.last_error = Some(error.code.to_string());
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

    #[cfg(test)]
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
            peer_addr: endpoint.to_string(),
            request,
            recorded_at,
            expires_at,
            attempt_count: 0,
            last_attempt_at: None,
            last_error: None,
        })
    }

    fn send_to_endpoint(
        &self,
        endpoint: SocketAddr,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, AtmError> {
        let frame = self.codec.request_to_frame(request)?;
        let started = Instant::now();
        let deadline = started + self.config.remote_retry_budget;
        let mut backoff = INITIAL_RETRY_BACKOFF;

        loop {
            match self.send_once(endpoint, &frame.bytes) {
                Ok(response) => return Ok(response),
                Err(failure) if failure.kind == AttemptFailureKind::Retryable => {
                    let now = Instant::now();
                    if now >= deadline {
                        return Err(failure.error);
                    }
                    let remaining = deadline.saturating_duration_since(now);
                    thread::sleep(backoff.min(remaining));
                    backoff = backoff.saturating_mul(2).min(MAX_RETRY_BACKOFF);
                }
                Err(failure) => return Err(failure.error),
            }
        }
    }

    fn send_once(
        &self,
        endpoint: SocketAddr,
        request_bytes: &[u8],
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
        stream.write_all(request_bytes).map_err(|source| {
            Box::new(AttemptFailure {
                kind: classify_io_error(&source),
                error: AtmError::daemon_unavailable("failed to write remote peer request frame")
                    .with_source(source),
            })
        })?;
        stream.shutdown(Shutdown::Write).map_err(|source| Box::new(AttemptFailure {
            kind: AttemptFailureKind::OutcomeUnknown,
            error: AtmError::remote_delivery_outcome_unknown(
                "remote peer connection dropped after the request was sent and acceptance is unknown",
            )
            .with_source(source),
        }))?;

        let response_bytes = read_peer_response(&mut stream).map_err(|error| {
            Box::new(AttemptFailure {
                kind: AttemptFailureKind::OutcomeUnknown,
                error,
            })
        })?;
        let response = self
            .codec
            .response_from_frame(FramePayload {
                bytes: response_bytes,
            })
            .map_err(|error| {
                Box::new(AttemptFailure {
                    kind: AttemptFailureKind::NonRetryable,
                    error: AtmError::daemon_unavailable(
                        "failed to decode remote peer response frame",
                    )
                    .with_source(error),
                })
            })?;
        match response {
            ResponseEnvelope::Error(error) => Err(Box::new(AttemptFailure {
                kind: AttemptFailureKind::NonRetryable,
                error: error.into_atm_error(),
            })),
            response => Ok(response),
        }
    }
}

impl boundary::sealed::Sealed for PeerClientTransport {}

impl ClientTransport for PeerClientTransport {
    fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        let endpoint = self.endpoint.ok_or_else(|| {
            AtmError::daemon_unavailable("remote peer endpoint is not configured")
                .with_recovery("Set ATM_DAEMON_PEER_ADDR or configure the daemon peer transport before retrying remote delivery.")
        })?;
        self.send_to_endpoint(endpoint, request)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PeerTransportRuntime {
    client: PeerClientTransport,
}

impl Default for PeerTransportRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl PeerTransportRuntime {
    pub(crate) fn new() -> Self {
        Self {
            client: PeerClientTransport::new(),
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
}

fn classify_io_error(error: &io::Error) -> AttemptFailureKind {
    match error.kind() {
        io::ErrorKind::TimedOut
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

fn read_peer_response(stream: &mut TcpStream) -> Result<Vec<u8>, AtmError> {
    let bytes = atm_core::protocol::read_bounded_stream(
        stream,
        "failed to read remote peer response frame",
        "remote peer response frame exceeded the maximum supported size",
    )?;
    if bytes.is_empty() {
        return Err(AtmError::remote_delivery_outcome_unknown(
            "remote peer connection closed after the request was sent and before one response frame was received",
        ));
    }
    if bytes.len() > MAX_DAEMON_FRAME_BYTES {
        return Err(AtmError::daemon_unavailable(
            "remote peer response frame exceeded the maximum supported size",
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::{
        ListenerBindingMode, ListenerInterfaceBehavior, PeerTransportConfig, PeerTransportRuntime,
    };
    use atm_core::boundary::MessageKey;
    use atm_core::error::AtmErrorCode;
    use atm_core::protocol::{
        HeartbeatActivity, ProtocolErrorEnvelope, RequestEnvelope, ResponseEnvelope,
        RuntimeMemberState, TeamMemberHeartbeatRequest, TeamMemberHeartbeatResponse,
    };
    use atm_core::types::{AgentName, IsoTimestamp, TeamName};
    use atm_rusqlite::assemble_boundary;
    use std::io::{Read, Write};
    use std::net::{IpAddr, Ipv4Addr, TcpListener};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn wildcard_bindings_survive_interface_churn() {
        assert_eq!(
            ListenerBindingMode::Wildcard.interface_behavior(),
            ListenerInterfaceBehavior::SurvivesInterfaceChurn
        );
        assert_eq!(
            ListenerBindingMode::Explicit(IpAddr::V4(Ipv4Addr::LOCALHOST)).interface_behavior(),
            ListenerInterfaceBehavior::RequiresReloadAndDegrades
        );
    }

    #[test]
    fn peer_transport_round_trips_one_heartbeat_request() {
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
            let mut request_bytes = Vec::new();
            stream.read_to_end(&mut request_bytes).expect("request");
            let _: RequestEnvelope =
                serde_json::from_slice(&request_bytes).expect("decode request");
            let response_bytes = serde_json::to_vec(&expected).expect("encode response");
            stream.write_all(&response_bytes).expect("write response");
            stream.flush().expect("flush response");
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
    fn peer_transport_retries_transient_connect_failures_within_budget() {
        let tempdir = TempDir::new().expect("tempdir");
        let probe = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("probe");
        let endpoint = probe.local_addr().expect("addr");
        drop(probe);

        let transport = PeerTransportRuntime::new_for_test(
            endpoint,
            PeerTransportConfig {
                remote_retry_budget: Duration::from_millis(600),
            },
            tempdir.path().join("mail.db"),
        );
        let team: TeamName = "test-team".parse().expect("team");
        let member: AgentName = "test-member".parse().expect("member");
        let request = RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: team.clone(),
            member: member.clone(),
            pid: 7,
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::ActiveToolUse,
        });
        let (ready_tx, ready_rx) = mpsc::channel();

        thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            let listener = TcpListener::bind(endpoint).expect("listener");
            ready_tx.send(()).expect("ready");
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request_bytes = Vec::new();
            stream.read_to_end(&mut request_bytes).expect("request");
            let _: RequestEnvelope =
                serde_json::from_slice(&request_bytes).expect("decode request");
            let response = ResponseEnvelope::Heartbeat(TeamMemberHeartbeatResponse {
                team,
                member,
                pid: 7,
                pid_changed: false,
                state: RuntimeMemberState::Active,
                last_active_at: Some(IsoTimestamp::now()),
            });
            let bytes = serde_json::to_vec(&response).expect("response");
            stream.write_all(&bytes).expect("write response");
            stream.flush().expect("flush response");
        });

        ready_rx.recv().expect("listener ready");
        let response = transport
            .client_transport()
            .send(request)
            .expect("response");
        assert!(matches!(response, ResponseEnvelope::Heartbeat(_)));
    }

    #[test]
    fn peer_transport_reports_outcome_unknown_after_send_without_response() {
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
            let mut request_bytes = Vec::new();
            stream.read_to_end(&mut request_bytes).expect("request");
        });

        let error = transport
            .client_transport()
            .send(request)
            .expect_err("outcome unknown");
        assert_eq!(error.code, AtmErrorCode::RemoteDeliveryOutcomeUnknown);
    }

    #[test]
    fn peer_transport_treats_remote_error_envelope_as_non_retryable() {
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
            let mut request_bytes = Vec::new();
            stream.read_to_end(&mut request_bytes).expect("request");
            let response = ResponseEnvelope::Error(ProtocolErrorEnvelope {
                code: AtmErrorCode::DaemonUnavailable,
                message: "remote rejected request".to_string(),
                recovery: None,
            });
            let bytes = serde_json::to_vec(&response).expect("response");
            stream.write_all(&bytes).expect("write response");
            stream.flush().expect("flush response");
        });

        let error = transport
            .client_transport()
            .send(request)
            .expect_err("remote reject");
        assert_eq!(error.code, AtmErrorCode::DaemonUnavailable);
        assert!(error.message.contains("remote rejected request"));
    }

    #[test]
    fn replay_resume_replays_and_deletes_delivered_rows() {
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
            let mut request_bytes = Vec::new();
            stream.read_to_end(&mut request_bytes).expect("request");
            let response = ResponseEnvelope::Heartbeat(TeamMemberHeartbeatResponse {
                team,
                member,
                pid: 21,
                pid_changed: false,
                state: RuntimeMemberState::Idle,
                last_active_at: Some(IsoTimestamp::now()),
            });
            let bytes = serde_json::to_vec(&response).expect("response");
            stream.write_all(&bytes).expect("write response");
            stream.flush().expect("flush response");
        });

        let summary = transport.resume_pending_replay().expect("resume");
        assert_eq!(summary.delivered, 1);
        assert_eq!(summary.retained, 0);
        let pending = assemble_boundary(&db_path)
            .expect("assembly")
            .load_remote_replay_states()
            .expect("load");
        assert!(pending.is_empty());
    }
}
