use super::*;

#[test]
#[serial_test::serial(env)]
fn local_ipc_host_qualified_admission_returns_before_blocked_peer_delivery() {
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
    let peer_host: atm_storage::HostName = "peer.example.test".parse().expect("peer host");
    let peer_store = open_sqlite_boundary(&db_path)
        .expect("sqlite boundary")
        .peer_config_store();
    peer_store
        .save_trusted_peer(&TrustedPeer {
            host: peer_host.clone(),
            fingerprint: "sha256:test-peer".parse().expect("fingerprint"),
            enabled: true,
            https_port: NonZeroU16::new(43101).expect("non-zero port"),
        })
        .expect("save trusted peer");
    peer_store
        .save_peer_sync_policy(
            &peer_host,
            PeerSyncPolicy {
                max_message_age: Duration::from_secs(60),
                max_batch_messages: NonZeroU16::new(1).expect("non-zero batch"),
            },
        )
        .expect("enable peer recovery");

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
    let socket_path = atm_core::home::host_runtime_dir_from_home(&atm_home)
        .join(atm_core::home::HOST_RUNTIME_SOCKET_FILE);
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
    let dispatcher = Arc::new(DaemonRequestDispatcher::new_for_test(
        atm_home.clone(),
        RuntimeStatusCache::new(),
        db_path.clone(),
    ));
    let (started, started_rx) = mpsc::sync_channel(1);
    let (release, release_rx) = mpsc::sync_channel(1);
    dispatcher
        .install_https_transport(Arc::new(BlockingPeerDelivery {
            started,
            release: std::sync::Mutex::new(release_rx),
        }))
        .expect("install blocked peer transport");
    dispatcher
        .start_peer_drain_coordinator()
        .expect("start peer drain coordinator");
    let router: Arc<dyn ApiRouter + Send + Sync> = dispatcher.clone();
    let (serve_result_tx, serve_result_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);

    let join = std::thread::spawn(move || {
        let result = runtime.serve_with_runtime_hooks(
            router,
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

    drop(connect_daemon_local_ipc_until_ready(&socket_path, ready_rx));
    #[cfg(windows)]
    let endpoint = atm_daemon_client::resolve_daemon_local_ipc_endpoint()
        .expect("windows local HTTP endpoint");
    #[cfg(not(windows))]
    let endpoint = atm_daemon_client::DaemonLocalIpcEndpoint::new(
        atm_core::local_http::local_http_record_path(&atm_home),
    )
    .expect("local HTTP endpoint record");
    let request = SendRequest::new(
        atm_home.clone(),
        workspace_dir.clone(),
        ROLE_TEAM_LEAD.parse().expect("caller"),
        "remote-agent@remote-team.peer.example.test",
        TEST_TEAM.parse().expect("team"),
        SendMessageSource::Inline("hello local ipc ack-required".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("send request");
    let request = RequestEnvelope::Write(Box::new(request));
    let mut verified = atm_daemon_client::verify_connection_compatibility(
        &endpoint,
        atm_daemon_client::CompatibilityPreflight {
            client_release: atm_daemon_client::ReleaseVersion::current(),
            cli_schema_version: atm_core::protocol::CLI_SCHEMA_VERSION,
            http_api_version: atm_core::protocol::HttpApiVersion::current(),
        },
        Duration::from_secs(3),
    )
    .expect("preflight compatible");
    let response = verified
        .dispatch_write(&endpoint, request, Duration::from_secs(3))
        .expect("dispatch write");
    match response {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => {
            assert_eq!(outcome.outcome.as_str(), "sent");
            assert!(!outcome.requires_ack);
        }
        other => panic!("unexpected response: {other:?}"),
    }
    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("post-commit worker must begin the blocked peer delivery");
    release.send(()).expect("release peer worker");

    lifecycle.set_terminate_for_test(true);
    serve_result_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("recv serve result")
        .expect("serve runtime result");
    join.join().expect("join serve thread");
    dispatcher
        .stop_peer_drain_coordinator()
        .expect("stop peer drain coordinator");
}

#[test]
#[serial_test::serial(env)]
fn local_ipc_real_daemon_admission_burst_accepts_each_public_write() {
    const BURST_SIZE: usize = 16;

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
    let mut runtime = server_transport
        .prepare_runtime_at_socket_path_for_home(socket_path.clone(), &atm_home)
        .expect("prepare runtime");
    let endpoint_guard = runtime.take_endpoint_guard().expect("take endpoint guard");
    let (lifecycle, _reset) = {
        let lifecycle = LifecycleControlSourceAdapter::install().expect("install lifecycle");
        let reset = LifecycleFlagResetGuard::install(lifecycle.clone());
        (lifecycle, reset)
    };
    let dispatcher: Arc<dyn ApiRouter + Send + Sync> = Arc::new(
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), RuntimeStatusCache::new(), db_path),
    );
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
                            "admission burst test failed to observe daemon ready",
                        )
                    })
                },
            },
        );
        serve_result_tx.send(result).expect("send serve result");
    });

    drop(connect_daemon_local_ipc_until_ready(&socket_path, ready_rx));
    let joins: Vec<_> = (0..BURST_SIZE)
        .map(|sequence| {
            let atm_home = atm_home.clone();
            let workspace_dir = workspace_dir.clone();
            let socket_path = socket_path.clone();
            std::thread::spawn(move || {
                let request = RequestEnvelope::Write(Box::new(
                    SendRequest::new(
                        atm_home,
                        workspace_dir,
                        ROLE_TEAM_LEAD.parse().expect("caller"),
                        "qa-a@test-team",
                        TEST_TEAM.parse().expect("team"),
                        SendMessageSource::Inline(format!("real local admission burst {sequence}")),
                        None,
                        false,
                        None,
                        false,
                    )
                    .expect("send request"),
                ));
                #[cfg(windows)]
                let mut stream = match atm_daemon_client::try_connect(
                    &atm_daemon_client::resolve_daemon_local_ipc_endpoint()
                        .expect("windows local HTTP endpoint"),
                )
                .expect("connect public local TCP endpoint")
                {
                    atm_daemon_client::LocalDaemonConnection::TcpLoopback(stream) => stream,
                };
                #[cfg(not(windows))]
                let mut stream = {
                    use interprocess::local_socket::Stream as LocalSocketStream;
                    use interprocess::local_socket::traits::Stream as _;

                    let name = atm_core::protocol::daemon_local_ipc_name_from_path(&socket_path)
                        .expect("local IPC name")
                        .into_owned();
                    LocalSocketStream::connect(name).expect("connect public local UDS endpoint")
                };
                configure_test_local_ipc_timeouts(&stream);
                write_test_local_ipc_request(&mut stream, &request)
                    .expect("write public local admission");
                atm_core::api::read_http_response(&mut stream, &request)
                    .expect("read public local admission")
            })
        })
        .collect();
    for join in joins {
        assert!(matches!(
            join.join().expect("join admission burst client"),
            ResponseEnvelope::Send(SendResponseEnvelope::Sent(_))
        ));
    }

    lifecycle.set_terminate_for_test(true);
    serve_result_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("receive serve result")
        .expect("serve runtime result");
    join.join().expect("join serve thread");
}
