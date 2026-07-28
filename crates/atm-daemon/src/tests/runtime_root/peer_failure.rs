use super::*;

#[test]
#[serial_test::serial(env)]
fn unavailable_peer_does_not_relabel_a_committed_local_admission() {
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
    let peer_store = open_sqlite_boundary(&db_path)
        .expect("sqlite boundary")
        .peer_config_store();
    let peer_host: atm_core::types::HostName = "peer.example.test".parse().expect("peer host");
    peer_store
        .save_trusted_peer(&TrustedPeer {
            host: peer_host.clone(),
            fingerprint: "sha256:test-peer".parse().expect("fingerprint"),
            enabled: true,
            https_port: NonZeroU16::new(43101).expect("non-zero"),
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
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), RuntimeStatusCache::new(), db_path);
    let (attempted_tx, attempted_rx) = mpsc::sync_channel(1);
    let transport = Arc::new(RouteFailure {
        attempted: std::sync::Mutex::new(Vec::new()),
        attempted_tx: std::sync::Mutex::new(Some(attempted_tx)),
    });
    dispatcher
        .install_https_transport(transport.clone())
        .expect("install failing HTTPS delivery");
    dispatcher
        .start_peer_drain_coordinator()
        .expect("start post-commit peer worker");
    let response = dispatcher
        .dispatch(RequestEnvelope::Write(Box::new(
            SendRequest::new(
                atm_home,
                workspace_dir,
                ROLE_TEAM_LEAD.parse().expect("caller"),
                "remote-agent@remote-team.peer.example.test",
                TEST_TEAM.parse().expect("team"),
                SendMessageSource::Inline("peer write".to_string()),
                None,
                false,
                None,
                false,
            )
            .expect("remote write request"),
        )))
        .expect("peer unavailability belongs to post-commit recovery, not local admission");
    let local_message_id = match response {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => outcome.message_id,
        other => panic!("local admission must remain sent, got {other:?}"),
    };
    let delivered = attempted_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("post-commit peer worker must attempt the retained message");
    assert_eq!(delivered.origin_message_id, Some(local_message_id));
    assert_eq!(transport.attempted.lock().expect("deliveries").len(), 1);
    assert_eq!(
        dispatcher.peer_link_statuses()[0].last_error_code,
        Some(AtmErrorCode::RemoteDeliveryUnconfirmed)
    );
    dispatcher
        .stop_peer_drain_coordinator()
        .expect("stop post-commit peer worker");
}
