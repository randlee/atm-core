use super::*;
use atm_core::ack::AckRequest;

#[test]
#[serial_test::serial(env)]
fn heartbeat_and_local_dispatch_converge_on_cache() {
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    let workspace_dir = tempdir.path().join("workspace");
    let db_path = tempdir.path().join("mail.db");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir");
    write_team_config(&atm_home, &[]);
    write_workspace_config(&workspace_dir);
    add_member_via_retained_admin(
        &db_path,
        &atm_home,
        TEST_TEAM,
        ROLE_TEAM_LEAD,
        &workspace_dir,
    );
    add_member_via_retained_admin(&db_path, &atm_home, TEST_TEAM, "qa-a", &workspace_dir);

    let _env = EnvGuard::set_many([
        ("ATM_HOME", Some(atm_home.to_str().expect("utf8 atm home"))),
        (
            "ATM_CONFIG_HOME",
            Some(tempdir.path().to_str().expect("utf8 config home")),
        ),
        (
            SQLITE_RUNTIME_PATH_ENV,
            Some(db_path.to_str().expect("utf8 sqlite db path")),
        ),
        ("HOME", Some(tempdir.path().to_str().expect("utf8 home"))),
        ("USERPROFILE", None),
    ]);
    let socket_path = tempdir.path().join("daemon.sock");
    let server_transport = LocalIpcServerTransportAdapter::new();
    let runtime = server_transport
        .prepare_runtime_at_socket_path_for_home(socket_path.clone(), &atm_home)
        .expect("prepare runtime");
    let mut runtime = runtime;
    let endpoint_guard = runtime.take_endpoint_guard().expect("take endpoint guard");
    let (lifecycle, _reset) = {
        let lifecycle = LifecycleControlSourceAdapter::install().expect("install lifecycle");
        let reset = LifecycleFlagResetGuard::install(lifecycle.clone());
        (lifecycle, reset)
    };
    let status_cache = RuntimeStatusCache::new();
    let dispatcher: Arc<dyn ApiRouter + Send + Sync> =
        Arc::new(DaemonRequestDispatcher::new_for_test(
            atm_home.clone(),
            status_cache.clone(),
            db_path.clone(),
        ));
    let (serve_result_tx, serve_result_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);

    let join = std::thread::spawn(move || {
        let result = runtime.serve_with_runtime_hooks(
            dispatcher,
            RuntimeServeHooks {
                endpoint_guard,
                graceful_drain_deadline: Duration::from_millis(500),
                force_cancel_deadline: Duration::from_secs(2),
                begin_shutdown: || Ok(()),
                reload_runtime_view: || Ok(()),
                publish_ready: move || {
                    ready_tx.send(()).map_err(|_| {
                        AtmError::daemon_unavailable(
                            "send round-trip test failed to observe the daemon ready signal",
                        )
                    })
                },
            },
        );
        serve_result_tx.send(result).expect("send serve result");
    });

    let mut stream = connect_daemon_local_ipc_until_ready(&socket_path, ready_rx);
    configure_test_local_ipc_timeouts(&stream);
    let request = RequestEnvelope::Write(Box::new(
        SendRequest::new(
            atm_home.clone(),
            workspace_dir.clone(),
            ROLE_TEAM_LEAD.parse().expect("caller"),
            "qa-a@test-team",
            TEST_TEAM.parse().expect("team"),
            SendMessageSource::Inline("hello local ipc".to_string()),
            None,
            true,
            None,
            false,
        )
        .expect("send request")
        .with_activity_observation(Some(atm_core::caller_context::ActivityObservation {
            team: TEST_TEAM.parse().expect("team"),
            member: ROLE_TEAM_LEAD.parse().expect("member"),
            session_id: Some(
                atm_core::types::SessionId::new("transport-session").expect("session"),
            ),
            pid: Some(42),
        })),
    ));
    write_test_local_ipc_request(&mut stream, &request).expect("write send request");
    let response =
        atm_core::api::read_http_response(&mut stream, &request).expect("read send response");
    let sent_message_id = match response {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => {
            assert_eq!(outcome.outcome.as_str(), "sent");
            outcome.message_id
        }
        other => panic!("unexpected response: {other:?}"),
    };

    let read_request = RequestEnvelope::Receive(
        ReadQuery::new(
            atm_home.clone(),
            workspace_dir.clone(),
            "qa-a".parse().expect("reader"),
            None,
            TEST_TEAM.parse().expect("team"),
            ReadSelection::All,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("read request"),
    );
    #[cfg(not(windows))]
    let mut read_stream = {
        let ipc_name = atm_core::protocol::daemon_local_ipc_name_from_path(&socket_path)
            .expect("ipc name")
            .into_owned();
        connect_local_ipc_with_timeout(ipc_name, Duration::from_secs(5))
            .expect("connect local IPC for read")
    };
    #[cfg(windows)]
    let mut read_stream = match atm_daemon_client::try_connect(
        &atm_daemon_client::resolve_daemon_local_ipc_endpoint().expect("endpoint"),
    )
    .expect("connect local HTTP for read")
    {
        atm_daemon_client::LocalDaemonConnection::TcpLoopback(stream) => stream,
    };
    configure_test_local_ipc_timeouts(&read_stream);
    write_test_local_ipc_request(&mut read_stream, &read_request).expect("write read request");
    let read_response = atm_core::api::read_http_response(&mut read_stream, &read_request)
        .expect("read read response");
    assert!(matches!(read_response, ResponseEnvelope::Receive(_)));
    assert_eq!(
        status_cache.cached_session_id(test_team(), &ROLE_TEAM_LEAD.parse().expect("member")),
        Some(atm_core::types::SessionId::new("transport-session").expect("session")),
    );

    #[cfg(windows)]
    let record_path = atm_daemon_client::resolve_daemon_local_ipc_endpoint()
        .expect("resolve loopback endpoint record")
        .as_ref()
        .to_path_buf();
    #[cfg(not(windows))]
    let record_path = socket_path
        .parent()
        .expect("socket path parent")
        .join(atm_core::local_http::LOCAL_HTTP_RECORD_FILENAME);
    let record: atm_core::local_http::LocalHttpEndpointRecord =
        serde_json::from_slice(&std::fs::read(record_path).expect("read loopback endpoint record"))
            .expect("parse loopback endpoint record");
    let capability = record
        .capability()
        .expect("local capability")
        .to_base64url();

    let heartbeat_session = atm_core::types::SessionId::new("wire-heartbeat").expect("session");
    for incoming in [
        Some(heartbeat_session.clone()),
        None,
        Some(heartbeat_session.clone()),
    ] {
        let heartbeat = RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: TEST_TEAM.parse().expect("team"),
            member: ROLE_TEAM_LEAD.parse().expect("member"),
            pid: 42,
            observed_at: atm_core::types::IsoTimestamp::now(),
            activity: HeartbeatActivity::ActiveToolUse,
            session_id: incoming,
        });
        let mut heartbeat_stream =
            std::net::TcpStream::connect(record.ipv4_loopback.expect("loopback"))
                .expect("connect loopback TCP heartbeat");
        atm_core::api::write_http_request_with_headers(
            &mut heartbeat_stream,
            &heartbeat,
            &[(
                atm_core::local_http::LOCAL_CAPABILITY_HEADER,
                capability.as_str(),
            )],
        )
        .expect("write heartbeat");
        let response = atm_core::api::read_http_response(&mut heartbeat_stream, &heartbeat)
            .expect("read heartbeat response");
        let ResponseEnvelope::Heartbeat(response) = response else {
            panic!("unexpected heartbeat response")
        };
        assert_eq!(response.session_id, Some(heartbeat_session.clone()));
    }
    assert_eq!(
        status_cache.cached_session_id(test_team(), &ROLE_TEAM_LEAD.parse().expect("member")),
        Some(heartbeat_session),
    );
    let mut tcp_stream = std::net::TcpStream::connect(record.ipv4_loopback.expect("loopback"))
        .expect("connect loopback TCP");
    atm_core::api::write_http_request_with_headers(
        &mut tcp_stream,
        &request,
        &[(
            atm_core::local_http::LOCAL_CAPABILITY_HEADER,
            capability.as_str(),
        )],
    )
    .expect("write TCP send request");
    let tcp_response = atm_core::api::read_http_response(&mut tcp_stream, &request)
        .expect("read TCP send response");
    assert!(matches!(tcp_response, ResponseEnvelope::Send(_)));
    assert_eq!(
        status_cache.cached_session_id(test_team(), &ROLE_TEAM_LEAD.parse().expect("member")),
        Some(atm_core::types::SessionId::new("transport-session").expect("session")),
    );

    let ack = AckRequest {
        home_dir: atm_home,
        current_dir: workspace_dir,
        caller_identity: "qa-a".parse().expect("ack caller"),
        caller_chat_id: None,
        caller_team: TEST_TEAM.parse().expect("team"),
        activity_observation: None,
        message_id: sent_message_id,
        reply_body: "acknowledged over TCP".to_owned(),
    };
    let ack_request = RequestEnvelope::Write(Box::new(ack.into_write_request()));
    let mut ack_stream = std::net::TcpStream::connect(record.ipv4_loopback.expect("loopback"))
        .expect("connect loopback TCP ack");
    atm_core::api::write_http_request_with_headers(
        &mut ack_stream,
        &ack_request,
        &[(
            atm_core::local_http::LOCAL_CAPABILITY_HEADER,
            capability.as_str(),
        )],
    )
    .expect("write TCP ack request");
    let ack_response = atm_core::api::read_http_response(&mut ack_stream, &ack_request)
        .expect("read TCP ack response");
    assert!(
        matches!(
            ack_response,
            ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(_))
        ),
        "unexpected TCP ack response: {ack_response:?}"
    );

    lifecycle.set_terminate_for_test(true);
    serve_result_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("recv serve result")
        .expect("serve runtime result");
    join.join().expect("join serve thread");
}
