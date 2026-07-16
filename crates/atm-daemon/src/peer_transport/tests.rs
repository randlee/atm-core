use super::{
    AttemptFailureKind, PEER_REQUEST_DEADLINE, PeerClientTransport, PeerTransportConfig,
    PeerTransportRuntime, classify_io_error, jittered_backoff,
    remote_peer_endpoint_not_configured_error, remote_replay_persistence_failed_error,
    remote_replay_store_not_configured_error,
};
use crate::lifecycle_control::LifecycleControlSourceAdapter;
use crate::runtime_status_cache::RuntimeStatusCache;
use crate::test_support::DoctorOnlyDispatcher;
use crate::test_support::LifecycleFlagResetGuard;
use crate::{DaemonSubsystem, SubsystemObservability};
use atm_core::boundary::{AtmProtocol, ClientTransport, MessageKey, RequestDispatcher};
use atm_core::doctor::DoctorQuery;
use atm_core::error::AtmErrorCode;
use atm_core::protocol::{
    HeartbeatActivity, ProtocolErrorEnvelope, RequestEnvelope, ResponseEnvelope,
    RuntimeMemberState, RuntimeReadinessState, TeamMemberHeartbeatRequest,
    TeamMemberHeartbeatResponse,
};
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use atm_runtime_test_support::open_sqlite_boundary;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

#[path = "tests_transport.rs"]
mod tests_transport;

fn test_team_name() -> TeamName {
    atm_core::test_support::TEST_TEAM.parse().expect("team")
}

fn test_sender_name() -> AgentName {
    atm_core::test_support::TEST_SENDER.parse().expect("member")
}

fn test_recipient_name() -> AgentName {
    atm_core::test_support::TEST_RECIPIENT
        .parse()
        .expect("member")
}

fn read_request_frame(
    stream: &mut TcpStream,
) -> (
    atm_core::protocol::RequestId,
    RequestEnvelope,
    atm_core::protocol::JsonAtmProtocolCodec,
) {
    let codec = atm_core::protocol::JsonAtmProtocolCodec;
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
    codec: &atm_core::protocol::JsonAtmProtocolCodec,
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

#[derive(Debug)]
struct SleepingDispatcher {
    sleep_for: Duration,
}

impl atm_core::boundary::sealed::Sealed for SleepingDispatcher {}

impl RequestDispatcher for SleepingDispatcher {
    fn dispatch(
        &self,
        _request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, atm_core::error::AtmError> {
        thread::sleep(self.sleep_for);
        Ok(ResponseEnvelope::Heartbeat(TeamMemberHeartbeatResponse {
            team: atm_core::test_support::TEST_TEAM.parse().expect("team"),
            member: atm_core::test_support::TEST_SENDER.parse().expect("member"),
            pid: 7,
            pid_changed: false,
            state: RuntimeMemberState::Idle,
            last_active_at: Some(IsoTimestamp::now()),
        }))
    }
}

#[derive(Debug, Default)]
struct CountingDispatcher {
    count: AtomicUsize,
}

impl atm_core::boundary::sealed::Sealed for CountingDispatcher {}

impl RequestDispatcher for CountingDispatcher {
    fn dispatch(
        &self,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, atm_core::error::AtmError> {
        self.count.fetch_add(1, Ordering::SeqCst);
        DoctorOnlyDispatcher.dispatch(request)
    }
}

#[test]
fn peer_listener_round_trips_one_doctor_request() {
    let _guard = install_shared_lifecycle_reset_guard();
    let listener_transport = PeerTransportRuntime::new_server_for_test(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
    );
    listener_transport
        .start(Arc::new(DoctorOnlyDispatcher))
        .expect("start peer listener");
    let endpoint = listener_transport
        .bound_addr_for_test()
        .expect("bound peer listener addr");

    let tempdir = TempDir::new().expect("tempdir");
    let client_transport = PeerTransportRuntime::new_for_test(
        endpoint,
        PeerTransportConfig::default(),
        tempdir.path().join("replay.db"),
    );
    let response = client_transport
        .client_transport()
        .send(RequestEnvelope::Doctor(atm_core::doctor::DoctorQuery {
            home_dir: tempdir.path().to_path_buf(),
            current_dir: tempdir.path().to_path_buf(),
            team_override: None,
            ..atm_core::doctor::DoctorQuery::default()
        }))
        .expect("peer listener doctor response");
    match response {
        ResponseEnvelope::Doctor(report) => {
            assert_eq!(
                report.summary.status,
                atm_core::doctor::DoctorStatus::Healthy
            );
        }
        other => panic!("unexpected response from peer listener: {other:?}"),
    }

    listener_transport
        .shutdown()
        .expect("shutdown peer listener");
}

#[test]
fn peer_listener_rejects_unauthorized_host_before_dispatch() {
    let _guard = install_shared_lifecycle_reset_guard();
    let assembly = open_sqlite_boundary(
        std::env::temp_dir().join(format!("atm-ag5-peer-auth-{}.db", std::process::id())),
    )
    .expect("runtime assembly");
    let listener_transport = PeerTransportRuntime::new_server_for_test_with_allowed_host_store(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
        RuntimeStatusCache::new(),
        assembly.allowed_host_store_arc(),
    );
    let dispatcher = Arc::new(CountingDispatcher::default());
    listener_transport
        .start(dispatcher.clone())
        .expect("start peer listener");
    let endpoint = listener_transport
        .bound_addr_for_test()
        .expect("bound peer listener addr");

    let tempdir = TempDir::new().expect("tempdir");
    let client_transport = PeerTransportRuntime::new_for_test(
        endpoint,
        PeerTransportConfig::default(),
        tempdir.path().join("replay.db"),
    );
    let error = client_transport
        .client_transport()
        .send(RequestEnvelope::Doctor(DoctorQuery {
            home_dir: tempdir.path().to_path_buf(),
            current_dir: tempdir.path().to_path_buf(),
            team_override: None,
            ..DoctorQuery::default()
        }))
        .expect_err("unauthorized host should be rejected");
    assert_eq!(error.code, AtmErrorCode::MessageValidationFailed);
    assert_eq!(dispatcher.count.load(Ordering::SeqCst), 0);

    listener_transport
        .shutdown()
        .expect("shutdown peer listener");
}

#[test]
fn peer_listener_accepts_allowed_socket_host_and_dispatches() {
    let _guard = install_shared_lifecycle_reset_guard();
    let assembly = open_sqlite_boundary(std::env::temp_dir().join(format!(
        "atm-ag5-peer-auth-allowed-{}.db",
        std::process::id()
    )))
    .expect("runtime assembly");
    let allowed_host_store = assembly.allowed_host_store_arc();
    allowed_host_store
        .allow_host(
            atm_storage::AllowHostCommand::new(
                "127.0.0.1",
                "arch-ctm@atm-dev",
                Some("loopback".to_string()),
            )
            .expect("allow host command"),
        )
        .expect("allow host");
    let listener_transport = PeerTransportRuntime::new_server_for_test_with_allowed_host_store(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
        RuntimeStatusCache::new(),
        allowed_host_store,
    );
    let dispatcher = Arc::new(CountingDispatcher::default());
    listener_transport
        .start(dispatcher.clone())
        .expect("start peer listener");
    let endpoint = listener_transport
        .bound_addr_for_test()
        .expect("bound peer listener addr");

    let tempdir = TempDir::new().expect("tempdir");
    let client_transport = PeerTransportRuntime::new_for_test(
        endpoint,
        PeerTransportConfig::default(),
        tempdir.path().join("replay.db"),
    );
    let response = client_transport
        .client_transport()
        .send(RequestEnvelope::Doctor(DoctorQuery {
            home_dir: tempdir.path().to_path_buf(),
            current_dir: tempdir.path().to_path_buf(),
            team_override: None,
            ..DoctorQuery::default()
        }))
        .expect("authorized host should round-trip");
    assert!(matches!(response, ResponseEnvelope::Doctor(_)));
    assert_eq!(dispatcher.count.load(Ordering::SeqCst), 1);

    listener_transport
        .shutdown()
        .expect("shutdown peer listener");
}

#[test]
fn peer_listener_rejects_disabled_host_before_dispatch() {
    let _guard = install_shared_lifecycle_reset_guard();
    let tempdir = TempDir::new().expect("tempdir");
    let assembly = open_sqlite_boundary(tempdir.path().join("auth.db")).expect("runtime assembly");
    let allowed_host_store = assembly.allowed_host_store_arc();
    let allowed_host = "127.0.0.1"
        .parse::<atm_storage::AllowedHostName>()
        .expect("allowed host");
    allowed_host_store
        .allow_host(
            atm_storage::AllowHostCommand::new(
                "127.0.0.1",
                "arch-ctm@atm-dev",
                Some("loopback".to_string()),
            )
            .expect("allow host command"),
        )
        .expect("allow host");
    allowed_host_store
        .deny_host(&allowed_host)
        .expect("deny host");
    let listener_transport = PeerTransportRuntime::new_server_for_test_with_allowed_host_store(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
        RuntimeStatusCache::new(),
        allowed_host_store,
    );
    let dispatcher = Arc::new(CountingDispatcher::default());
    listener_transport
        .start(dispatcher.clone())
        .expect("start peer listener");
    let endpoint = listener_transport
        .bound_addr_for_test()
        .expect("bound peer listener addr");

    let client_transport = PeerTransportRuntime::new_for_test(
        endpoint,
        PeerTransportConfig::default(),
        tempdir.path().join("replay.db"),
    );
    let error = client_transport
        .client_transport()
        .send(RequestEnvelope::Doctor(DoctorQuery {
            home_dir: tempdir.path().to_path_buf(),
            current_dir: tempdir.path().to_path_buf(),
            team_override: None,
            ..DoctorQuery::default()
        }))
        .expect_err("disabled host should be rejected");
    assert_eq!(error.code, AtmErrorCode::MessageValidationFailed);
    assert!(
        error
            .message
            .contains("presented host `127.0.0.1` but that host is disabled"),
        "unexpected error message: {}",
        error.message
    );
    assert_eq!(dispatcher.count.load(Ordering::SeqCst), 0);

    listener_transport
        .shutdown()
        .expect("shutdown peer listener");
}

#[test]
fn peer_listener_reload_bind_failure_persists_degraded_status_until_successful_rebind() {
    let _guard = install_shared_lifecycle_reset_guard();
    let status_cache = RuntimeStatusCache::new();
    let listener_transport = PeerTransportRuntime::new_server_for_test_with_status_cache(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
        status_cache.clone(),
    );
    listener_transport
        .start(Arc::new(DoctorOnlyDispatcher))
        .expect("start peer listener");

    let blocker = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("blocker");
    let blocked_addr = blocker.local_addr().expect("blocked addr");
    let outcomes = listener_transport
        .reload_listeners(vec![blocked_addr], Arc::new(DoctorOnlyDispatcher))
        .expect("reload should preserve degraded status when one address is blocked");
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].listen_addr, blocked_addr);
    assert!(outcomes[0].bound_addr.is_none());
    assert!(outcomes[0].error_message.is_some());

    let degraded = status_cache.snapshot();
    assert_eq!(degraded.readiness, RuntimeReadinessState::Degraded);
    assert!(degraded.degraded_peer_listener);
    assert!(
        degraded
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("peer listener"))
    );

    drop(blocker);
    listener_transport
        .reload_listeners(vec![blocked_addr], Arc::new(DoctorOnlyDispatcher))
        .expect("reload after releasing blocker");

    let recovered = status_cache.snapshot();
    assert_eq!(recovered.readiness, RuntimeReadinessState::Ready);
    assert!(!recovered.degraded_peer_listener);
    listener_transport.shutdown().expect("shutdown");
}

#[test]
fn peer_listener_reload_keeps_healthy_rows_running_when_one_row_fails_to_bind() {
    let _guard = install_shared_lifecycle_reset_guard();
    let status_cache = RuntimeStatusCache::new();
    let listener_transport = PeerTransportRuntime::new_server_for_test_with_status_cache(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
        status_cache.clone(),
    );

    let good_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("good listener");
    let good_addr = good_listener.local_addr().expect("good addr");
    drop(good_listener);

    let blocker = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("blocker");
    let blocked_addr = blocker.local_addr().expect("blocked addr");

    let outcomes = listener_transport
        .reload_listeners(
            vec![good_addr, blocked_addr],
            Arc::new(DoctorOnlyDispatcher),
        )
        .expect("reload listeners");
    assert_eq!(outcomes.len(), 2);
    assert!(outcomes.iter().any(|outcome| {
        outcome.listen_addr == good_addr
            && outcome.bound_addr.is_some()
            && outcome.error_message.is_none()
    }));
    assert!(outcomes.iter().any(|outcome| {
        outcome.listen_addr == blocked_addr
            && outcome.bound_addr.is_none()
            && outcome.error_message.is_some()
    }));

    let degraded = status_cache.snapshot();
    assert_eq!(degraded.readiness, RuntimeReadinessState::Degraded);
    assert!(degraded.degraded_peer_listener);

    listener_transport.shutdown().expect("shutdown");
}

#[test]
fn peer_listener_dispatch_observes_one_shared_connection_deadline() {
    let _guard = install_shared_lifecycle_reset_guard();
    let listener_transport = PeerTransportRuntime::new_server_for_test(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
    );
    listener_transport
        .start(Arc::new(SleepingDispatcher {
            sleep_for: PEER_REQUEST_DEADLINE + Duration::from_millis(250),
        }))
        .expect("start listener");
    let endpoint = listener_transport.bound_addr_for_test().expect("endpoint");

    let tempdir = TempDir::new().expect("tempdir");
    let client_transport = PeerTransportRuntime::new_for_test(
        endpoint,
        PeerTransportConfig {
            remote_retry_budget: Duration::from_secs(4),
            peer_listen_addr: None,
        },
        tempdir.path().join("replay.db"),
    );

    let started = Instant::now();
    let error = client_transport
        .client_transport()
        .send(RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: atm_core::test_support::TEST_TEAM.parse().expect("team"),
            member: atm_core::test_support::TEST_SENDER.parse().expect("member"),
            pid: 9,
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::Idle,
        }))
        .expect_err("deadline should expire before a response can be written");
    let elapsed = started.elapsed();

    assert_eq!(error.code, AtmErrorCode::RemoteDeliveryOutcomeUnknown);
    assert!(elapsed < PEER_REQUEST_DEADLINE + Duration::from_secs(1));
    listener_transport.shutdown().expect("shutdown");
}

#[test]
fn peer_listener_delivers_response_computed_within_shared_connection_deadline() {
    let _guard = install_shared_lifecycle_reset_guard();
    let listener_transport = PeerTransportRuntime::new_server_for_test(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
    );
    listener_transport
        .start(Arc::new(SleepingDispatcher {
            sleep_for: PEER_REQUEST_DEADLINE - Duration::from_millis(500),
        }))
        .expect("start listener");
    let endpoint = listener_transport.bound_addr_for_test().expect("endpoint");

    let tempdir = TempDir::new().expect("tempdir");
    let client_transport = PeerTransportRuntime::new_for_test(
        endpoint,
        PeerTransportConfig {
            remote_retry_budget: Duration::from_secs(4),
            peer_listen_addr: None,
        },
        tempdir.path().join("replay.db"),
    );

    let response = client_transport
        .client_transport()
        .send(RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: atm_core::test_support::TEST_TEAM.parse().expect("team"),
            member: atm_core::test_support::TEST_SENDER.parse().expect("member"),
            pid: 9,
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::Idle,
        }))
        .expect("response should be written before the shared deadline expires");

    match response {
        ResponseEnvelope::Heartbeat(response) => {
            assert_eq!(response.team, test_team_name());
            assert_eq!(response.member, test_sender_name());
            assert_eq!(response.pid, 7);
            assert!(!response.pid_changed);
            assert_eq!(response.state, RuntimeMemberState::Idle);
            assert!(response.last_active_at.is_some());
        }
        other => panic!("unexpected response: {other:?}"),
    }
    listener_transport.shutdown().expect("shutdown");
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
    let team = test_team_name();
    let agent = test_sender_name();
    let client = PeerClientTransport {
        endpoint: Some(SocketAddr::from((Ipv4Addr::LOCALHOST, 7001))),
        config: PeerTransportConfig::default(),
        replay_store: None,
        codec: atm_core::protocol::JsonAtmProtocolCodec,
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
    assert!(error.primary_recovery().is_some());
}

#[test]
fn persist_replay_request_missing_endpoint_matches_send_surface_contract() {
    let tempdir = TempDir::new().expect("tempdir");
    let team = test_team_name();
    let agent = test_sender_name();
    let replay_store =
        atm_runtime::sqlite_remote_replay_store_for_test(tempdir.path().join("mail.db"))
            .expect("replay store");
    let client = PeerClientTransport {
        endpoint: None,
        config: PeerTransportConfig::default(),
        replay_store: Some(replay_store),
        codec: atm_core::protocol::JsonAtmProtocolCodec,
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
    let team = test_team_name();
    let agent = test_sender_name();
    let replay_store =
        atm_runtime::sqlite_remote_replay_store_for_test(tempdir.path().join("mail.db"))
            .expect("replay store");
    let client = PeerClientTransport {
        endpoint: Some(SocketAddr::from((Ipv4Addr::LOCALHOST, 7002))),
        config: PeerTransportConfig {
            remote_retry_budget: Duration::MAX,
            peer_listen_addr: None,
        },
        replay_store: Some(replay_store),
        codec: atm_core::protocol::JsonAtmProtocolCodec,
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
    assert!(error.primary_recovery().is_some());
}

#[test]
fn protocol_envelope_preserves_replay_persistence_failure_contract() {
    let error = remote_replay_persistence_failed_error(remote_replay_store_not_configured_error());
    let envelope = ProtocolErrorEnvelope::from_error(&error);
    let round_trip = envelope.into_atm_error();
    assert_eq!(round_trip.code, AtmErrorCode::RemoteDeliveryOutcomeUnknown);
    assert_eq!(round_trip.message, error.message);
    assert_eq!(round_trip.recovery, error.recovery);
}
