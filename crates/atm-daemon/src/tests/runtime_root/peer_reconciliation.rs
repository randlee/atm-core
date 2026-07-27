use super::*;
use atm_core::ack::AckRequest;
use atm_storage::MessageQuery;

struct BlockingPeerDelivery {
    blocked_peer: atm_storage::HostName,
    started: mpsc::SyncSender<()>,
    release: std::sync::Mutex<mpsc::Receiver<()>>,
}

impl HttpsMessageTransport for BlockingPeerDelivery {
    fn deliver(
        &self,
        _request: WriteRequest,
        peer: &TrustedPeer,
        _deadline: RequestDeadline,
    ) -> Result<ResponseEnvelope, AtmError> {
        if peer.host == self.blocked_peer {
            self.started.send(()).expect("report blocked peer");
            self.release
                .lock()
                .expect("release lock")
                .recv()
                .expect("release blocked peer");
        }
        Ok(ResponseEnvelope::CompatibilityVerdict(
            atm_core::protocol::CompatibilityVerdict::Compatible {
                daemon_release: atm_core::protocol::ReleaseVersion::current(),
                daemon_schema_version: atm_core::protocol::CLI_SCHEMA_VERSION,
                daemon_http_api_version: atm_core::protocol::HttpApiVersion::current(),
            },
        ))
    }
}

struct ReconciliationReceiverDelivery {
    receiver: Arc<DaemonRequestDispatcher>,
    source_host: atm_storage::HostName,
    deliveries: std::sync::Mutex<usize>,
}

impl HttpsMessageTransport for ReconciliationReceiverDelivery {
    fn deliver(
        &self,
        mut request: WriteRequest,
        _peer: &TrustedPeer,
        deadline: RequestDeadline,
    ) -> Result<ResponseEnvelope, AtmError> {
        request.authenticated_source_host = Some(self.source_host.clone());
        *self.deliveries.lock().expect("delivery count lock") += 1;
        ApiRouter::route(
            self.receiver.as_ref(),
            ApiRequest::new(RequestEnvelope::Write(Box::new(request))),
            AuthenticatedIngress::Peer,
            deadline,
        )
        .map(|response| response.into_inner())
    }
}

fn recipient_message_count(db_path: &std::path::Path, recipient: &str) -> usize {
    open_sqlite_boundary(db_path)
        .expect("sqlite boundary")
        .message_store_arc()
        .list_messages(&MessageQuery {
            team: TEST_TEAM.parse().expect("team"),
            agent: recipient.parse().expect("recipient"),
            sender: None,
            task_id: None,
            limit: None,
        })
        .expect("list recipient inbox")
        .len()
}

#[test]
#[serial_test::serial(env)]
fn response_write_failure_keeps_source_pending_until_the_shared_write_retries() {
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
    let failing = Arc::new(ResponseWriteFailure::default());
    dispatcher
        .install_https_transport(failing.clone())
        .expect("install failing transport");
    let error = dispatcher
        .dispatch(RequestEnvelope::Write(Box::new(
            ack.clone().into_write_request(),
        )))
        .expect_err("failed remote acknowledgement must remain unconfirmed");
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
            disposition,
        }) => {
            assert_eq!(returned_peer, peer);
            assert_eq!(disposition, PeerSyncDisposition::Completed);
            assert_eq!(
                delivered, 2,
                "the coordinator drains all bounded pages before releasing its lease"
            );
        }
        other => panic!("expected peer-sync outcome, got {other:?}"),
    }
    let delivered = transport.delivered.lock().expect("deliveries");
    assert_eq!(
        delivered.len(),
        4,
        "two ordinary writes plus both ordered recovery pages are delivered"
    );
    assert_eq!(
        delivered[0].origin_message_id, delivered[2].origin_message_id,
        "reconciliation reuses the canonical immutable write and its original ULID"
    );
}

#[test]
#[serial_test::serial(env)]
fn reconciliation_duplicate_arrival_keeps_receiver_inbox_idempotent() {
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let source_home = tempdir.path().join("source-home");
    let source_workspace = tempdir.path().join("source-workspace");
    let source_db = tempdir.path().join("source.db");
    let receiver_home = tempdir.path().join("receiver-home");
    let receiver_workspace = tempdir.path().join("receiver-workspace");
    let receiver_db = tempdir.path().join("receiver.db");
    for directory in [
        &source_home,
        &source_workspace,
        &receiver_home,
        &receiver_workspace,
    ] {
        std::fs::create_dir_all(directory).expect("test directory");
    }
    write_team_config(&source_home, &[]);
    write_team_config(&receiver_home, &[]);
    add_member_via_retained_admin(
        &source_db,
        &source_home,
        TEST_TEAM,
        ROLE_TEAM_LEAD,
        &source_workspace,
    );
    add_member_via_retained_admin(
        &receiver_db,
        &receiver_home,
        TEST_TEAM,
        "receiver",
        &receiver_workspace,
    );

    let receiver_host: atm_storage::HostName =
        "receiver.example.test".parse().expect("receiver host");
    let source_host: atm_storage::HostName = "source.example.test".parse().expect("source host");
    let source_peers = open_sqlite_boundary(&source_db)
        .expect("source sqlite boundary")
        .peer_config_store();
    source_peers
        .save_trusted_peer(&TrustedPeer {
            host: receiver_host.clone(),
            fingerprint: "sha256:receiver".parse().expect("fingerprint"),
            enabled: true,
            https_port: std::num::NonZeroU16::new(43101).expect("port"),
        })
        .expect("save receiver peer");
    source_peers
        .save_peer_sync_policy(
            &receiver_host,
            PeerSyncPolicy {
                max_message_age: Duration::from_secs(60),
                max_batch_messages: std::num::NonZeroU16::new(8).expect("batch cap"),
            },
        )
        .expect("enable reconciliation");
    open_sqlite_boundary(&receiver_db)
        .expect("receiver sqlite boundary")
        .peer_config_store()
        .save_trusted_peer(&TrustedPeer {
            host: source_host.clone(),
            fingerprint: "sha256:source".parse().expect("fingerprint"),
            enabled: true,
            https_port: std::num::NonZeroU16::new(43101).expect("port"),
        })
        .expect("save source peer");

    let receiver = Arc::new(DaemonRequestDispatcher::new_for_test(
        receiver_home.clone(),
        RuntimeStatusCache::new(),
        receiver_db.clone(),
    ));
    let source = DaemonRequestDispatcher::new_for_test(
        source_home.clone(),
        RuntimeStatusCache::new(),
        source_db,
    );
    let delivery = Arc::new(ReconciliationReceiverDelivery {
        receiver,
        source_host,
        deliveries: std::sync::Mutex::new(0),
    });
    source
        .install_https_transport(delivery.clone())
        .expect("install receiver transport");

    source
        .dispatch(RequestEnvelope::Write(Box::new(
            SendRequest::new(
                source_home,
                source_workspace,
                ROLE_TEAM_LEAD.parse().expect("sender"),
                "receiver@test-team.receiver.example.test",
                TEST_TEAM.parse().expect("team"),
                SendMessageSource::Inline("reconcile once".to_string()),
                None,
                true,
                None,
                false,
            )
            .expect("peer write"),
        )))
        .expect("initial reconciliation delivery");
    assert_eq!(recipient_message_count(&receiver_db, "receiver"), 1);

    let sync = source
        .dispatch(RequestEnvelope::PeerSync(PeerSyncRequest {
            peer: receiver_host,
        }))
        .expect("repeat reconciliation delivery");
    assert!(matches!(sync, ResponseEnvelope::PeerSync(_)));
    assert_eq!(
        *delivery.deliveries.lock().expect("delivery count lock"),
        2,
        "the receiver sees the original delivery and its reconciliation duplicate"
    );
    assert_eq!(
        recipient_message_count(&receiver_db, "receiver"),
        1,
        "the receiving daemon retains one immutable inbox row for an identical reconciliation duplicate"
    );
}

#[test]
#[serial_test::serial(env)]
fn peer_sync_for_one_peer_does_not_hold_the_progress_map_during_another_delivery() {
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
    let peer_a: atm_storage::HostName = "peer-a.example.test".parse().expect("peer A");
    let peer_b: atm_storage::HostName = "peer-b.example.test".parse().expect("peer B");
    let peer_store = open_sqlite_boundary(&db_path)
        .expect("sqlite boundary")
        .peer_config_store();
    for peer in [&peer_a, &peer_b] {
        peer_store
            .save_trusted_peer(&TrustedPeer {
                host: peer.clone(),
                fingerprint: "sha256:test-peer".parse().expect("fingerprint"),
                enabled: true,
                https_port: NonZeroU16::new(43101).expect("port"),
            })
            .expect("save trusted peer");
        peer_store
            .save_peer_sync_policy(
                peer,
                PeerSyncPolicy {
                    max_message_age: Duration::from_secs(60),
                    max_batch_messages: NonZeroU16::new(1).expect("batch cap"),
                },
            )
            .expect("save sync policy");
    }
    let dispatcher = Arc::new(DaemonRequestDispatcher::new_for_test(
        atm_home.clone(),
        RuntimeStatusCache::new(),
        db_path,
    ));
    dispatcher
        .install_https_transport(Arc::new(RecordingHttpsDelivery::default()))
        .expect("install recording delivery");
    for (peer, body) in [(&peer_a, "for peer A"), (&peer_b, "for peer B")] {
        dispatcher
            .dispatch(RequestEnvelope::Write(Box::new(
                SendRequest::new(
                    atm_home.clone(),
                    workspace_dir.clone(),
                    ROLE_TEAM_LEAD.parse().expect("caller"),
                    &format!("remote@remote-team.{peer}"),
                    TEST_TEAM.parse().expect("team"),
                    SendMessageSource::Inline(body.to_string()),
                    None,
                    false,
                    None,
                    false,
                )
                .expect("store peer write"),
            )))
            .expect("initial peer delivery");
    }
    let (started, started_rx) = mpsc::sync_channel(1);
    let (release, release_rx) = mpsc::sync_channel(1);
    dispatcher
        .install_https_transport(Arc::new(BlockingPeerDelivery {
            blocked_peer: peer_a.clone(),
            started,
            release: std::sync::Mutex::new(release_rx),
        }))
        .expect("install blocking delivery");
    let first_dispatcher = Arc::clone(&dispatcher);
    let first_peer = peer_a.clone();
    let first = std::thread::spawn(move || {
        first_dispatcher.dispatch(RequestEnvelope::PeerSync(PeerSyncRequest {
            peer: first_peer,
        }))
    });
    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("peer A must enter delivery");
    let (second_tx, second_rx) = mpsc::sync_channel(1);
    let second_dispatcher = Arc::clone(&dispatcher);
    let second = std::thread::spawn(move || {
        second_tx
            .send(
                second_dispatcher
                    .dispatch(RequestEnvelope::PeerSync(PeerSyncRequest { peer: peer_b })),
            )
            .expect("report peer B result");
    });
    let second_result = second_rx.recv_timeout(Duration::from_millis(250));
    release.send(()).expect("release peer A");
    assert!(first.join().expect("join peer A sync").is_ok());
    second.join().expect("join peer B sync");
    assert!(
        matches!(
            second_result,
            Ok(Ok(ResponseEnvelope::PeerSync(PeerSyncOutcome {
                disposition: PeerSyncDisposition::Completed,
                ..
            })))
        ),
        "peer B must complete while peer A owns only its own recovery slot"
    );
}
