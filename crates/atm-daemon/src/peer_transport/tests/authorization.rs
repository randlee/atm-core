use super::*;

#[test]
fn peer_listener_rejects_unauthorized_host_before_dispatch() {
    let _guard = install_shared_lifecycle_reset_guard();
    let assembly = open_sqlite_boundary(
        std::env::temp_dir().join(format!("atm-ag5-peer-auth-{}.db", std::process::id())),
    )
    .expect("backend");
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
    .expect("backend");
    assembly
        .allowed_host_store_arc()
        .allow_host(
            atm_storage::AllowHostCommand::new(
                "127.0.0.1",
                atm_core::test_support::TEST_SENDER_ADDRESS,
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
    let assembly = open_sqlite_boundary(tempdir.path().join("auth.db")).expect("backend");
    let allowed_host_store = assembly.allowed_host_store_arc();
    let allowed_host = "127.0.0.1"
        .parse::<atm_storage::AllowedHostName>()
        .expect("allowed host");
    allowed_host_store
        .allow_host(
            atm_storage::AllowHostCommand::new(
                "127.0.0.1",
                atm_core::test_support::TEST_SENDER_ADDRESS,
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
