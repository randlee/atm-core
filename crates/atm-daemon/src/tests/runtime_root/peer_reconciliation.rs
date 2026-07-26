use super::*;

struct DelayedPeerHttpsDelivery {
    observed_budgets: std::sync::Mutex<Vec<Duration>>,
    entered_tx: std::sync::Mutex<Option<mpsc::SyncSender<()>>>,
    release_rx: std::sync::Mutex<Option<mpsc::Receiver<()>>>,
}

struct DelayedPeerControl {
    entered_rx: mpsc::Receiver<()>,
    release_tx: mpsc::SyncSender<()>,
}

impl DelayedPeerHttpsDelivery {
    fn blocking() -> (Self, DelayedPeerControl) {
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        (
            Self {
                observed_budgets: std::sync::Mutex::new(Vec::new()),
                entered_tx: std::sync::Mutex::new(Some(entered_tx)),
                release_rx: std::sync::Mutex::new(Some(release_rx)),
            },
            DelayedPeerControl {
                entered_rx,
                release_tx,
            },
        )
    }
}

impl HttpsMessageTransport for DelayedPeerHttpsDelivery {
    fn deliver(
        &self,
        _request: WriteRequest,
        _peer: &TrustedPeer,
        deadline: RequestDeadline,
    ) -> Result<ResponseEnvelope, AtmError> {
        self.observed_budgets
            .lock()
            .expect("peer deadline recording lock")
            .push(
                deadline
                    .remaining()
                    .expect("shared budget remains briefly live"),
            );
        let entered_tx = self
            .entered_tx
            .lock()
            .expect("delayed peer entry signal lock")
            .take()
            .expect("delayed peer must be entered once");
        entered_tx
            .send(())
            .expect("test observes delayed peer delivery start");
        let release_rx = self
            .release_rx
            .lock()
            .expect("delayed peer release signal lock")
            .take()
            .expect("delayed peer must be released once");
        release_rx
            .recv()
            .expect("test releases delayed peer delivery");
        Err(AtmError::daemon_unavailable(
            "simulated delayed peer disconnect",
        ))
    }
}

#[test]
#[serial_test::serial(env)]
fn three_second_local_write_passes_only_its_remaining_budget_to_a_delayed_peer() {
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
    open_sqlite_boundary(&db_path)
        .expect("sqlite boundary")
        .peer_config_store()
        .save_trusted_peer(&TrustedPeer {
            host: "peer.example.test".parse().expect("peer host"),
            fingerprint: "sha256:test-peer".parse().expect("fingerprint"),
            enabled: true,
            https_port: NonZeroU16::new(43101).expect("non-zero"),
        })
        .expect("save trusted peer");
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), RuntimeStatusCache::new(), db_path);
    let (delayed_transport, control) = DelayedPeerHttpsDelivery::blocking();
    let transport = Arc::new(delayed_transport);
    dispatcher
        .install_https_transport(transport.clone())
        .expect("install delayed peer transport");
    let request = ApiRequest::new(RequestEnvelope::Write(Box::new(
        SendRequest::new(
            atm_home,
            workspace_dir,
            ROLE_TEAM_LEAD.parse().expect("caller"),
            "remote-agent@remote-team.peer.example.test",
            TEST_TEAM.parse().expect("team"),
            SendMessageSource::Inline("deadline budget".to_string()),
            None,
            false,
            None,
            false,
        )
        .expect("remote write request"),
    )));
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let route_thread = std::thread::spawn(move || {
        let result = ApiRouter::route(
            &dispatcher,
            request,
            AuthenticatedIngress::Local,
            RequestDeadline::after(Duration::from_secs(3)),
        );
        result_tx
            .send(result)
            .expect("report delayed peer route result");
    });
    control
        .entered_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("delayed peer delivery started");
    control
        .release_tx
        .send(())
        .expect("release delayed peer delivery");
    let error = result_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("delayed peer route finished")
        .expect_err("delayed peer disconnect is not remote acceptance");
    route_thread.join().expect("join delayed peer route");
    assert_eq!(error.code(), AtmErrorCode::RemoteDeliveryUnconfirmed);
    let budgets = transport.observed_budgets.lock().expect("recorded budgets");
    assert_eq!(budgets.len(), 1, "one foreground peer attempt");
    assert!(
        budgets[0] < Duration::from_secs(3),
        "the delayed peer receives the local request's remaining budget, never a fresh five seconds"
    );
}

#[test]
#[serial_test::serial(env)]
fn local_client_close_during_delayed_peer_delivery_never_reports_success_or_leaks_work() {
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
    open_sqlite_boundary(&db_path)
        .expect("sqlite boundary")
        .peer_config_store()
        .save_trusted_peer(&TrustedPeer {
            host: "peer.example.test".parse().expect("peer host"),
            fingerprint: "sha256:test-peer".parse().expect("fingerprint"),
            enabled: true,
            https_port: NonZeroU16::new(43101).expect("non-zero"),
        })
        .expect("save trusted peer");
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
    let dispatcher = Arc::new(DaemonRequestDispatcher::new_for_test(
        atm_home.clone(),
        RuntimeStatusCache::new(),
        db_path,
    ));
    let (delayed_transport, control) = DelayedPeerHttpsDelivery::blocking();
    dispatcher
        .install_https_transport(Arc::new(delayed_transport))
        .expect("install delayed peer transport");
    let dispatcher: Arc<dyn ApiRouter + Send + Sync> = dispatcher;
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
                            "cancellation test failed to observe daemon ready signal",
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
            atm_home,
            workspace_dir,
            ROLE_TEAM_LEAD.parse().expect("caller"),
            "remote-agent@remote-team.peer.example.test",
            TEST_TEAM.parse().expect("team"),
            SendMessageSource::Inline("cancelled peer write".to_string()),
            None,
            false,
            None,
            false,
        )
        .expect("remote write request"),
    ));
    write_test_local_ipc_request(&mut stream, &request).expect("write remote request");
    control
        .entered_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("peer delivery started before client cancellation");
    drop(stream);
    control
        .release_tx
        .send(())
        .expect("release delayed peer after client cancellation");
    lifecycle.set_terminate_for_test(true);
    serve_result_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("runtime joins cancelled request worker")
        .expect("cancelled request does not crash daemon runtime");
    join.join().expect("join runtime thread");
}

#[test]
#[serial_test::serial(env)]
fn failed_peer_ack_keeps_source_pending_until_the_shared_write_retries() {
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
        "local-recipient",
        &workspace_dir,
    );
    open_sqlite_boundary(&db_path)
        .expect("sqlite boundary")
        .peer_config_store()
        .save_trusted_peer(&TrustedPeer {
            host: "peer.example.test".parse().expect("peer host"),
            fingerprint: "sha256:test-peer".parse().expect("fingerprint"),
            enabled: true,
            https_port: std::num::NonZeroU16::new(43101).expect("non-zero"),
        })
        .expect("save trusted peer");

    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), RuntimeStatusCache::new(), db_path);
    let origin_message_id = atm_core::schema::AtmMessageId::new();
    let mut inbound = SendRequest::new(
        atm_home.clone(),
        workspace_dir.clone(),
        "remote-sender".parse().expect("peer sender"),
        "local-recipient@test-team",
        "remote-team".parse().expect("peer team"),
        SendMessageSource::Inline("please acknowledge".to_string()),
        None,
        true,
        None,
        false,
    )
    .expect("inbound peer request")
    .with_origin_metadata(origin_message_id, atm_core::types::IsoTimestamp::now());
    inbound.authenticated_source_host = Some("peer.example.test".parse().expect("peer host"));
    let inbound_response = ApiRouter::route(
        &dispatcher,
        ApiRequest::new(RequestEnvelope::Write(Box::new(inbound))),
        AuthenticatedIngress::Peer,
        RequestDeadline::after(Duration::from_secs(1)),
    )
    .expect("inbound peer write");
    let source_message_id = match inbound_response.into_inner() {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => outcome.message_id,
        other => panic!("expected inbound send response, got {other:?}"),
    };

    let ack = AckRequest {
        home_dir: atm_home,
        current_dir: workspace_dir,
        caller_identity: "local-recipient".parse().expect("recipient"),
        caller_chat_id: None,
        caller_team: TEST_TEAM.parse().expect("team"),
        message_id: source_message_id,
        reply_body: "acknowledged".to_string(),
    };
    let failing = Arc::new(FailingHttpsDelivery::default());
    dispatcher
        .install_https_transport(failing.clone())
        .expect("install failing transport");
    let error = dispatcher
        .dispatch(RequestEnvelope::Write(Box::new(
            ack.clone().into_write_request(),
        )))
        .expect_err("failed remote acknowledgement must return the transport error");
    assert_eq!(error.code(), AtmErrorCode::RemoteDeliveryUnconfirmed);
    assert_eq!(
        failing
            .attempted
            .lock()
            .expect("attempt recording lock")
            .len(),
        1,
        "ack is one shared peer write attempt"
    );

    let succeeding = Arc::new(RecordingHttpsDelivery::default());
    dispatcher
        .install_https_transport(succeeding.clone())
        .expect("replace test transport");
    let retry = dispatcher
        .dispatch(RequestEnvelope::Write(Box::new(ack.into_write_request())))
        .expect("source remains pending after failed peer acknowledgement");
    assert!(matches!(
        retry,
        ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(_))
    ));
    assert_eq!(
        succeeding
            .delivered
            .lock()
            .expect("delivery recording lock")
            .len(),
        1,
        "the successful retry follows the same write/router path"
    );
}

#[test]
#[serial_test::serial(env)]
fn explicit_peer_sync_resends_one_bounded_immutable_write() {
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
    let peer: atm_storage::HostName = "peer.example.test".parse().expect("peer host");
    let peer_store = open_sqlite_boundary(&db_path)
        .expect("sqlite boundary")
        .peer_config_store();
    peer_store
        .save_trusted_peer(&TrustedPeer {
            host: peer.clone(),
            fingerprint: "sha256:test-peer".parse().expect("fingerprint"),
            enabled: true,
            https_port: std::num::NonZeroU16::new(43101).expect("non-zero"),
        })
        .expect("save trusted peer");

    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), RuntimeStatusCache::new(), db_path);
    let transport = Arc::new(RecordingHttpsDelivery::default());
    dispatcher
        .install_https_transport(transport.clone())
        .expect("install test HTTPS delivery");
    for body in ["first", "second"] {
        dispatcher
            .dispatch(RequestEnvelope::Write(Box::new(
                SendRequest::new(
                    atm_home.clone(),
                    workspace_dir.clone(),
                    ROLE_TEAM_LEAD.parse().expect("caller"),
                    "remote-agent@remote-team.peer.example.test",
                    TEST_TEAM.parse().expect("team"),
                    SendMessageSource::Inline(body.to_string()),
                    None,
                    false,
                    None,
                    false,
                )
                .expect("remote write request"),
            )))
            .expect("initial peer write");
    }
    let disabled = dispatcher
        .dispatch(RequestEnvelope::PeerSync(PeerSyncRequest {
            peer: peer.clone(),
        }))
        .expect("disabled policy is a no-op");
    assert!(matches!(
        disabled,
        ResponseEnvelope::PeerSync(PeerSyncOutcome { delivered: 0, .. })
    ));
    assert_eq!(
        transport.delivered.lock().expect("deliveries").len(),
        2,
        "disabled policy never scans or delivers stored writes"
    );
    peer_store
        .save_peer_sync_policy(
            &peer,
            PeerSyncPolicy {
                max_message_age: Duration::from_secs(60),
                max_batch_messages: NonZeroU16::new(1).expect("non-zero cap"),
            },
        )
        .expect("enable one-message sync");

    let response = dispatcher
        .dispatch(RequestEnvelope::PeerSync(PeerSyncRequest {
            peer: peer.clone(),
        }))
        .expect("explicit peer sync");
    match response {
        ResponseEnvelope::PeerSync(PeerSyncOutcome {
            peer: returned_peer,
            delivered,
        }) => {
            assert_eq!(returned_peer, peer);
            assert_eq!(
                delivered, 1,
                "the explicit path honors the durable batch cap"
            );
        }
        other => panic!("expected peer-sync outcome, got {other:?}"),
    }
    let delivered = transport.delivered.lock().expect("deliveries");
    assert_eq!(
        delivered.len(),
        3,
        "two ordinary writes plus exactly one bounded replay are delivered"
    );
    assert_eq!(
        delivered[0].origin_message_id, delivered[2].origin_message_id,
        "reconciliation reuses the canonical immutable write and its original ULID"
    );
}
