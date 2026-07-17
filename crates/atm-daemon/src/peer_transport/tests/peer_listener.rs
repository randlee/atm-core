use super::*;

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
        PeerTransportConfig {
            remote_retry_budget: Duration::from_secs(1),
            peer_listen_addr: None,
        },
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
fn secure_peer_listener_round_trips_one_doctor_request() {
    let _guard = install_shared_lifecycle_reset_guard();
    let assembly = open_sqlite_boundary(std::env::temp_dir().join(format!(
        "atm-ag10-secure-roundtrip-{}.db",
        std::process::id()
    )))
    .expect("assembly");
    assembly
        .allowed_host_store_arc()
        .allow_host(
            atm_storage::AllowHostCommand::new(
                "127.0.0.1",
                format!(
                    "{}@{}",
                    atm_core::test_support::TEST_SENDER,
                    atm_core::test_support::TEST_TEAM
                ),
                Some("loopback".to_string()),
            )
            .expect("allow host command"),
        )
        .expect("allow host");
    let peer_security_store = assembly.peer_security_store_arc();
    configure_secure_mode_and_trust_self(&peer_security_store, "127.0.0.1");
    let listener_transport =
        PeerTransportRuntime::new_server_for_test_with_security_and_allowed_host_store(
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
            RuntimeStatusCache::new(),
            assembly.allowed_host_store_arc(),
            peer_security_store.clone(),
        );
    listener_transport
        .start(Arc::new(DoctorOnlyDispatcher))
        .expect("start secure peer listener");
    let endpoint = listener_transport
        .bound_addr_for_test()
        .expect("bound peer listener addr");

    let tempdir = TempDir::new().expect("tempdir");
    let client_transport = PeerTransportRuntime::new_for_test_with_security_store(
        endpoint,
        PeerTransportConfig::default(),
        tempdir.path().join("replay.db"),
        peer_security_store,
    );
    let response = client_transport
        .client_transport()
        .send(RequestEnvelope::Doctor(atm_core::doctor::DoctorQuery {
            home_dir: tempdir.path().to_path_buf(),
            current_dir: tempdir.path().to_path_buf(),
            team_override: None,
            ..atm_core::doctor::DoctorQuery::default()
        }))
        .expect("secure peer listener doctor response");
    assert!(matches!(response, ResponseEnvelope::Doctor(_)));

    listener_transport
        .shutdown()
        .expect("shutdown secure peer listener");
}

#[test]
fn secure_peer_listener_rejects_untrusted_client_before_dispatch() {
    let _guard = install_shared_lifecycle_reset_guard();
    let server_assembly = open_sqlite_boundary(
        std::env::temp_dir().join(format!("atm-ag10-secure-server-{}.db", std::process::id())),
    )
    .expect("server assembly");
    server_assembly
        .allowed_host_store_arc()
        .allow_host(
            atm_storage::AllowHostCommand::new(
                "127.0.0.1",
                format!(
                    "{}@{}",
                    atm_core::test_support::TEST_SENDER,
                    atm_core::test_support::TEST_TEAM
                ),
                Some("loopback".to_string()),
            )
            .expect("allow host command"),
        )
        .expect("allow host");
    let server_security_store = server_assembly.peer_security_store_arc();
    server_security_store
        .set_security_mode(
            SetPeerSecurityModeCommand::new(
                PeerSecurityMode::SecureRequired,
                format!(
                    "{}@{}",
                    atm_core::test_support::TEST_SENDER,
                    atm_core::test_support::TEST_TEAM
                ),
            )
            .expect("security mode command"),
        )
        .expect("set security mode");
    server_security_store
        .load_or_create_local_identity()
        .expect("server identity");

    let listener_transport =
        PeerTransportRuntime::new_server_for_test_with_security_and_allowed_host_store(
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
            RuntimeStatusCache::new(),
            server_assembly.allowed_host_store_arc(),
            server_security_store,
        );
    let dispatcher = Arc::new(CountingDispatcher::default());
    listener_transport
        .start(dispatcher.clone())
        .expect("start secure peer listener");
    let endpoint = listener_transport
        .bound_addr_for_test()
        .expect("bound peer listener addr");

    let client_assembly = open_sqlite_boundary(
        std::env::temp_dir().join(format!("atm-ag10-secure-client-{}.db", std::process::id())),
    )
    .expect("client assembly");
    let client_security_store = client_assembly.peer_security_store_arc();
    configure_secure_mode_and_trust_self(&client_security_store, "127.0.0.1");
    let tempdir = TempDir::new().expect("tempdir");
    let client_transport = PeerTransportRuntime::new_for_test_with_security_store(
        endpoint,
        PeerTransportConfig::default(),
        tempdir.path().join("replay.db"),
        client_security_store,
    );
    let error = client_transport
        .client_transport()
        .send(RequestEnvelope::Doctor(DoctorQuery {
            home_dir: tempdir.path().to_path_buf(),
            current_dir: tempdir.path().to_path_buf(),
            team_override: None,
            ..DoctorQuery::default()
        }))
        .expect_err("untrusted secure client should be rejected");
    assert_eq!(error.code, AtmErrorCode::DaemonUnavailable);
    assert_eq!(dispatcher.count.load(Ordering::SeqCst), 0);

    listener_transport
        .shutdown()
        .expect("shutdown secure peer listener");
}

#[test]
fn secure_client_does_not_silently_fallback_when_server_fingerprint_mismatches() {
    let _guard = install_shared_lifecycle_reset_guard();
    let server_assembly = open_sqlite_boundary(std::env::temp_dir().join(format!(
        "atm-ag10-fingerprint-mismatch-{}.db",
        std::process::id()
    )))
    .expect("server assembly");
    server_assembly
        .allowed_host_store_arc()
        .allow_host(
            atm_storage::AllowHostCommand::new(
                "127.0.0.1",
                format!(
                    "{}@{}",
                    atm_core::test_support::TEST_SENDER,
                    atm_core::test_support::TEST_TEAM
                ),
                Some("loopback".to_string()),
            )
            .expect("allow host command"),
        )
        .expect("allow host");
    let server_security_store = server_assembly.peer_security_store_arc();
    configure_secure_mode_and_trust_self(&server_security_store, "127.0.0.1");
    let listener_transport =
        PeerTransportRuntime::new_server_for_test_with_security_and_allowed_host_store(
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            SubsystemObservability::disabled(DaemonSubsystem::PeerTransport),
            RuntimeStatusCache::new(),
            server_assembly.allowed_host_store_arc(),
            server_security_store,
        );
    let dispatcher = Arc::new(CountingDispatcher::default());
    listener_transport
        .start(dispatcher.clone())
        .expect("start secure peer listener");
    let endpoint = listener_transport
        .bound_addr_for_test()
        .expect("bound peer listener addr");

    let client_assembly = open_sqlite_boundary(std::env::temp_dir().join(format!(
        "atm-ag10-fingerprint-mismatch-client-{}.db",
        std::process::id()
    )))
    .expect("client assembly");
    let client_security_store = client_assembly.peer_security_store_arc();
    client_security_store
        .set_security_mode(
            SetPeerSecurityModeCommand::new(
                PeerSecurityMode::SecureRequired,
                format!(
                    "{}@{}",
                    atm_core::test_support::TEST_SENDER,
                    atm_core::test_support::TEST_TEAM
                ),
            )
            .expect("security mode command"),
        )
        .expect("set security mode");
    client_security_store
        .load_or_create_local_identity()
        .expect("client identity");
    client_security_store
        .upsert_trusted_peer(
            UpsertTrustedPeerCommand::new(
                "127.0.0.1",
                "00".repeat(32),
                Some("wrong".to_string()),
                format!(
                    "{}@{}",
                    atm_core::test_support::TEST_SENDER,
                    atm_core::test_support::TEST_TEAM
                ),
            )
            .expect("trusted peer command"),
        )
        .expect("approve wrong trusted peer");
    let tempdir = TempDir::new().expect("tempdir");
    let client_transport = PeerTransportRuntime::new_for_test_with_security_store(
        endpoint,
        PeerTransportConfig::default(),
        tempdir.path().join("replay.db"),
        client_security_store,
    );
    let error = client_transport
        .client_transport()
        .send(RequestEnvelope::Doctor(DoctorQuery {
            home_dir: tempdir.path().to_path_buf(),
            current_dir: tempdir.path().to_path_buf(),
            team_override: None,
            ..DoctorQuery::default()
        }))
        .expect_err("fingerprint mismatch should fail");
    assert_eq!(error.code, AtmErrorCode::MessageValidationFailed);
    assert_eq!(dispatcher.count.load(Ordering::SeqCst), 0);

    listener_transport
        .shutdown()
        .expect("shutdown secure peer listener");
}
