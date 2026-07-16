use super::*;
use atm_core::boundary::{AtmProtocol, RequestDispatcher};
use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::protocol::{
    JsonAtmProtocolCodec, RequestEnvelope, ResponseEnvelope, SendRequestEnvelope,
    SendResponseEnvelope, next_request_id,
};
use atm_core::read::ReadQuery;
use atm_core::send::{RemoteTargetHost, SendMessageSource, SendRequest};
use atm_core::team_admin::{AddMemberRequest, add_member_with_roster_store};
use atm_core::test_support::{EnvGuard, ROLE_TEAM_LEAD};
use atm_core::types::ReadSelection;
use atm_runtime_test_support::{SQLITE_RUNTIME_PATH_ENV, open_sqlite_boundary};
use atm_storage::{PeerSecurityMode, SetPeerSecurityModeCommand, UpsertTrustedPeerCommand};
use std::io::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;
use tempfile::TempDir;

use crate::test_support::{
    configure_test_local_ipc_timeouts, connect_daemon_local_ipc_until_ready,
};

fn add_member_via_retained_admin(
    db_path: &std::path::Path,
    atm_home: &std::path::Path,
    team: &str,
    member: &str,
    member_home_dir: &std::path::Path,
) {
    let assembly = open_sqlite_boundary(db_path).expect("sqlite boundary");
    let roster_store = assembly.roster_store_arc();
    add_member_with_roster_store(
        roster_store.as_ref(),
        AddMemberRequest::new(
            atm_home.to_path_buf(),
            team,
            member,
            "general-purpose".to_string(),
            "unknown".to_string(),
            member_home_dir.to_path_buf(),
            None,
        )
        .expect("add-member request"),
    )
    .expect("add member");
}

fn configure_secure_loopback(db_path: &std::path::Path, host: &str) {
    let assembly = open_sqlite_boundary(db_path).expect("sqlite boundary");
    assembly
        .allowed_host_store_arc()
        .allow_host(
            atm_storage::AllowHostCommand::new(
                host,
                format!("{ROLE_TEAM_LEAD}@{TEST_TEAM}"),
                Some("loopback".to_string()),
            )
            .expect("allow host command"),
        )
        .expect("allow host");
    let security_store = assembly.peer_security_store_arc();
    security_store
        .set_security_mode(
            SetPeerSecurityModeCommand::new(
                PeerSecurityMode::SecureRequired,
                format!("{ROLE_TEAM_LEAD}@{TEST_TEAM}"),
            )
            .expect("security mode command"),
        )
        .expect("set security mode");
    let identity = security_store
        .load_or_create_local_identity()
        .expect("local identity");
    security_store
        .upsert_trusted_peer(
            UpsertTrustedPeerCommand::new(
                host,
                identity.fingerprint_sha256().to_string(),
                Some("loopback".to_string()),
                format!("{ROLE_TEAM_LEAD}@{TEST_TEAM}"),
            )
            .expect("trusted peer command"),
        )
        .expect("upsert trusted peer");
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_send_after_add_member_roster_state_serializes_cleanly() {
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    let workspace_dir = tempdir.path().join("workspace");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir");
    let db_path = tempdir.path().join("mail.db");
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

    let status_cache = RuntimeStatusCache::new();
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), status_cache, db_path.clone());
    let response = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(
            SendRequest::new(
                atm_home.clone(),
                workspace_dir.clone(),
                ROLE_TEAM_LEAD.parse().expect("caller"),
                "qa-a@test-team",
                TEST_TEAM.parse().expect("team"),
                SendMessageSource::Inline("hello add-member roster".to_string()),
                None,
                false,
                None,
                false,
            )
            .expect("send request"),
        )))
        .expect("dispatch send");
    let ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) = &response else {
        panic!("expected send response, got {response:?}");
    };
    assert_eq!(outcome.outcome.as_str(), "sent");
    JsonAtmProtocolCodec
        .response_to_frame(next_request_id(), response)
        .expect("encode send response");
}

#[test]
#[serial_test::serial(env)]
fn threaded_dispatcher_send_after_add_member_roster_state_serializes_cleanly() {
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
    add_member_via_retained_admin(&db_path, &atm_home, TEST_TEAM, "qa-a", &workspace_dir);

    let dispatcher = DaemonRequestDispatcher::new_for_test(
        atm_home.clone(),
        RuntimeStatusCache::new(),
        db_path.clone(),
    );
    let handle = std::thread::spawn(move || {
        let response = dispatcher
            .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(
                SendRequest::new(
                    atm_home.clone(),
                    workspace_dir.clone(),
                    ROLE_TEAM_LEAD.parse().expect("caller"),
                    "qa-a@test-team",
                    TEST_TEAM.parse().expect("team"),
                    SendMessageSource::Inline("hello threaded dispatch".to_string()),
                    None,
                    false,
                    None,
                    false,
                )
                .expect("send request"),
            )))
            .expect("dispatch send");
        JsonAtmProtocolCodec
            .response_to_frame(next_request_id(), response)
            .expect("encode send response");
    });

    handle.join().expect("threaded send dispatch");
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_send_rejects_self_addressed_message_before_persistence() {
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

    let dispatcher = DaemonRequestDispatcher::new_for_test(
        atm_home.clone(),
        RuntimeStatusCache::new(),
        db_path.clone(),
    );
    let self_address = format!("{ROLE_TEAM_LEAD}@{TEST_TEAM}");
    let error = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(
            SendRequest::new(
                atm_home.clone(),
                workspace_dir.clone(),
                ROLE_TEAM_LEAD.parse().expect("caller"),
                &self_address,
                TEST_TEAM.parse().expect("team"),
                SendMessageSource::Inline("hello self".to_string()),
                None,
                false,
                None,
                false,
            )
            .expect("send request"),
        )))
        .expect_err("self-addressed daemon send must fail");

    assert_eq!(error.code, AtmErrorCode::SelfAddressedSendInvalid);
    assert!(error.is_validation());
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_read_rejects_cross_agent_target_on_mutating_path() {
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
    add_member_via_retained_admin(&db_path, &atm_home, TEST_TEAM, "qa-a", &workspace_dir);

    let dispatcher = DaemonRequestDispatcher::new_for_test(
        atm_home.clone(),
        RuntimeStatusCache::new(),
        db_path.clone(),
    );
    let error = dispatcher
        .dispatch(RequestEnvelope::Receive(
            ReadQuery::new(
                atm_home.clone(),
                workspace_dir.clone(),
                ROLE_TEAM_LEAD.parse().expect("caller"),
                Some("qa-a@test-team"),
                TEST_TEAM.parse().expect("team"),
                ReadSelection::All,
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
        .expect_err("cross-agent daemon read must fail");

    assert_eq!(error.code, AtmErrorCode::MessageValidationFailed);
    assert!(error.message.contains("owner-only `atm read`"), "{error:?}");
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_loopback_send_round_trips_through_peer_listener_into_self_inbox() {
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
    add_member_via_retained_admin(&db_path, &atm_home, TEST_TEAM, "qa-a", &workspace_dir);

    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");
    assembly
        .allowed_host_store_arc()
        .allow_host(
            atm_storage::AllowHostCommand::new(
                "127.0.0.1",
                format!("{ROLE_TEAM_LEAD}@{TEST_TEAM}"),
                Some("localhost success fixture".to_string()),
            )
            .expect("allow host command"),
        )
        .expect("allow host");
    let status_cache = RuntimeStatusCache::new();
    let peer_transport = crate::PeerTransportRuntime::new_server_for_test_with_allowed_host_store(
        SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0)),
        crate::SubsystemObservability::disabled(crate::DaemonSubsystem::PeerTransport),
        status_cache.clone(),
        assembly.allowed_host_store_arc(),
    );
    let dispatcher = Arc::new(DaemonRequestDispatcher::new_for_test_with_peer_transport(
        atm_home.clone(),
        status_cache,
        db_path.clone(),
        peer_transport.clone(),
    ));
    peer_transport
        .start(dispatcher.clone())
        .expect("start peer listener");

    let mut request = SendRequest::new(
        atm_home.clone(),
        workspace_dir.clone(),
        ROLE_TEAM_LEAD.parse().expect("caller"),
        "qa-a@test-team",
        TEST_TEAM.parse().expect("team"),
        SendMessageSource::Inline("hello loopback".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("send request");
    request.remote_host = Some(RemoteTargetHost::parse("localhost").expect("host"));

    let response = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(request)))
        .expect("dispatch loopback send");
    let ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) = response else {
        panic!("expected send response");
    };
    assert_eq!(outcome.agent.as_str(), "qa-a");

    let read = dispatcher
        .dispatch(RequestEnvelope::Receive(
            ReadQuery::new(
                atm_home,
                workspace_dir,
                "qa-a".parse().expect("caller"),
                None,
                TEST_TEAM.parse().expect("team"),
                ReadSelection::All,
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
        .expect("read self inbox");
    let ResponseEnvelope::Receive(report) = read else {
        panic!("expected receive response");
    };
    assert!(
        report
            .message
            .as_ref()
            .is_some_and(|message| message.envelope.text == "hello loopback"),
        "localhost remote-target message missing from inbox"
    );

    peer_transport.shutdown().expect("shutdown peer listener");
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_loopback_send_rejects_unauthorized_host_before_mailbox_mutation() {
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
    add_member_via_retained_admin(&db_path, &atm_home, TEST_TEAM, "qa-a", &workspace_dir);

    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");
    let allowed_host = "127.0.0.1"
        .parse::<atm_storage::AllowedHostName>()
        .expect("allowed host");
    assembly
        .allowed_host_store_arc()
        .allow_host(
            atm_storage::AllowHostCommand::new(
                "127.0.0.1",
                format!("{ROLE_TEAM_LEAD}@{TEST_TEAM}"),
                Some("localhost rejection fixture".to_string()),
            )
            .expect("allow host command"),
        )
        .expect("allow host");
    assembly
        .allowed_host_store_arc()
        .deny_host(&allowed_host)
        .expect("deny host");
    let status_cache = RuntimeStatusCache::new();
    let peer_transport = crate::PeerTransportRuntime::new_server_for_test_with_allowed_host_store(
        SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0)),
        crate::SubsystemObservability::disabled(crate::DaemonSubsystem::PeerTransport),
        status_cache.clone(),
        assembly.allowed_host_store_arc(),
    );
    let dispatcher = Arc::new(DaemonRequestDispatcher::new_for_test_with_peer_transport(
        atm_home.clone(),
        status_cache,
        db_path.clone(),
        peer_transport.clone(),
    ));
    peer_transport
        .start(dispatcher.clone())
        .expect("start peer listener");

    let mut request = SendRequest::new(
        atm_home.clone(),
        workspace_dir.clone(),
        ROLE_TEAM_LEAD.parse().expect("caller"),
        "qa-a@test-team",
        TEST_TEAM.parse().expect("team"),
        SendMessageSource::Inline("unauthorized localhost".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("send request");
    request.remote_host = Some(RemoteTargetHost::parse("localhost").expect("host"));

    let error = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(request)))
        .expect_err("unauthorized localhost send must fail closed");
    assert_eq!(error.code, AtmErrorCode::MessageValidationFailed);

    let read = dispatcher
        .dispatch(RequestEnvelope::Receive(
            ReadQuery::new(
                atm_home,
                workspace_dir,
                "qa-a".parse().expect("caller"),
                None,
                TEST_TEAM.parse().expect("team"),
                ReadSelection::All,
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
        .expect("read self inbox");
    let ResponseEnvelope::Receive(report) = read else {
        panic!("expected receive response");
    };
    assert!(
        report.message.is_none(),
        "unauthorized localhost send mutated the receiver mailbox"
    );

    peer_transport.shutdown().expect("shutdown peer listener");
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_secure_loopback_requires_ack_round_trips_and_updates_reply_state() {
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
    add_member_via_retained_admin(&db_path, &atm_home, TEST_TEAM, "qa-a", &workspace_dir);
    configure_secure_loopback(&db_path, "127.0.0.1");
    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");

    let status_cache = RuntimeStatusCache::new();
    let peer_transport = crate::PeerTransportRuntime::new_with_observability(
        None,
        Some(assembly.allowed_host_store_arc()),
        Some(assembly.peer_security_store_arc()),
        crate::peer_transport::PeerTransportConfig {
            remote_retry_budget: Duration::from_secs(30),
            peer_listen_addr: Some(SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0))),
        },
        crate::SubsystemObservability::disabled(crate::DaemonSubsystem::PeerTransport),
        status_cache.clone(),
    );
    let dispatcher = Arc::new(DaemonRequestDispatcher::new_for_test_with_peer_transport(
        atm_home.clone(),
        status_cache,
        db_path.clone(),
        peer_transport.clone(),
    ));
    peer_transport
        .start(dispatcher.clone())
        .expect("start secure peer listener");

    let mut request = SendRequest::new(
        atm_home.clone(),
        workspace_dir.clone(),
        ROLE_TEAM_LEAD.parse().expect("caller"),
        "qa-a@test-team",
        TEST_TEAM.parse().expect("team"),
        SendMessageSource::Inline("hello secure ack loopback".to_string()),
        None,
        true,
        None,
        false,
    )
    .expect("send request");
    request.remote_host = Some(RemoteTargetHost::parse("localhost").expect("host"));

    let response = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(request)))
        .expect("dispatch secure loopback send");
    let ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) = response else {
        panic!("expected send response");
    };
    assert_eq!(outcome.agent.as_str(), "qa-a");
    assert!(outcome.requires_ack);

    let read = dispatcher
        .dispatch(RequestEnvelope::Receive(
            ReadQuery::new(
                atm_home.clone(),
                workspace_dir.clone(),
                "qa-a".parse().expect("caller"),
                None,
                TEST_TEAM.parse().expect("team"),
                ReadSelection::All,
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
        .expect("read self inbox");
    let ResponseEnvelope::Receive(report) = read else {
        panic!("expected receive response");
    };
    let message = report.message.expect("loopback message");
    assert_eq!(message.envelope.text, "hello secure ack loopback");
    let source_message_id = message.envelope.message_id.expect("message id");

    let ack = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(
            atm_core::ack::AckRequest {
                home_dir: atm_home.clone(),
                current_dir: workspace_dir.clone(),
                caller_identity: "qa-a".parse().expect("caller"),
                caller_team: TEST_TEAM.parse().expect("team"),
                message_id: source_message_id,
                reply_body: "ack from secure localhost".to_string(),
            },
        )))
        .expect("ack over secure localhost");
    let ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome)) = ack else {
        panic!("expected ack response");
    };
    assert!(matches!(
        outcome.reply_disposition,
        atm_core::ack::AckReplyDisposition::Sent { .. }
    ));

    let sender_read = dispatcher
        .dispatch(RequestEnvelope::Receive(
            ReadQuery::new(
                atm_home,
                workspace_dir,
                ROLE_TEAM_LEAD.parse().expect("caller"),
                None,
                TEST_TEAM.parse().expect("team"),
                ReadSelection::All,
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
        .expect("read sender inbox");
    let ResponseEnvelope::Receive(report) = sender_read else {
        panic!("expected sender receive response");
    };
    let ack_message = report.message.expect("ack reply message");
    assert_eq!(ack_message.envelope.text, "ack from secure localhost");
    assert_eq!(
        ack_message.envelope.acknowledges_message_id,
        Some(source_message_id)
    );

    peer_transport
        .shutdown()
        .expect("shutdown secure peer listener");
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_secure_loopback_send_round_trips_through_peer_listener_into_self_inbox() {
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
    add_member_via_retained_admin(&db_path, &atm_home, TEST_TEAM, "qa-a", &workspace_dir);
    configure_secure_loopback(&db_path, "127.0.0.1");
    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");

    let status_cache = RuntimeStatusCache::new();
    let peer_transport = crate::PeerTransportRuntime::new_with_observability(
        None,
        Some(assembly.allowed_host_store_arc()),
        Some(assembly.peer_security_store_arc()),
        crate::peer_transport::PeerTransportConfig {
            remote_retry_budget: Duration::from_secs(30),
            peer_listen_addr: Some(SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0))),
        },
        crate::SubsystemObservability::disabled(crate::DaemonSubsystem::PeerTransport),
        status_cache.clone(),
    );
    let dispatcher = Arc::new(DaemonRequestDispatcher::new_for_test_with_peer_transport(
        atm_home.clone(),
        status_cache,
        db_path.clone(),
        peer_transport.clone(),
    ));
    peer_transport
        .start(dispatcher.clone())
        .expect("start secure peer listener");

    let mut request = SendRequest::new(
        atm_home.clone(),
        workspace_dir.clone(),
        ROLE_TEAM_LEAD.parse().expect("caller"),
        "qa-a@test-team",
        TEST_TEAM.parse().expect("team"),
        SendMessageSource::Inline("hello secure loopback".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("send request");
    request.remote_host = Some(RemoteTargetHost::parse("localhost").expect("host"));

    let response = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(request)))
        .expect("dispatch secure loopback send");
    let ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) = response else {
        panic!("expected send response");
    };
    assert_eq!(outcome.agent.as_str(), "qa-a");

    let read = dispatcher
        .dispatch(RequestEnvelope::Receive(
            ReadQuery::new(
                atm_home,
                workspace_dir,
                "qa-a".parse().expect("caller"),
                None,
                TEST_TEAM.parse().expect("team"),
                ReadSelection::All,
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
        .expect("read self inbox");
    let ResponseEnvelope::Receive(report) = read else {
        panic!("expected receive response");
    };
    assert!(
        report
            .message
            .as_ref()
            .is_some_and(|message| message.envelope.text == "hello secure loopback"),
        "secure loopback-delivered message missing from inbox"
    );

    peer_transport
        .shutdown()
        .expect("shutdown secure peer listener");
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_self_ip_send_rejects_disabled_host_before_mailbox_mutation() {
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
    add_member_via_retained_admin(&db_path, &atm_home, TEST_TEAM, "qa-a", &workspace_dir);

    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");
    let allowed_host = "127.0.0.1"
        .parse::<atm_storage::AllowedHostName>()
        .expect("allowed host");
    assembly
        .allowed_host_store_arc()
        .allow_host(
            atm_storage::AllowHostCommand::new(
                "127.0.0.1",
                format!("{ROLE_TEAM_LEAD}@{TEST_TEAM}"),
                Some("self-ip rejection fixture".to_string()),
            )
            .expect("allow host command"),
        )
        .expect("allow host");
    assembly
        .allowed_host_store_arc()
        .deny_host(&allowed_host)
        .expect("deny host");

    let status_cache = RuntimeStatusCache::new();
    let peer_transport = crate::PeerTransportRuntime::new_server_for_test_with_allowed_host_store(
        SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0)),
        crate::SubsystemObservability::disabled(crate::DaemonSubsystem::PeerTransport),
        status_cache.clone(),
        assembly.allowed_host_store_arc(),
    );
    let dispatcher = Arc::new(DaemonRequestDispatcher::new_for_test_with_peer_transport(
        atm_home.clone(),
        status_cache,
        db_path.clone(),
        peer_transport.clone(),
    ));
    peer_transport
        .start(dispatcher.clone())
        .expect("start peer listener");

    let mut request = SendRequest::new(
        atm_home.clone(),
        workspace_dir.clone(),
        ROLE_TEAM_LEAD.parse().expect("caller"),
        "qa-a@test-team",
        TEST_TEAM.parse().expect("team"),
        SendMessageSource::Inline("unauthorized self ip".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("send request");
    request.remote_host = Some(RemoteTargetHost::parse("127.0.0.1").expect("host"));

    let error = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(request)))
        .expect_err("self-ip send must fail closed");
    assert_eq!(error.code, AtmErrorCode::MessageValidationFailed);

    let read = dispatcher
        .dispatch(RequestEnvelope::Receive(
            ReadQuery::new(
                atm_home,
                workspace_dir,
                "qa-a".parse().expect("caller"),
                None,
                TEST_TEAM.parse().expect("team"),
                ReadSelection::All,
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
        .expect("read self inbox");
    let ResponseEnvelope::Receive(report) = read else {
        panic!("expected receive response");
    };
    assert!(
        report.message.is_none(),
        "unauthorized self-ip send mutated the receiver mailbox"
    );

    peer_transport.shutdown().expect("shutdown peer listener");
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_secure_self_ip_requires_ack_round_trips_and_updates_reply_state() {
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
    add_member_via_retained_admin(&db_path, &atm_home, TEST_TEAM, "qa-a", &workspace_dir);
    configure_secure_loopback(&db_path, "127.0.0.1");
    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");

    let status_cache = RuntimeStatusCache::new();
    let peer_transport = crate::PeerTransportRuntime::new_with_observability(
        None,
        Some(assembly.allowed_host_store_arc()),
        Some(assembly.peer_security_store_arc()),
        crate::peer_transport::PeerTransportConfig {
            remote_retry_budget: Duration::from_secs(30),
            peer_listen_addr: Some(SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0))),
        },
        crate::SubsystemObservability::disabled(crate::DaemonSubsystem::PeerTransport),
        status_cache.clone(),
    );
    let dispatcher = Arc::new(DaemonRequestDispatcher::new_for_test_with_peer_transport(
        atm_home.clone(),
        status_cache,
        db_path.clone(),
        peer_transport.clone(),
    ));
    peer_transport
        .start(dispatcher.clone())
        .expect("start secure peer listener");

    let mut request = SendRequest::new(
        atm_home.clone(),
        workspace_dir.clone(),
        ROLE_TEAM_LEAD.parse().expect("caller"),
        "qa-a@test-team",
        TEST_TEAM.parse().expect("team"),
        SendMessageSource::Inline("hello secure self ip".to_string()),
        None,
        true,
        None,
        false,
    )
    .expect("send request");
    request.remote_host = Some(RemoteTargetHost::parse("127.0.0.1").expect("host"));

    let response = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(request)))
        .expect("dispatch secure self-ip send");
    let ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) = response else {
        panic!("expected send response");
    };
    assert_eq!(outcome.agent.as_str(), "qa-a");
    assert!(outcome.requires_ack);

    let read = dispatcher
        .dispatch(RequestEnvelope::Receive(
            ReadQuery::new(
                atm_home.clone(),
                workspace_dir.clone(),
                "qa-a".parse().expect("caller"),
                None,
                TEST_TEAM.parse().expect("team"),
                ReadSelection::All,
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
        .expect("read self inbox");
    let ResponseEnvelope::Receive(report) = read else {
        panic!("expected receive response");
    };
    let message = report.message.expect("self-ip message");
    assert_eq!(message.envelope.text, "hello secure self ip");
    let source_message_id = message.envelope.message_id.expect("message id");

    let ack = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(
            atm_core::ack::AckRequest {
                home_dir: atm_home.clone(),
                current_dir: workspace_dir.clone(),
                caller_identity: "qa-a".parse().expect("caller"),
                caller_team: TEST_TEAM.parse().expect("team"),
                message_id: source_message_id,
                reply_body: "ack from secure self ip".to_string(),
            },
        )))
        .expect("ack over secure self ip");
    let ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome)) = ack else {
        panic!("expected ack response");
    };
    assert!(matches!(
        outcome.reply_disposition,
        atm_core::ack::AckReplyDisposition::Sent { .. }
    ));

    let sender_read = dispatcher
        .dispatch(RequestEnvelope::Receive(
            ReadQuery::new(
                atm_home,
                workspace_dir,
                ROLE_TEAM_LEAD.parse().expect("caller"),
                None,
                TEST_TEAM.parse().expect("team"),
                ReadSelection::All,
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
        .expect("read sender inbox");
    let ResponseEnvelope::Receive(report) = sender_read else {
        panic!("expected sender receive response");
    };
    let ack_message = report.message.expect("ack reply message");
    assert_eq!(ack_message.envelope.text, "ack from secure self ip");
    assert_eq!(
        ack_message.envelope.acknowledges_message_id,
        Some(source_message_id)
    );

    peer_transport
        .shutdown()
        .expect("shutdown secure peer listener");
}

#[test]
#[serial_test::serial(env)]
fn local_ipc_runtime_round_trips_send_after_add_member_roster_state() {
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
    let dispatcher: Arc<dyn RequestDispatcher + Send + Sync> =
        Arc::new(DaemonRequestDispatcher::new_for_test(
            atm_home.clone(),
            RuntimeStatusCache::new(),
            db_path.clone(),
        ));
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
                finalize_shutdown: || {},
                publish_ready: move || {
                    ready_tx.send(()).map_err(|_| {
                        AtmError::daemon_unavailable(
                            "send round-trip test failed to observe the daemon ready signal",
                        )
                        .with_recovery(
                            "Rerun the same-host daemon test after restoring the bounded ready-signal handshake.",
                        )
                    })
                },
            },
        );
        serve_result_tx.send(result).expect("send serve result");
    });

    let mut stream = connect_daemon_local_ipc_until_ready(&socket_path, ready_rx);
    configure_test_local_ipc_timeouts(&stream);
    let request = RequestEnvelope::Send(SendRequestEnvelope::Compose(
        SendRequest::new(
            atm_home.clone(),
            workspace_dir.clone(),
            ROLE_TEAM_LEAD.parse().expect("caller"),
            "qa-a@test-team",
            TEST_TEAM.parse().expect("team"),
            SendMessageSource::Inline("hello local ipc".to_string()),
            None,
            false,
            None,
            false,
        )
        .expect("send request"),
    ));
    let request_id = next_request_id();
    let frame = atm_core::protocol::request_to_frame_payload(request_id, request).expect("frame");
    atm_core::protocol::write_frame(&mut stream, &frame, "write send frame").expect("write");
    stream.flush().expect("flush");
    let response_frame =
        atm_core::protocol::read_frame(&mut stream, "read send frame", "send frame too large")
            .expect("read frame")
            .expect("response frame");
    let (response_id, response) =
        atm_core::protocol::response_from_frame_payload(response_frame).expect("decode response");
    assert_eq!(response_id, request_id);
    match response {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => {
            assert_eq!(outcome.outcome.as_str(), "sent");
        }
        other => panic!("unexpected response: {other:?}"),
    }

    lifecycle.set_terminate_for_test(true);
    serve_result_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("recv serve result")
        .expect("serve runtime result");
    join.join().expect("join serve thread");
}

#[test]
#[serial_test::serial(env)]
fn local_ipc_client_preflight_round_trips_ack_required_send_after_add_member_roster_state() {
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
    let dispatcher: Arc<dyn RequestDispatcher + Send + Sync> =
        Arc::new(DaemonRequestDispatcher::new_for_test(
            atm_home.clone(),
            RuntimeStatusCache::new(),
            db_path.clone(),
        ));
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
                finalize_shutdown: || {},
                publish_ready: move || {
                    ready_tx.send(()).map_err(|_| {
                        AtmError::daemon_unavailable(
                            "send round-trip test failed to observe the daemon ready signal",
                        )
                        .with_recovery(
                            "Rerun the same-host daemon test after restoring the bounded ready-signal handshake.",
                        )
                    })
                },
            },
        );
        serve_result_tx.send(result).expect("send serve result");
    });

    let _stream = connect_daemon_local_ipc_until_ready(&socket_path, ready_rx);
    let endpoint =
        atm_daemon_client::DaemonLocalIpcEndpoint::new(socket_path.clone()).expect("endpoint");
    let request = SendRequest::new(
        atm_home.clone(),
        workspace_dir.clone(),
        ROLE_TEAM_LEAD.parse().expect("caller"),
        "qa-a@test-team",
        TEST_TEAM.parse().expect("team"),
        SendMessageSource::Inline("hello local ipc ack-required".to_string()),
        None,
        true,
        None,
        false,
    )
    .expect("send request");
    let request = RequestEnvelope::Send(SendRequestEnvelope::Compose(request));
    let envelope = atm_daemon_client::RpcEnvelope::encode_body(
        atm_daemon_client::RpcHeader::new(
            atm_daemon_client::RequestId::new(next_request_id().into_inner()).expect("request id"),
            atm_daemon_client::MessageKind::SendComposeRequest,
        ),
        &request,
    )
    .expect("encode request");

    let mut verified = atm_daemon_client::verify_connection_compatibility(
        &endpoint,
        atm_daemon_client::CompatibilityPreflight {
            client_release: atm_daemon_client::ReleaseVersion::current(),
            wire_version: atm_core::protocol::ATM_FRAME_VERSION_V1,
        },
        Duration::from_secs(3),
    )
    .expect("preflight compatible");
    let response = verified
        .dispatch_write(&endpoint, envelope, Duration::from_secs(3))
        .expect("dispatch write");
    let response: ResponseEnvelope = response.decode_body().expect("decode response");
    match response {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => {
            assert_eq!(outcome.outcome.as_str(), "sent");
            assert!(outcome.requires_ack);
        }
        other => panic!("unexpected response: {other:?}"),
    }

    lifecycle.set_terminate_for_test(true);
    serve_result_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("recv serve result")
        .expect("serve runtime result");
    join.join().expect("join serve thread");
}
