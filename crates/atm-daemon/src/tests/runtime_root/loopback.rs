use super::*;

#[test]
#[serial_test::serial(env)]
fn dispatcher_loopback_send_round_trips_through_peer_listener_into_self_inbox() {
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    let workspace_dir = tempdir.path().join("workspace");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir");
    let db_path = tempdir.path().join("mail.db");
    write_team_config(&atm_home, &[]);

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
                Some("localhost success fixture".to_string()),
            )
            .expect("allow host command"),
        )
        .expect("allow host");
    let status_cache = RuntimeStatusCache::new();
    let peer_transport = crate::PeerTransportRuntime::new_server_for_test_with_allowed_host_store(
        SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0)),
        crate::SubsystemObservability::disabled(crate::DaemonSubsystem::PeerTransport),
        status_cache.clone(),
        assembly.allowed_host_store_arc(),
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

    let mut request = SendRequest::new(
        atm_home.clone(),
        workspace_dir.clone(),
        ROLE_TEAM_LEAD.parse().expect("caller"),
        "qa-a@test-team",
        TEST_TEAM.parse().expect("team"),
        SendMessageSource::Inline("hello loopback".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("send request");
    request.remote_host = atm_core::send::parse_send_target("qa-a@test-team.localhost", None)
        .expect("parse target")
        .remote_host;

    let response = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(request)))
        .expect("dispatch loopback send");
    let ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) = response else {
        panic!("expected send response");
    };
    assert_eq!(outcome.agent.as_str(), "qa-a");

    let read = dispatcher
        .dispatch(RequestEnvelope::Receive(
            ReadQuery::new(
                atm_home,
                workspace_dir,
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
        ))
        .expect("read self inbox");
    let ResponseEnvelope::Receive(report) = read else {
        panic!("expected receive response");
    };
    assert!(
        report
            .message
            .as_ref()
            .is_some_and(|message| message.envelope.text == "hello loopback"),
        "localhost remote-target message missing from inbox"
    );

    peer_transport.shutdown().expect("shutdown peer listener");
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_loopback_send_rejects_unauthorized_host_before_mailbox_mutation() {
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    let workspace_dir = tempdir.path().join("workspace");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir");
    let db_path = tempdir.path().join("mail.db");
    write_team_config(&atm_home, &[]);

    add_member_via_retained_admin(
        &db_path,
        &atm_home,
        TEST_TEAM,
        ROLE_TEAM_LEAD,
        &workspace_dir,
    );
    add_member_via_retained_admin(&db_path, &atm_home, TEST_TEAM, "qa-a", &workspace_dir);

    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");
    let allowed_host = "127.0.0.1"
        .parse::<atm_storage::AllowedHostName>()
        .expect("allowed host");
    assembly
        .allowed_host_store_arc()
        .allow_host(
            atm_storage::AllowHostCommand::new(
                "127.0.0.1",
                format!("{ROLE_TEAM_LEAD}@{TEST_TEAM}"),
                Some("localhost rejection fixture".to_string()),
            )
            .expect("allow host command"),
        )
        .expect("allow host");
    assembly
        .allowed_host_store_arc()
        .deny_host(&allowed_host)
        .expect("deny host");
    let status_cache = RuntimeStatusCache::new();
    let peer_transport = crate::PeerTransportRuntime::new_server_for_test_with_allowed_host_store(
        SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0)),
        crate::SubsystemObservability::disabled(crate::DaemonSubsystem::PeerTransport),
        status_cache.clone(),
        assembly.allowed_host_store_arc(),
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

    let mut request = SendRequest::new(
        atm_home.clone(),
        workspace_dir.clone(),
        ROLE_TEAM_LEAD.parse().expect("caller"),
        "qa-a@test-team",
        TEST_TEAM.parse().expect("team"),
        SendMessageSource::Inline("unauthorized localhost".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("send request");
    request.remote_host = atm_core::send::parse_send_target("qa-a@test-team.localhost", None)
        .expect("parse target")
        .remote_host;

    let error = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(request)))
        .expect_err("unauthorized localhost send must fail closed");
    assert_eq!(error.code, AtmErrorCode::MessageValidationFailed);

    let read = dispatcher
        .dispatch(RequestEnvelope::Receive(
            ReadQuery::new(
                atm_home,
                workspace_dir,
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
        ))
        .expect("read self inbox");
    let ResponseEnvelope::Receive(report) = read else {
        panic!("expected receive response");
    };
    assert!(
        report.message.is_none(),
        "unauthorized localhost send mutated the receiver mailbox"
    );

    peer_transport.shutdown().expect("shutdown peer listener");
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_secure_loopback_requires_ack_round_trips_and_updates_reply_state() {
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    let workspace_dir = tempdir.path().join("workspace");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir");
    let db_path = tempdir.path().join("mail.db");
    write_team_config(&atm_home, &[]);

    add_member_via_retained_admin(
        &db_path,
        &atm_home,
        TEST_TEAM,
        ROLE_TEAM_LEAD,
        &workspace_dir,
    );
    add_member_via_retained_admin(&db_path, &atm_home, TEST_TEAM, "qa-a", &workspace_dir);
    configure_secure_loopback(&db_path, "127.0.0.1");
    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");

    let status_cache = RuntimeStatusCache::new();
    let peer_transport = crate::PeerTransportRuntime::new_with_observability(
        None,
        Some(assembly.allowed_host_store_arc()),
        Some(assembly.peer_security_store_arc()),
        crate::peer_transport::PeerTransportConfig {
            remote_retry_budget: Duration::from_secs(30),
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
        .expect("start secure peer listener");

    let mut request = SendRequest::new(
        atm_home.clone(),
        workspace_dir.clone(),
        ROLE_TEAM_LEAD.parse().expect("caller"),
        "qa-a@test-team",
        TEST_TEAM.parse().expect("team"),
        SendMessageSource::Inline("hello secure ack loopback".to_string()),
        None,
        true,
        None,
        false,
    )
    .expect("send request");
    request.remote_host = atm_core::send::parse_send_target("qa-a@test-team.localhost", None)
        .expect("parse target")
        .remote_host;

    let response = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(request)))
        .expect("dispatch secure loopback send");
    let ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) = response else {
        panic!("expected send response");
    };
    assert_eq!(outcome.agent.as_str(), "qa-a");
    assert!(outcome.requires_ack);

    let read = dispatcher
        .dispatch(RequestEnvelope::Receive(
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
        ))
        .expect("read self inbox");
    let ResponseEnvelope::Receive(report) = read else {
        panic!("expected receive response");
    };
    let message = report.message.expect("loopback message");
    assert_eq!(message.envelope.text, "hello secure ack loopback");
    let source_message_id = message.envelope.message_id.expect("message id");

    let ack = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(
            atm_core::ack::AckRequest {
                home_dir: atm_home.clone(),
                current_dir: workspace_dir.clone(),
                caller_identity: "qa-a".parse().expect("caller"),
                caller_team: TEST_TEAM.parse().expect("team"),
                message_id: source_message_id,
                reply_body: "ack from secure localhost".to_string(),
            },
        )))
        .expect("ack over secure localhost");
    let ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome)) = ack else {
        panic!("expected ack response");
    };
    assert!(matches!(
        outcome.reply_disposition,
        atm_core::ack::AckReplyDisposition::Sent { .. }
    ));

    let sender_read = dispatcher
        .dispatch(RequestEnvelope::Receive(
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
        ))
        .expect("read sender inbox");
    let ResponseEnvelope::Receive(report) = sender_read else {
        panic!("expected sender receive response");
    };
    let ack_message = report.message.expect("ack reply message");
    assert_eq!(ack_message.envelope.text, "ack from secure localhost");
    assert_eq!(
        ack_message.envelope.acknowledges_message_id,
        Some(source_message_id)
    );

    peer_transport
        .shutdown()
        .expect("shutdown secure peer listener");
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_secure_loopback_send_round_trips_through_peer_listener_into_self_inbox() {
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    let workspace_dir = tempdir.path().join("workspace");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir");
    let db_path = tempdir.path().join("mail.db");
    write_team_config(&atm_home, &[]);

    add_member_via_retained_admin(
        &db_path,
        &atm_home,
        TEST_TEAM,
        ROLE_TEAM_LEAD,
        &workspace_dir,
    );
    add_member_via_retained_admin(&db_path, &atm_home, TEST_TEAM, "qa-a", &workspace_dir);
    configure_secure_loopback(&db_path, "127.0.0.1");
    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");

    let status_cache = RuntimeStatusCache::new();
    let peer_transport = crate::PeerTransportRuntime::new_with_observability(
        None,
        Some(assembly.allowed_host_store_arc()),
        Some(assembly.peer_security_store_arc()),
        crate::peer_transport::PeerTransportConfig {
            remote_retry_budget: Duration::from_secs(30),
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
        .expect("start secure peer listener");

    let mut request = SendRequest::new(
        atm_home.clone(),
        workspace_dir.clone(),
        ROLE_TEAM_LEAD.parse().expect("caller"),
        "qa-a@test-team",
        TEST_TEAM.parse().expect("team"),
        SendMessageSource::Inline("hello secure loopback".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("send request");
    request.remote_host = atm_core::send::parse_send_target("qa-a@test-team.localhost", None)
        .expect("parse target")
        .remote_host;

    let response = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(request)))
        .expect("dispatch secure loopback send");
    let ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) = response else {
        panic!("expected send response");
    };
    assert_eq!(outcome.agent.as_str(), "qa-a");

    let read = dispatcher
        .dispatch(RequestEnvelope::Receive(
            ReadQuery::new(
                atm_home,
                workspace_dir,
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
        ))
        .expect("read self inbox");
    let ResponseEnvelope::Receive(report) = read else {
        panic!("expected receive response");
    };
    assert!(
        report
            .message
            .as_ref()
            .is_some_and(|message| message.envelope.text == "hello secure loopback"),
        "secure loopback-delivered message missing from inbox"
    );

    peer_transport
        .shutdown()
        .expect("shutdown secure peer listener");
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_self_ip_send_rejects_disabled_host_before_mailbox_mutation() {
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    let workspace_dir = tempdir.path().join("workspace");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir");
    let db_path = tempdir.path().join("mail.db");
    write_team_config(&atm_home, &[]);

    add_member_via_retained_admin(
        &db_path,
        &atm_home,
        TEST_TEAM,
        ROLE_TEAM_LEAD,
        &workspace_dir,
    );
    add_member_via_retained_admin(&db_path, &atm_home, TEST_TEAM, "qa-a", &workspace_dir);

    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");
    let allowed_host = "127.0.0.1"
        .parse::<atm_storage::AllowedHostName>()
        .expect("allowed host");
    assembly
        .allowed_host_store_arc()
        .allow_host(
            atm_storage::AllowHostCommand::new(
                "127.0.0.1",
                format!("{ROLE_TEAM_LEAD}@{TEST_TEAM}"),
                Some("self-ip rejection fixture".to_string()),
            )
            .expect("allow host command"),
        )
        .expect("allow host");
    assembly
        .allowed_host_store_arc()
        .deny_host(&allowed_host)
        .expect("deny host");

    let status_cache = RuntimeStatusCache::new();
    let peer_transport = crate::PeerTransportRuntime::new_server_for_test_with_allowed_host_store(
        SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0)),
        crate::SubsystemObservability::disabled(crate::DaemonSubsystem::PeerTransport),
        status_cache.clone(),
        assembly.allowed_host_store_arc(),
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

    let mut request = SendRequest::new(
        atm_home.clone(),
        workspace_dir.clone(),
        ROLE_TEAM_LEAD.parse().expect("caller"),
        "qa-a@test-team",
        TEST_TEAM.parse().expect("team"),
        SendMessageSource::Inline("unauthorized self ip".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("send request");
    request.remote_host = atm_core::send::parse_send_target("qa-a@test-team.127.0.0.1", None)
        .expect("parse target")
        .remote_host;

    let error = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(request)))
        .expect_err("self-ip send must fail closed");
    assert_eq!(error.code, AtmErrorCode::MessageValidationFailed);

    let read = dispatcher
        .dispatch(RequestEnvelope::Receive(
            ReadQuery::new(
                atm_home,
                workspace_dir,
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
        ))
        .expect("read self inbox");
    let ResponseEnvelope::Receive(report) = read else {
        panic!("expected receive response");
    };
    assert!(
        report.message.is_none(),
        "unauthorized self-ip send mutated the receiver mailbox"
    );

    peer_transport.shutdown().expect("shutdown peer listener");
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_localhost_without_listener_fails_closed_without_mailbox_mutation() {
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    let workspace_dir = tempdir.path().join("workspace");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir");
    let db_path = tempdir.path().join("mail.db");
    write_team_config(&atm_home, &[]);

    add_member_via_retained_admin(
        &db_path,
        &atm_home,
        TEST_TEAM,
        ROLE_TEAM_LEAD,
        &workspace_dir,
    );
    add_member_via_retained_admin(&db_path, &atm_home, TEST_TEAM, "qa-a", &workspace_dir);

    let dispatcher = DaemonRequestDispatcher::new_for_test(
        atm_home.clone(),
        RuntimeStatusCache::new(),
        db_path.clone(),
    );
    let mut request = SendRequest::new(
        atm_home.clone(),
        workspace_dir.clone(),
        ROLE_TEAM_LEAD.parse().expect("caller"),
        "qa-a@test-team",
        TEST_TEAM.parse().expect("team"),
        SendMessageSource::Inline("localhost without listener".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("send request");
    request.remote_host = atm_core::send::parse_send_target("qa-a@test-team.localhost", None)
        .expect("parse target")
        .remote_host;

    let error = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(request)))
        .expect_err("localhost send without listener must fail closed");
    assert_eq!(error.code, AtmErrorCode::DaemonUnavailable);

    let read = dispatcher
        .dispatch(RequestEnvelope::Receive(
            ReadQuery::new(
                atm_home,
                workspace_dir,
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
        ))
        .expect("read self inbox");
    let ResponseEnvelope::Receive(report) = read else {
        panic!("expected receive response");
    };
    assert!(
        report.message.is_none(),
        "localhost send without listener mutated the receiver mailbox"
    );
}
