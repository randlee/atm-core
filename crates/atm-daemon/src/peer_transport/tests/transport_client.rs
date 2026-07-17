use super::*;

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
        .reload_listener(Some(blocked_addr), Arc::new(DoctorOnlyDispatcher))
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
    let client = PeerClientTransport::new_with_observability(
        None,
        None,
        PeerTransportConfig::default(),
        SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
    );
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
        peer_security_store: None,
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
        peer_security_store: None,
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

    let bind_error =
        TcpListener::bind((Ipv4Addr::new(198, 51, 100, 10), 0)).expect_err("explicit bind failure");
    assert_eq!(bind_error.kind(), io::ErrorKind::AddrNotAvailable);
}

#[test]
#[serial_test::serial(env)]
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
    let team = test_team_name();
    let member = test_recipient_name();
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
#[serial_test::serial(env)]
fn peer_transport_aborts_before_connect_when_terminate_is_requested() {
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
            peer_listen_addr: None,
        },
        tempdir.path().join("replay.db"),
    );

    let error = transport
        .client_transport()
        .send(RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: atm_core::test_support::TEST_TEAM.parse().expect("team"),
            member: atm_core::test_support::TEST_SENDER.parse().expect("member"),
            pid: std::process::id(),
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::ActiveToolUse,
        }))
        .expect_err("terminate should short-circuit before connect");
    assert_eq!(error.code, AtmErrorCode::DaemonUnavailable);
}

#[test]
#[serial_test::serial(env)]
fn peer_transport_uses_port_zero_listener_handoff_without_rebind_race() {
    let _reset = install_shared_lifecycle_reset_guard();
    let tempdir = TempDir::new().expect("tempdir");
    let (endpoint_tx, endpoint_rx) = mpsc::channel();
    let team = test_team_name();
    let member = test_recipient_name();
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
            peer_listen_addr: None,
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
#[serial_test::serial(env)]
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
    let team = test_team_name();
    let member = test_recipient_name();
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
#[serial_test::serial(env)]
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
    let team = test_team_name();
    let member = test_recipient_name();
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
            recovery: Vec::new(),
        });
        write_response_frame(&mut stream, &codec, request_id, response);
    });

    let error = transport
        .client_transport()
        .send(request)
        .expect_err("remote reject");
    assert_eq!(error.code, AtmErrorCode::DaemonUnavailable);
}

#[test]
#[serial_test::serial(env)]
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
    let team = test_team_name();
    let member = test_recipient_name();
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
#[serial_test::serial(env)]
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
    let team = test_team_name();
    let member = test_recipient_name();
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
#[serial_test::serial(env)]
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
        ..DoctorQuery::default()
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
            .primary_recovery()
            .expect("recovery guidance")
            .contains("let the daemon resume the pending handoff")
    );
    let pending = transport
        .load_pending_replay_records()
        .expect("load pending");
    assert!(pending.is_empty());
}

#[test]
#[serial_test::serial(env)]
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
    let team = test_team_name();
    let member = test_recipient_name();
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
    let team = test_team_name();
    let member = test_recipient_name();
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
