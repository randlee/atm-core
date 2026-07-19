use super::*;

#[test]
#[serial_test::serial(env)]
fn local_ipc_runtime_round_trips_send_after_add_member_roster_state() {
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
    let dispatcher: Arc<dyn RequestDispatcher + Send + Sync> =
        Arc::new(DaemonRequestDispatcher::new_for_test(
            atm_home.clone(),
            RuntimeStatusCache::new(),
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
                finalize_shutdown: || {},
                publish_ready: move || {
                    ready_tx.send(()).map_err(|_| {
                        AtmError::daemon_unavailable(
                            "send round-trip test failed to observe the daemon ready signal",
                        )
                        .with_recovery(
                            "Rerun the same-host daemon test after restoring the bounded ready-signal handshake.",
                        )
                    })
                },
            },
        );
        serve_result_tx.send(result).expect("send serve result");
    });

    let mut stream = connect_daemon_local_ipc_until_ready(&socket_path, ready_rx);
    configure_test_local_ipc_timeouts(&stream);
    let request = RequestEnvelope::Send(Box::new(
        SendRequest::new(
            atm_home.clone(),
            workspace_dir.clone(),
            ROLE_TEAM_LEAD.parse().expect("caller"),
            "qa-a@test-team",
            TEST_TEAM.parse().expect("team"),
            SendMessageSource::Inline("hello local ipc".to_string()),
            None,
            false,
            None,
            false,
        )
        .expect("send request"),
    ));
    let request_id = next_request_id();
    let frame = atm_core::protocol::request_to_frame_payload(request_id, request).expect("frame");
    atm_core::protocol::write_frame(&mut stream, &frame, "write send frame").expect("write");
    stream.flush().expect("flush");
    let response_frame =
        atm_core::protocol::read_frame(&mut stream, "read send frame", "send frame too large")
            .expect("read frame")
            .expect("response frame");
    let (response_id, response) =
        atm_core::protocol::response_from_frame_payload(response_frame).expect("decode response");
    assert_eq!(response_id, request_id);
    match response {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => {
            assert_eq!(outcome.outcome.as_str(), "sent");
        }
        other => panic!("unexpected response: {other:?}"),
    }

    lifecycle.set_terminate_for_test(true);
    serve_result_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("recv serve result")
        .expect("serve runtime result");
    join.join().expect("join serve thread");
}

#[test]
#[serial_test::serial(env)]
fn local_ipc_client_preflight_round_trips_ack_required_send_after_add_member_roster_state() {
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
    let dispatcher: Arc<dyn RequestDispatcher + Send + Sync> =
        Arc::new(DaemonRequestDispatcher::new_for_test(
            atm_home.clone(),
            RuntimeStatusCache::new(),
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
                finalize_shutdown: || {},
                publish_ready: move || {
                    ready_tx.send(()).map_err(|_| {
                        AtmError::daemon_unavailable(
                            "send round-trip test failed to observe the daemon ready signal",
                        )
                        .with_recovery(
                            "Rerun the same-host daemon test after restoring the bounded ready-signal handshake.",
                        )
                    })
                },
            },
        );
        serve_result_tx.send(result).expect("send serve result");
    });

    let _stream = connect_daemon_local_ipc_until_ready(&socket_path, ready_rx);
    let endpoint =
        atm_daemon_client::DaemonLocalIpcEndpoint::new(socket_path.clone()).expect("endpoint");
    let request = SendRequest::new(
        atm_home.clone(),
        workspace_dir.clone(),
        ROLE_TEAM_LEAD.parse().expect("caller"),
        "qa-a@test-team",
        TEST_TEAM.parse().expect("team"),
        SendMessageSource::Inline("hello local ipc ack-required".to_string()),
        None,
        true,
        None,
        false,
    )
    .expect("send request");
    let request = RequestEnvelope::Send(Box::new(request));
    let envelope = atm_daemon_client::RpcEnvelope::encode_body(
        atm_daemon_client::RpcHeader::new(
            atm_daemon_client::RequestId::new(next_request_id().into_inner()).expect("request id"),
            atm_daemon_client::MessageKind::SendRequest,
        ),
        &request,
    )
    .expect("encode request");

    let mut verified = atm_daemon_client::verify_connection_compatibility(
        &endpoint,
        atm_daemon_client::CompatibilityPreflight {
            client_release: atm_daemon_client::ReleaseVersion::current(),
            wire_version: atm_core::protocol::ATM_FRAME_VERSION_V1,
        },
        Duration::from_secs(3),
    )
    .expect("preflight compatible");
    let response = verified
        .dispatch_write(&endpoint, envelope, Duration::from_secs(3))
        .expect("dispatch write");
    let response: ResponseEnvelope = response.decode_body().expect("decode response");
    match response {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => {
            assert_eq!(outcome.outcome.as_str(), "sent");
            assert!(outcome.requires_ack);
        }
        other => panic!("unexpected response: {other:?}"),
    }

    lifecycle.set_terminate_for_test(true);
    serve_result_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("recv serve result")
        .expect("serve runtime result");
    join.join().expect("join serve thread");
}

#[test]
#[serial_test::serial(env)]
fn local_ipc_runtime_round_trips_remote_target_send_read_and_ack_over_production_dispatch() {
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

    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");
    assembly
        .allowed_host_store_arc()
        .allow_host(
            atm_storage::AllowHostCommand::new(
                "127.0.0.1",
                format!("{ROLE_TEAM_LEAD}@{TEST_TEAM}"),
                Some("ag14 tier3 localhost fixture".to_string()),
            )
            .expect("allow host command"),
        )
        .expect("allow host");
    let status_cache = RuntimeStatusCache::new();
    let peer_transport = crate::PeerTransportRuntime::new_with_observability(
        None,
        Some(assembly.allowed_host_store_arc()),
        None,
        crate::peer_transport::PeerTransportConfig {
            peer_listen_addr: Some(SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0))),
        },
        crate::SubsystemObservability::disabled(crate::DaemonSubsystem::PeerTransport),
        status_cache.clone(),
    );
    let dispatcher = Arc::new(DaemonRequestDispatcher::new_for_test_with_peer_transport(
        atm_home.clone(),
        status_cache,
        db_path.clone(),
        peer_transport.clone(),
    ));
    peer_transport
        .start(dispatcher.clone())
        .expect("start peer listener");

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
    let (serve_result_tx, serve_result_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let dispatch_for_runtime: Arc<dyn RequestDispatcher + Send + Sync> = dispatcher.clone();

    let join = std::thread::spawn(move || {
        let result = runtime.serve_with_runtime_hooks(
            dispatch_for_runtime,
            RuntimeServeHooks {
                endpoint_guard,
                graceful_drain_deadline: Duration::from_millis(500),
                force_cancel_deadline: Duration::from_secs(2),
                begin_shutdown: || Ok(()),
                reload_runtime_view: || Ok(()),
                finalize_shutdown: || {},
                publish_ready: move || {
                    ready_tx.send(()).map_err(|_| {
                        AtmError::daemon_unavailable(
                            "remote-target round-trip test failed to observe the daemon ready signal",
                        )
                        .with_recovery(
                            "Rerun the same-host daemon test after restoring the bounded ready-signal handshake.",
                        )
                    })
                },
            },
        );
        serve_result_tx.send(result).expect("send serve result");
    });

    let _ready_stream = connect_daemon_local_ipc_until_ready(&socket_path, ready_rx);
    let ipc_name = atm_core::protocol::daemon_local_ipc_name_from_path(&socket_path)
        .expect("ipc name")
        .into_owned();
    let dispatch_once = |request_id,
                         request: RequestEnvelope,
                         write_label: &'static str,
                         read_label: &'static str| {
        let frame =
            atm_core::protocol::request_to_frame_payload(request_id, request).expect("frame");
        let mut stream = crate::test_support::connect_local_ipc_with_timeout(
            ipc_name.clone(),
            Duration::from_secs(3),
        )
        .expect("connect local ipc");
        configure_test_local_ipc_timeouts(&stream);
        atm_core::protocol::write_frame(&mut stream, &frame, write_label).expect("write");
        stream.flush().expect("flush");
        let response_frame =
            atm_core::protocol::read_frame(&mut stream, read_label, "response frame too large")
                .expect("read frame")
                .expect("response frame");
        let (response_id, response) =
            atm_core::protocol::response_from_frame_payload(response_frame)
                .expect("decode response");
        assert_eq!(response_id, request_id);
        response
    };

    let mut send_request = SendRequest::new(
        atm_home.clone(),
        workspace_dir.clone(),
        ROLE_TEAM_LEAD.parse().expect("caller"),
        "qa-a@test-team",
        TEST_TEAM.parse().expect("team"),
        SendMessageSource::Inline("hello remote-target over local ipc".to_string()),
        None,
        true,
        None,
        false,
    )
    .expect("send request");
    send_request.remote_host = atm_core::send::parse_send_target("qa-a@test-team.localhost", None)
        .expect("parse target")
        .remote_host;
    let send_request = RequestEnvelope::Send(Box::new(send_request));
    let send_request_id = next_request_id();
    let send_response = dispatch_once(
        send_request_id,
        send_request,
        "write send frame",
        "read send frame",
    );
    match send_response {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => {
            assert_eq!(outcome.outcome.as_str(), "sent");
            assert!(outcome.requires_ack);
        }
        other => panic!("unexpected send response: {other:?}"),
    }

    let read_request_id = next_request_id();
    let read_request = RequestEnvelope::Receive(
        ReadQuery::new(
            atm_home.clone(),
            workspace_dir.clone(),
            "qa-a".parse().expect("caller"),
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
        .expect("read query"),
    );
    let read_response = dispatch_once(
        read_request_id,
        read_request,
        "write read frame",
        "read read frame",
    );
    let source_message_id = match read_response {
        ResponseEnvelope::Receive(report) => {
            let message = report.message.expect("receiver message");
            assert_eq!(message.envelope.text, "hello remote-target over local ipc");
            message.envelope.message_id.expect("message id")
        }
        other => panic!("unexpected read response: {other:?}"),
    };

    let ack_request_id = next_request_id();
    let ack_request = canonical_ack_request(
        &atm_home,
        &workspace_dir,
        "qa-a",
        TEST_TEAM,
        source_message_id,
        "ack from remote-target local ipc",
    );
    let ack_response = dispatch_once(
        ack_request_id,
        ack_request,
        "write ack frame",
        "read ack frame",
    );
    match ack_response {
        ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome)) => {
            assert!(!outcome.reply_message_id.to_string().is_empty());
        }
        other => panic!("unexpected ack response: {other:?}"),
    }

    let sender_read_request_id = next_request_id();
    let sender_read_request = RequestEnvelope::Receive(
        ReadQuery::new(
            atm_home,
            workspace_dir,
            ROLE_TEAM_LEAD.parse().expect("caller"),
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
        .expect("read query"),
    );
    let sender_read_response = dispatch_once(
        sender_read_request_id,
        sender_read_request,
        "write sender read frame",
        "read sender frame",
    );
    match sender_read_response {
        ResponseEnvelope::Receive(report) => {
            let message = report.message.expect("sender ack reply");
            assert_eq!(message.envelope.text, "ack from remote-target local ipc");
            assert_eq!(
                message.envelope.acknowledges_message_id,
                Some(source_message_id)
            );
        }
        other => panic!("unexpected sender read response: {other:?}"),
    }

    lifecycle.set_terminate_for_test(true);
    serve_result_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("recv serve result")
        .expect("serve runtime result");
    join.join().expect("join serve thread");
    peer_transport.shutdown().expect("shutdown peer listener");
}
