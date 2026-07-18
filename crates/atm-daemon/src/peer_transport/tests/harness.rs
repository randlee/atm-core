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
            Box::new(
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
            ),
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
            Box::new(
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
            ),
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
            Box::new(
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
            ),
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
            Box::new(
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
            ),
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
                Some("localhost notification classification".to_string()),
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
        .start(dispatcher.clone())
        .expect("start peer listener");

    let mut request = SendRequest::new(
        atm_home.clone(),
        workspace_dir.clone(),
        ROLE_TEAM_LEAD.parse().expect("caller"),
        "qa-a@test-team",
        test_team_name(),
        SendMessageSource::Inline("hello degraded localhost remote target".to_string()),
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
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(
            Box::new(request),
        )))
        .expect("remote-target send should still succeed");
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

    let read_response = dispatcher
        .dispatch(RequestEnvelope::Receive(
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
        .expect("receiver read should succeed");
    match read_response {
        ResponseEnvelope::Receive(outcome) => {
            let message = outcome.message.expect("delivered message");
            assert_eq!(
                message.envelope.text,
                "hello degraded localhost remote target"
            );
        }
        other => panic!("unexpected receiver read response: {other:?}"),
    }

    listener_transport
        .shutdown()
        .expect("shutdown peer listener");
}

#[test]
#[serial_test::serial(env)]
fn localhost_remote_target_retry_visible_recovery_remains_bounded_and_observable() {
    let _guard = install_shared_lifecycle_reset_guard();
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    let workspace_dir = tempdir.path().join("workspace");
    let db_path = tempdir.path().join("mail.db");
    let _env = atm_core::test_support::EnvGuard::set_many([(
        atm_runtime_test_support::SQLITE_RUNTIME_PATH_ENV,
        Some(db_path.to_str().expect("utf8 sqlite db path")),
    )]);
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

    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");
    assembly
        .allowed_host_store
        .allow_host(
            atm_storage::AllowHostCommand::new(
                "127.0.0.1",
                test_sender_identity(),
                Some("localhost retry visible recovery".to_string()),
            )
            .expect("allow host command"),
        )
        .expect("allow host");
    assembly
        .peer_interface_config_store
        .add_interface(
            atm_storage::AddPeerInterfaceCommand::new(
                "lo0",
                Ipv4Addr::LOCALHOST.into(),
                Ipv4Addr::LOCALHOST.into(),
                endpoint.port(),
                atm_storage::PeerInterfaceKind::Loopback,
                test_sender_identity(),
            )
            .expect("add interface"),
        )
        .expect("store interface");
    assembly
        .peer_interface_config_store
        .set_interface_enabled(
            &atm_storage::PeerInterfaceKey::new("lo0", Ipv4Addr::LOCALHOST.into(), endpoint.port())
                .expect("interface key"),
            true,
        )
        .expect("enable interface");

    let sender_status_cache = RuntimeStatusCache::new();
    let sender_transport = PeerTransportRuntime::new_with_observability(
        Some(assembly.remote_replay_store.clone()),
        Some(assembly.allowed_host_store_arc()),
        None,
        PeerTransportConfig::default(),
        SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
        sender_status_cache.clone(),
    );
    let replay_worker = sender_transport
        .start_replay_resume_worker()
        .expect("start replay worker");
    let sender_dispatcher = DaemonRequestDispatcher::new_for_test_with_peer_transport(
        atm_home.clone(),
        sender_status_cache,
        db_path.clone(),
        sender_transport.clone(),
    );

    let mut request = SendRequest::new(
        atm_home.clone(),
        workspace_dir.clone(),
        ROLE_TEAM_LEAD.parse().expect("caller"),
        "qa-a@test-team",
        test_team_name(),
        SendMessageSource::Inline("retry after remote-target listener starts".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("send request");
    request.remote_host = atm_core::send::parse_send_target("qa-a@test-team.localhost", None)
        .expect("parse target")
        .remote_host;

    let deferred = sender_dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(
            Box::new(request),
        )))
        .expect("initial remote-target send should defer");
    let receipt_message_id = match deferred {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => {
            assert_eq!(outcome.outcome.as_str(), "deferred");
            assert_eq!(outcome.receipt_message_id, Some(outcome.message_id));
            assert!(
                outcome
                    .summary
                    .as_deref()
                    .is_some_and(|summary| summary.contains("deferred remote delivery"))
            );
            outcome.message_id
        }
        other => panic!("unexpected deferred response: {other:?}"),
    };

    let sender_receipt = sender_dispatcher
        .dispatch(RequestEnvelope::Receive(
            ReadQuery::new(
                atm_home.clone(),
                workspace_dir.clone(),
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
        .expect("sender receipt read should succeed");
    match sender_receipt {
        ResponseEnvelope::Receive(outcome) => {
            let message = outcome.message.expect("deferred receipt");
            assert_eq!(message.envelope.message_id, Some(receipt_message_id));
            assert!(message.envelope.text.contains(
                "deferred remote delivery because the cross-host path is not currently healthy"
            ));
        }
        other => panic!("unexpected sender receipt response: {other:?}"),
    }

    let listener_status_cache = RuntimeStatusCache::new();
    let listener_transport = PeerTransportRuntime::new_server_for_test_with_allowed_host_store(
        endpoint,
        SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
        listener_status_cache.clone(),
        assembly.allowed_host_store_arc(),
    );
    let listener_dispatcher = Arc::new(DaemonRequestDispatcher::new_for_test_with_peer_transport(
        atm_home.clone(),
        listener_status_cache,
        db_path.clone(),
        listener_transport.clone(),
    ));
    listener_transport
        .start(listener_dispatcher.clone())
        .expect("start peer listener");

    let started = std::time::Instant::now();
    loop {
        let receiver_read = listener_dispatcher
            .dispatch(RequestEnvelope::Receive(
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
            .expect("receiver read after replay should succeed");
        let receiver_delivered = match receiver_read {
            ResponseEnvelope::Receive(outcome) => outcome.message.is_some_and(|message| {
                message.envelope.text == "retry after remote-target listener starts"
            }),
            other => panic!("unexpected receiver replay response: {other:?}"),
        };

        let sender_recovery = sender_dispatcher
            .dispatch(RequestEnvelope::Receive(
                ReadQuery::new(
                    atm_home.clone(),
                    workspace_dir.clone(),
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
            .expect("sender recovery receipt should succeed");
        let receipt_delivered = match sender_recovery {
            ResponseEnvelope::Receive(outcome) => outcome.message.is_some_and(|message| {
                message.envelope.message_id == Some(receipt_message_id)
                    && message
                        .envelope
                        .text
                        .contains("delivered the deferred remote message")
            }),
            other => panic!("unexpected sender recovery response: {other:?}"),
        };

        if receiver_delivered && receipt_delivered {
            break;
        }
        // The replay worker waits up to one poll interval before sweeping, then the
        // client retry path can wait one full initial backoff window before the
        // listener accepts the deferred send under CI load. Keep the harness bound
        // above that 5s + 5s schedule and pace the observation loop so it does not
        // starve the background replay worker on oversubscribed runners.
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "background replay worker did not deliver the deferred message within the bounded window"
        );
        std::thread::park_timeout(Duration::from_millis(25));
    }

    listener_transport
        .shutdown()
        .expect("shutdown peer listener");
    let _ = replay_worker.stop_tx.send(());
    replay_worker
        .join_handle
        .join()
        .expect("join replay worker");
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
            Box::new(
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
            ),
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
