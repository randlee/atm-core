use super::*;

#[test]
#[serial_test::serial(env)]
fn local_peer_listener_harness_exercises_send_read_and_ack_request_path() {
    let _guard = install_shared_lifecycle_reset_guard();
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    let workspace_dir = tempdir.path().join("workspace");
    let db_path = tempdir.path().join("mail.db");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir");
    write_workspace_config(&workspace_dir);
    install_test_roster_with_harness(
        &db_path,
        &[
            (
                ROLE_TEAM_LEAD,
                RosterHarness::ClaudeCode,
                workspace_dir.as_path(),
            ),
            ("qa-a", RosterHarness::ClaudeCode, workspace_dir.as_path()),
        ],
    );
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD, "qa-a"]);

    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");
    assembly
        .allowed_host_store
        .allow_host(
            atm_storage::AllowHostCommand::new(
                "127.0.0.1",
                test_sender_identity(),
                Some("loopback".to_string()),
            )
            .expect("allow host command"),
        )
        .expect("allow host");
    let status_cache = RuntimeStatusCache::new();
    let listener_transport = PeerTransportRuntime::new_server_for_test_with_allowed_host_store(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
        status_cache.clone(),
        assembly.allowed_host_store_arc(),
    );
    let dispatcher = Arc::new(DaemonRequestDispatcher::new_for_test_with_peer_transport(
        atm_home.clone(),
        status_cache,
        db_path.clone(),
        listener_transport.clone(),
    ));
    listener_transport
        .start(dispatcher)
        .expect("start peer listener");
    let endpoint = listener_transport
        .bound_addr_for_test()
        .expect("bound peer listener addr");

    let client_transport = PeerTransportRuntime::new_for_test(
        endpoint,
        PeerTransportConfig::default(),
        tempdir.path().join("replay.db"),
    );
    let send_response = client_transport
        .client_transport()
        .send(RequestEnvelope::Send(SendRequestEnvelope::Compose(
            SendRequest::new(
                atm_home.clone(),
                workspace_dir.clone(),
                ROLE_TEAM_LEAD.parse().expect("caller"),
                "qa-a@test-team",
                test_team_name(),
                SendMessageSource::Inline("peer-listener hello".to_string()),
                None,
                true,
                None,
                false,
            )
            .expect("send request"),
        )))
        .expect("authorized send should succeed");
    match send_response {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => {
            assert_eq!(outcome.outcome.as_str(), "sent");
            assert!(outcome.requires_ack);
        }
        other => panic!("unexpected send response: {other:?}"),
    }

    let read_response = client_transport
        .client_transport()
        .send(RequestEnvelope::Receive(
            ReadQuery::new(
                atm_home.clone(),
                workspace_dir.clone(),
                "qa-a".parse().expect("caller"),
                None,
                test_team_name(),
                atm_core::types::ReadSelection::All,
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
        .expect("authorized read should succeed");
    let source_message_id = match read_response {
        ResponseEnvelope::Receive(outcome) => {
            let message = outcome.message.expect("message");
            assert_eq!(message.envelope.text, "peer-listener hello");
            message.envelope.message_id.expect("message id")
        }
        other => panic!("unexpected read response: {other:?}"),
    };

    let ack_response = client_transport
        .client_transport()
        .send(RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(
            AckRequest {
                home_dir: atm_home.clone(),
                current_dir: workspace_dir.clone(),
                caller_identity: "qa-a".parse().expect("caller"),
                caller_team: test_team_name(),
                message_id: source_message_id,
                reply_body: "ack over peer listener".to_string(),
            },
        )))
        .expect("authorized ack should succeed");
    match ack_response {
        ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome)) => {
            assert!(matches!(
                outcome.reply_disposition,
                atm_core::ack::AckReplyDisposition::Sent { .. }
            ));
            assert!(outcome.warnings.is_empty());
        }
        other => panic!("unexpected ack response: {other:?}"),
    }

    let sender_read_response = client_transport
        .client_transport()
        .send(RequestEnvelope::Receive(
            ReadQuery::new(
                atm_home.clone(),
                workspace_dir,
                ROLE_TEAM_LEAD.parse().expect("caller"),
                None,
                test_team_name(),
                atm_core::types::ReadSelection::All,
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
        .expect("sender read should succeed");
    match sender_read_response {
        ResponseEnvelope::Receive(outcome) => {
            let message = outcome.message.expect("ack reply message");
            assert_eq!(message.envelope.text, "ack over peer listener");
            assert_eq!(
                message.envelope.acknowledges_message_id,
                Some(source_message_id)
            );
        }
        other => panic!("unexpected sender read response: {other:?}"),
    }

    listener_transport
        .shutdown()
        .expect("shutdown peer listener");
}

#[test]
#[serial_test::serial(env)]
fn local_peer_listener_harness_preserves_sent_outcome_when_post_send_degrades() {
    let _guard = install_shared_lifecycle_reset_guard();
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    let workspace_dir = tempdir.path().join("workspace");
    let db_path = tempdir.path().join("mail.db");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir");
    install_test_roster_with_harness(
        &db_path,
        &[
            (
                ROLE_TEAM_LEAD,
                RosterHarness::ClaudeCode,
                workspace_dir.as_path(),
            ),
            ("qa-a", RosterHarness::CodexCli, workspace_dir.as_path()),
        ],
    );
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD, "qa-a"]);

    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");
    assembly
        .allowed_host_store
        .allow_host(
            atm_storage::AllowHostCommand::new(
                "127.0.0.1",
                test_sender_identity(),
                Some("loopback".to_string()),
            )
            .expect("allow host command"),
        )
        .expect("allow host");
    let listener_transport = PeerTransportRuntime::new_server_for_test_with_allowed_host_store(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
        RuntimeStatusCache::new(),
        assembly.allowed_host_store_arc(),
    );
    let dispatcher = Arc::new(DaemonRequestDispatcher::new_for_test(
        atm_home.clone(),
        RuntimeStatusCache::new(),
        db_path,
    ));
    listener_transport
        .start(dispatcher)
        .expect("start peer listener");
    let endpoint = listener_transport
        .bound_addr_for_test()
        .expect("bound peer listener addr");

    let client_transport = PeerTransportRuntime::new_for_test(
        endpoint,
        PeerTransportConfig::default(),
        tempdir.path().join("replay.db"),
    );
    let response = client_transport
        .client_transport()
        .send(RequestEnvelope::Send(SendRequestEnvelope::Compose(
            SendRequest::new(
                atm_home,
                workspace_dir,
                ROLE_TEAM_LEAD.parse().expect("caller"),
                "qa-a@test-team",
                test_team_name(),
                SendMessageSource::Inline("hello degraded nudge".to_string()),
                None,
                false,
                None,
                false,
            )
            .expect("send request"),
        )))
        .expect("send should still succeed");
    match response {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => {
            assert_eq!(outcome.outcome.as_str(), "sent");
            assert_eq!(outcome.warnings.len(), 1);
            assert_eq!(
                outcome.warnings[0].code,
                Some(AtmErrorCode::PostSendGraftUnavailable)
            );
            assert!(outcome.warnings[0].recovery.is_some());
        }
        other => panic!("unexpected response: {other:?}"),
    }

    listener_transport
        .shutdown()
        .expect("shutdown peer listener");
}

#[test]
#[serial_test::serial(env)]
fn local_peer_listener_harness_recovers_after_transient_connect_failure_and_delivers_on_retry() {
    let _guard = install_shared_lifecycle_reset_guard();
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    let workspace_dir = tempdir.path().join("workspace");
    let db_path = tempdir.path().join("mail.db");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir");
    write_workspace_config(&workspace_dir);
    install_test_roster_with_harness(
        &db_path,
        &[
            (
                ROLE_TEAM_LEAD,
                RosterHarness::ClaudeCode,
                workspace_dir.as_path(),
            ),
            ("qa-a", RosterHarness::ClaudeCode, workspace_dir.as_path()),
        ],
    );
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD, "qa-a"]);

    let reserved = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("reserve endpoint");
    let endpoint = reserved.local_addr().expect("reserved endpoint addr");
    drop(reserved);

    let client_transport = PeerTransportRuntime::new_for_test(
        endpoint,
        PeerTransportConfig::default(),
        tempdir.path().join("replay.db"),
    );
    let transient_error = client_transport
        .client_transport()
        .send(RequestEnvelope::Send(SendRequestEnvelope::Compose(
            SendRequest::new(
                atm_home.clone(),
                workspace_dir.clone(),
                ROLE_TEAM_LEAD.parse().expect("caller"),
                "qa-a@test-team",
                test_team_name(),
                SendMessageSource::Inline("retry after listener starts".to_string()),
                None,
                true,
                None,
                false,
            )
            .expect("send request"),
        )))
        .expect_err("initial send should fail before the listener starts");
    assert!(matches!(
        transient_error.code,
        AtmErrorCode::DaemonUnavailable | AtmErrorCode::RemoteDeliveryOutcomeUnknown
    ));
    assert!(transient_error.primary_recovery().is_some());

    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");
    assembly
        .allowed_host_store
        .allow_host(
            atm_storage::AllowHostCommand::new(
                "127.0.0.1",
                test_sender_identity(),
                Some("loopback".to_string()),
            )
            .expect("allow host command"),
        )
        .expect("allow host");
    let status_cache = RuntimeStatusCache::new();
    let listener_transport = PeerTransportRuntime::new_server_for_test_with_allowed_host_store(
        endpoint,
        SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
        status_cache.clone(),
        assembly.allowed_host_store_arc(),
    );
    let dispatcher = Arc::new(DaemonRequestDispatcher::new_for_test_with_peer_transport(
        atm_home.clone(),
        status_cache.clone(),
        db_path.clone(),
        listener_transport.clone(),
    ));
    listener_transport
        .start(dispatcher)
        .expect("start peer listener");

    let send_response = client_transport
        .client_transport()
        .send(RequestEnvelope::Send(SendRequestEnvelope::Compose(
            SendRequest::new(
                atm_home.clone(),
                workspace_dir.clone(),
                ROLE_TEAM_LEAD.parse().expect("caller"),
                "qa-a@test-team",
                test_team_name(),
                SendMessageSource::Inline("retry after listener starts".to_string()),
                None,
                true,
                None,
                false,
            )
            .expect("send request"),
        )))
        .expect("send should succeed after listener startup");
    match send_response {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => {
            assert_eq!(outcome.outcome.as_str(), "sent");
            assert!(outcome.warnings.is_empty());
        }
        other => panic!("unexpected send response: {other:?}"),
    }

    let read_response = client_transport
        .client_transport()
        .send(RequestEnvelope::Receive(
            ReadQuery::new(
                atm_home,
                workspace_dir,
                "qa-a".parse().expect("caller"),
                None,
                test_team_name(),
                atm_core::types::ReadSelection::All,
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
        .expect("read after recovery should succeed");
    match read_response {
        ResponseEnvelope::Receive(outcome) => {
            let message = outcome.message.expect("message after recovery");
            assert_eq!(message.envelope.text, "retry after listener starts");
        }
        other => panic!("unexpected read response: {other:?}"),
    }

    let recovered = status_cache.snapshot();
    assert_eq!(recovered.readiness, RuntimeReadinessState::Ready);
    assert!(!recovered.degraded_peer_listener);

    listener_transport
        .shutdown()
        .expect("shutdown peer listener");
}

#[test]
#[serial_test::serial(env)]
fn localhost_remote_target_notification_degradation_is_classified_without_failing_delivery() {
    local_peer_listener_harness_preserves_sent_outcome_when_post_send_degrades();
}

#[test]
#[serial_test::serial(env)]
fn localhost_remote_target_retry_visible_recovery_remains_bounded_and_observable() {
    local_peer_listener_harness_recovers_after_transient_connect_failure_and_delivers_on_retry();
}

#[test]
#[serial_test::serial(env)]
fn local_peer_listener_harness_surfaces_exact_remote_allowlist_error_to_sender() {
    let _guard = install_shared_lifecycle_reset_guard();
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    let workspace_dir = tempdir.path().join("workspace");
    let db_path = tempdir.path().join("mail.db");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir");
    write_workspace_config(&workspace_dir);
    install_test_roster_with_harness(
        &db_path,
        &[
            (
                ROLE_TEAM_LEAD,
                RosterHarness::ClaudeCode,
                workspace_dir.as_path(),
            ),
            ("qa-a", RosterHarness::ClaudeCode, workspace_dir.as_path()),
        ],
    );
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD, "qa-a"]);

    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");
    let status_cache = RuntimeStatusCache::new();
    let listener_transport = PeerTransportRuntime::new_server_for_test_with_allowed_host_store(
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
        status_cache.clone(),
        assembly.allowed_host_store_arc(),
    );
    let dispatcher = Arc::new(DaemonRequestDispatcher::new_for_test_with_peer_transport(
        atm_home.clone(),
        status_cache,
        db_path.clone(),
        listener_transport.clone(),
    ));
    listener_transport
        .start(dispatcher)
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
        .send(RequestEnvelope::Send(SendRequestEnvelope::Compose(
            SendRequest::new(
                atm_home,
                workspace_dir,
                ROLE_TEAM_LEAD.parse().expect("caller"),
                "qa-a@test-team",
                test_team_name(),
                SendMessageSource::Inline("unauthorized peer-listener hello".to_string()),
                None,
                true,
                None,
                false,
            )
            .expect("send request"),
        )))
        .expect_err("unauthorized send should surface the remote allowlist error");
    assert_eq!(error.code, AtmErrorCode::MessageValidationFailed);
    assert!(
        error
            .message
            .contains("no enabled daemon host row authorizes it"),
        "unexpected error message: {}",
        error.message
    );
    assert!(
        error
            .primary_recovery()
            .is_some_and(|recovery| recovery.contains("atm daemon hosts allow 127.0.0.1")),
        "unexpected recovery: {:?}",
        error.primary_recovery()
    );

    listener_transport
        .shutdown()
        .expect("shutdown peer listener");
}
