use super::*;

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

    let ack = crate::test_support::test_ack_write_request(
        atm_home,
        workspace_dir,
        "local-recipient".parse().expect("recipient"),
        TEST_TEAM.parse().expect("team"),
        source_message_id,
        "acknowledged",
    );
    let failing = Arc::new(FailingHttpsDelivery::default());
    dispatcher
        .install_https_transport(failing.clone())
        .expect("install failing transport");
    let error = dispatcher
        .dispatch(RequestEnvelope::Write(Box::new(ack.clone())))
        .expect_err("failed remote acknowledgement must return the transport error");
    assert_eq!(error.code(), AtmErrorCode::DaemonUnavailable);
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
        .dispatch(RequestEnvelope::Write(Box::new(ack)))
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
        "two ordinary writes plus exactly one bounded reconciliation delivery are delivered"
    );
    assert_eq!(
        delivered[0].origin_message_id, delivered[2].origin_message_id,
        "reconciliation reuses the canonical immutable write and its original ULID"
    );
    drop(delivered);

    let cooldown = dispatcher
        .dispatch(RequestEnvelope::PeerSync(PeerSyncRequest { peer }))
        .expect("immediate duplicate sync is rate limited");
    assert!(matches!(
        cooldown,
        ResponseEnvelope::PeerSync(PeerSyncOutcome { delivered: 0, .. })
    ));
    assert_eq!(
        transport.delivered.lock().expect("deliveries").len(),
        3,
        "the cooldown prevents another peer delivery pass"
    );
}

#[test]
#[serial_test::serial(env)]
fn successful_peer_write_does_not_start_automatic_reconciliation() {
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
        })
        .expect("save trusted peer");
    peer_store
        .save_peer_sync_policy(
            &peer,
            PeerSyncPolicy {
                max_message_age: Duration::from_secs(60),
                max_batch_messages: NonZeroU16::new(100).expect("non-zero cap"),
            },
        )
        .expect("enable explicit peer sync");

    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), RuntimeStatusCache::new(), db_path);
    let transport = Arc::new(RecordingHttpsDelivery::default());
    dispatcher
        .install_https_transport(transport.clone())
        .expect("install test HTTPS delivery");
    dispatcher
        .dispatch(RequestEnvelope::Write(Box::new(
            SendRequest::new(
                atm_home,
                workspace_dir,
                ROLE_TEAM_LEAD.parse().expect("caller"),
                "remote-agent@remote-team.peer.example.test",
                TEST_TEAM.parse().expect("team"),
                SendMessageSource::Inline("stored once".to_string()),
                None,
                false,
                None,
                false,
            )
            .expect("remote write request"),
        )))
        .expect("initial peer write");
    assert_eq!(
        transport.delivered.lock().expect("deliveries").len(),
        1,
        "a successful ordinary peer write never starts a second delivery pass"
    );
}
