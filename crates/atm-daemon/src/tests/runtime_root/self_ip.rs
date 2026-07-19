use super::*;

#[test]
#[serial_test::serial(env)]
fn dispatcher_self_ip_send_round_trips_through_peer_listener_into_self_inbox() {
    install_retained_runtime_factory();
    let self_ip = discover_non_loopback_ipv4_for_test();
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
                self_ip.to_string(),
                format!("{ROLE_TEAM_LEAD}@{TEST_TEAM}"),
                Some("self-ip success fixture".to_string()),
            )
            .expect("allow host command"),
        )
        .expect("allow host");
    let status_cache = RuntimeStatusCache::new();
    let peer_transport = crate::PeerTransportRuntime::new_server_for_test_with_allowed_host_store(
        SocketAddr::from((self_ip, 0)),
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

    let mut request = canonical_send_request(
        &atm_home,
        &workspace_dir,
        ROLE_TEAM_LEAD,
        "qa-a@test-team",
        TEST_TEAM,
        "hello self ip",
        false,
    );
    request.remote_host =
        atm_core::send::parse_send_target("qa-a@test-team", Some(&self_ip.to_string()))
            .expect("parse target")
            .remote_host;

    let response = dispatcher
        .dispatch(RequestEnvelope::Send(Box::new(request)))
        .expect("dispatch self-ip send");
    let ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) = response else {
        panic!("expected send response");
    };
    assert_eq!(outcome.agent.as_str(), "qa-a");
    assert_eq!(outcome.outcome.as_str(), "sent");

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
            .is_some_and(|message| message.envelope.text == "hello self ip"),
        "self-ip remote-target message missing from inbox"
    );

    peer_transport
        .shutdown()
        .expect("shutdown secure peer listener");
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_secure_self_ip_requires_ack_round_trips_and_updates_reply_state() {
    install_retained_runtime_factory();
    let self_ip = discover_non_loopback_ipv4_for_test();
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
    configure_secure_loopback(&db_path, &self_ip.to_string());
    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");

    let status_cache = RuntimeStatusCache::new();
    let peer_transport = crate::PeerTransportRuntime::new_with_observability(
        None,
        Some(assembly.allowed_host_store_arc()),
        Some(assembly.peer_security_store_arc()),
        crate::peer_transport::PeerTransportConfig {
            remote_retry_budget: Duration::from_secs(30),
            peer_listen_addr: Some(SocketAddr::from((self_ip, 0))),
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

    let mut request = canonical_send_request(
        &atm_home,
        &workspace_dir,
        ROLE_TEAM_LEAD,
        "qa-a@test-team",
        TEST_TEAM,
        "hello secure self ip",
        true,
    );
    request.remote_host =
        atm_core::send::parse_send_target("qa-a@test-team", Some(&self_ip.to_string()))
            .expect("parse target")
            .remote_host;

    let response = dispatcher
        .dispatch(RequestEnvelope::Send(Box::new(request)))
        .expect("dispatch secure self-ip send");
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
    let message = report.message.expect("self-ip message");
    assert_eq!(message.envelope.text, "hello secure self ip");
    let expected_remote_host = self_ip.to_string();
    assert_eq!(
        message
            .envelope
            .extra
            .get("metadata")
            .and_then(serde_json::Value::as_object)
            .and_then(|metadata| metadata.get("atm"))
            .and_then(serde_json::Value::as_object)
            .and_then(|atm| atm.get("remoteHost"))
            .and_then(serde_json::Value::as_str),
        Some(expected_remote_host.as_str())
    );
    let source_message_id = message.envelope.message_id.expect("message id");

    let ack = dispatcher
        .dispatch(canonical_ack_request(
            &atm_home,
            &workspace_dir,
            "qa-a",
            TEST_TEAM,
            source_message_id,
            "ack from secure self ip",
        ))
        .expect("ack over secure self ip");
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
    assert_eq!(ack_message.envelope.text, "ack from secure self ip");
    assert_eq!(
        ack_message.envelope.acknowledges_message_id,
        Some(source_message_id)
    );
    let expected_remote_host = self_ip.to_string();
    assert_eq!(
        ack_message
            .envelope
            .extra
            .get("metadata")
            .and_then(serde_json::Value::as_object)
            .and_then(|metadata| metadata.get("atm"))
            .and_then(serde_json::Value::as_object)
            .and_then(|atm| atm.get("remoteHost"))
            .and_then(serde_json::Value::as_str),
        Some(expected_remote_host.as_str())
    );

    peer_transport
        .shutdown()
        .expect("shutdown secure peer listener");
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_secure_self_ip_failed_ack_keeps_source_pending() {
    install_retained_runtime_factory();
    let self_ip = discover_non_loopback_ipv4_for_test();
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
    configure_secure_loopback(&db_path, &self_ip.to_string());
    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");

    let status_cache = RuntimeStatusCache::new();
    let peer_transport = crate::PeerTransportRuntime::new_with_observability(
        None,
        Some(assembly.allowed_host_store_arc()),
        Some(assembly.peer_security_store_arc()),
        crate::peer_transport::PeerTransportConfig {
            remote_retry_budget: Duration::from_secs(30),
            peer_listen_addr: Some(SocketAddr::from((self_ip, 0))),
        },
        crate::SubsystemObservability::disabled(crate::DaemonSubsystem::PeerTransport),
        status_cache.clone(),
    );
    let dispatcher = Arc::new(DaemonRequestDispatcher::new_for_test_with_peer_transport(
        atm_home.clone(),
        status_cache,
        db_path,
        peer_transport.clone(),
    ));
    peer_transport
        .start(dispatcher.clone())
        .expect("start secure peer listener");

    let mut request = canonical_send_request(
        &atm_home,
        &workspace_dir,
        ROLE_TEAM_LEAD,
        "qa-a@test-team",
        TEST_TEAM,
        "hello failed ack self ip",
        true,
    );
    request.remote_host =
        atm_core::send::parse_send_target("qa-a@test-team", Some(&self_ip.to_string()))
            .expect("parse target")
            .remote_host;

    let response = dispatcher
        .dispatch(RequestEnvelope::Send(Box::new(request)))
        .expect("dispatch secure self-ip send");
    let ResponseEnvelope::Send(SendResponseEnvelope::Sent(_)) = response else {
        panic!("expected send response");
    };

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
    let message = report.message.expect("self-ip message");
    let source_message_id = message.envelope.message_id.expect("message id");

    peer_transport
        .shutdown()
        .expect("shutdown secure peer listener before ack");

    let error = dispatcher
        .dispatch(canonical_ack_request(
            &atm_home,
            &workspace_dir,
            "qa-a",
            TEST_TEAM,
            source_message_id,
            "ack should fail while peer listener is down",
        ))
        .expect_err("ack must fail when secure self-ip peer listener is down");
    assert_eq!(error.code, AtmErrorCode::DaemonUnavailable);

    let read_after_failure = dispatcher
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
        .expect("read self inbox after failed ack");
    let ResponseEnvelope::Receive(report) = read_after_failure else {
        panic!("expected receive response");
    };
    assert_eq!(report.bucket_counts.pending_ack, 1);
    let message = report.message.expect("pending ack message");
    assert_eq!(message.envelope.message_id, Some(source_message_id));
    assert!(message.envelope.acknowledged_at.is_none());
    assert!(message.envelope.pending_ack_at.is_some());
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_self_ip_without_listener_fails_closed_without_mailbox_mutation() {
    install_retained_runtime_factory();
    let self_ip = discover_non_loopback_ipv4_for_test();
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
    let mut request = canonical_send_request(
        &atm_home,
        &workspace_dir,
        ROLE_TEAM_LEAD,
        "qa-a@test-team",
        TEST_TEAM,
        "self ip without listener",
        false,
    );
    request.remote_host =
        atm_core::send::parse_send_target("qa-a@test-team", Some(&self_ip.to_string()))
            .expect("parse target")
            .remote_host;

    let error = dispatcher
        .dispatch(RequestEnvelope::Send(Box::new(request)))
        .expect_err("self-ip send without listener must fail closed");
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
        "self-ip send without listener mutated the receiver mailbox"
    );
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_self_ip_send_rejects_disabled_host_before_mailbox_mutation() {
    install_retained_runtime_factory();
    let self_ip = discover_non_loopback_ipv4_for_test();
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
    let allowed_host = self_ip
        .to_string()
        .parse::<atm_storage::AllowedHostName>()
        .expect("allowed host");
    assembly
        .allowed_host_store_arc()
        .allow_host(
            atm_storage::AllowHostCommand::new(
                self_ip.to_string(),
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
        SocketAddr::from((self_ip, 0)),
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

    let mut request = canonical_send_request(
        &atm_home,
        &workspace_dir,
        ROLE_TEAM_LEAD,
        "qa-a@test-team",
        TEST_TEAM,
        "unauthorized self ip",
        false,
    );
    request.remote_host =
        atm_core::send::parse_send_target("qa-a@test-team", Some(&self_ip.to_string()))
            .expect("parse target")
            .remote_host;

    let error = dispatcher
        .dispatch(RequestEnvelope::Send(Box::new(request)))
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
