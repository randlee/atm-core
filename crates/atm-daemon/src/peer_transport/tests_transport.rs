use super::*;

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
    let request = RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
        team: test_team_name(),
        member: test_recipient_name(),
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
    let request = RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
        team: test_team_name(),
        member: test_recipient_name(),
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
    assert!(
        transport
            .load_pending_replay_records()
            .expect("load pending")
            .is_empty()
    );
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
    assert!(
        transport
            .load_pending_replay_records()
            .expect("load pending")
            .is_empty()
    );
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
    assert!(
        second
            .load_pending_replay_records()
            .expect("load pending")
            .is_empty()
    );

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
