use super::*;
use crate::peer_delivery_observability::{PeerDeliveryEvent, PeerDeliveryEventKind};

fn configured_dispatcher() -> (TempDir, DaemonRequestDispatcher) {
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");
    write_team_config(&atm_home, &[]);
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
        DaemonRequestDispatcher::new_for_test(atm_home, RuntimeStatusCache::new(), db_path);
    (tempdir, dispatcher)
}

#[test]
#[serial_test::serial(env)]
fn peer_delivery_events_project_only_safe_configured_peer_health() {
    let (_tempdir, dispatcher) = configured_dispatcher();
    let peer: atm_core::types::HostName = "peer.example.test".parse().expect("peer host");
    dispatcher.record_peer_delivery_event(PeerDeliveryEvent {
        kind: PeerDeliveryEventKind::PeerDeliveryUnconfirmed,
        request_id: atm_core::protocol::next_request_id(),
        message_id: Some(atm_core::schema::AtmMessageId::new()),
        peer: peer.clone(),
        error_code: Some(AtmErrorCode::RemoteDeliveryUnconfirmed),
        candidate_count: Some(1),
        next_attempt_at: None,
    });
    let statuses = dispatcher.peer_link_statuses();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].peer, peer);
    assert_eq!(
        statuses[0].quality,
        atm_core::doctor::PeerLinkQuality::Unreachable
    );
    assert_eq!(
        statuses[0].last_error_code,
        Some(AtmErrorCode::RemoteDeliveryUnconfirmed)
    );
    assert!(statuses[0].last_success_at.is_none());
    assert!(statuses[0].next_attempt_at.is_none());
    assert_eq!(statuses[0].drain, atm_core::doctor::PeerDrainState::Idle);

    dispatcher.record_peer_delivery_event(PeerDeliveryEvent {
        kind: PeerDeliveryEventKind::PeerDeliveryConfirmed,
        request_id: atm_core::protocol::next_request_id(),
        message_id: Some(atm_core::schema::AtmMessageId::new()),
        peer,
        error_code: None,
        candidate_count: Some(1),
        next_attempt_at: None,
    });
    let status = dispatcher
        .peer_link_statuses()
        .pop()
        .expect("configured peer status");
    assert_eq!(status.quality, atm_core::doctor::PeerLinkQuality::Healthy);
    assert!(status.last_success_at.is_some());
    assert!(status.last_error_code.is_none());
    assert!(status.next_attempt_at.is_none());
}

#[test]
#[serial_test::serial(env)]
fn configured_peer_without_attempt_is_misconfigured_in_doctor_projection() {
    let (_tempdir, dispatcher) = configured_dispatcher();
    let status = dispatcher
        .peer_link_statuses()
        .pop()
        .expect("configured peer status");
    assert_eq!(status.peer.as_str(), "peer.example.test");
    assert_eq!(
        status.quality,
        atm_core::doctor::PeerLinkQuality::Misconfigured
    );
    assert!(status.last_success_at.is_none());
    assert!(status.last_failure_at.is_none());
    assert!(status.last_error_code.is_none());
    assert!(status.next_attempt_at.is_none());
}

#[test]
#[serial_test::serial(env)]
fn expired_peer_delivery_is_retained_as_a_terminal_degraded_status() {
    let (_tempdir, dispatcher) = configured_dispatcher();
    let peer: atm_core::types::HostName = "peer.example.test".parse().expect("peer host");
    dispatcher.record_peer_delivery_event(PeerDeliveryEvent {
        kind: PeerDeliveryEventKind::PeerDeliveryExpired,
        request_id: atm_core::protocol::next_request_id(),
        message_id: Some(atm_core::schema::AtmMessageId::new()),
        peer,
        error_code: Some(AtmErrorCode::RemoteDeliveryUnconfirmed),
        candidate_count: None,
        next_attempt_at: None,
    });
    let status = dispatcher
        .peer_link_statuses()
        .pop()
        .expect("configured peer status");
    assert_eq!(status.quality, atm_core::doctor::PeerLinkQuality::Degraded);
    assert_eq!(
        status.last_error_code,
        Some(AtmErrorCode::RemoteDeliveryUnconfirmed)
    );
    assert!(status.last_failure_at.is_some());
    assert!(status.next_attempt_at.is_none());
    assert_eq!(status.drain, atm_core::doctor::PeerDrainState::Idle);
}

#[test]
#[serial_test::serial(env)]
fn peer_link_status_json_round_trip_exposes_no_authority_secrets() {
    let (_tempdir, dispatcher) = configured_dispatcher();
    let status = dispatcher
        .peer_link_statuses()
        .pop()
        .expect("configured peer status");

    let serialized = serde_json::to_string(&status).expect("serialize peer link status");
    assert!(
        !serialized.to_ascii_lowercase().contains("fingerprint"),
        "peer-link status must not expose a TLS fingerprint"
    );
    assert!(
        !serialized.to_ascii_lowercase().contains("ip_address"),
        "peer-link status must not expose resolved peer IP addresses"
    );
    assert_eq!(
        serde_json::from_str::<atm_core::doctor::PeerLinkStatus>(&serialized)
            .expect("deserialize peer link status"),
        status,
        "the safe doctor projection must round trip without adding hidden fields"
    );
}

#[test]
#[serial_test::serial(env)]
fn connection_handler_failure_is_not_a_local_admission_failure() {
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
    dispatcher
        .install_https_transport(Arc::new(ConnectionHandlerFailure::default()))
        .expect("install unavailable HTTPS delivery");

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
        .expect("a durable local write must not wait for the peer connection handler");
    assert!(matches!(
        response,
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(_))
    ));
    let status = dispatcher
        .peer_link_statuses()
        .pop()
        .expect("configured peer delivery status");
    assert_eq!(
        status.last_error_code, None,
        "no peer handler ran on the admission path"
    );
}
