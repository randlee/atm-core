use super::*;
use atm_core::api::{HttpFrameReader, decode_request, write_http_response};
use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, SendResponseEnvelope};
use atm_core::read::ReadQuery;
use atm_core::send::{SendCommandOutcome, SendOutcome};
use atm_core::send::{SendMessageSource, SendRequest};
use atm_core::team_admin::{AddMemberRequest, add_member_with_roster_store};
use atm_core::test_support::{EnvGuard, ROLE_TEAM_LEAD};
use atm_core::types::CommandAction;
use atm_core::types::ReadSelection;
use atm_core::{ApiRequest, ApiRouter, AuthenticatedIngress, RequestDeadline};
use atm_runtime_test_support::{SQLITE_RUNTIME_PATH_ENV, open_sqlite_boundary};
use atm_storage::{HostName, HttpsInterface, MessageKey, MessageQuery, PeerAliasKey, TrustedPeer};
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, TcpListener};
use std::num::NonZeroU16;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

use crate::test_support::{
    configure_test_local_ipc_timeouts, connect_daemon_local_ipc_until_ready,
    write_test_local_ipc_request,
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

fn configure_trusted_peer(db_path: &std::path::Path, canonical_host: &str, aliases: &[&str]) {
    assert!(
        canonical_host.parse::<IpAddr>().is_err(),
        "test canonical peer hosts must be stable DNS names; use an IP alias instead"
    );
    let assembly = open_sqlite_boundary(db_path).expect("sqlite boundary");
    let store = assembly.peer_config_store();
    let canonical_host: HostName = canonical_host.parse().expect("canonical host");
    store
        .save_trusted_peer(&TrustedPeer {
            host: canonical_host.clone(),
            fingerprint: "sha256:test".parse().expect("fingerprint"),
            enabled: true,
            https_port: std::num::NonZeroU16::new(43101).expect("port"),
        })
        .expect("save trusted peer");
    for alias in aliases {
        store
            .save_peer_alias(
                alias.parse::<PeerAliasKey>().expect("peer alias"),
                canonical_host.clone(),
            )
            .expect("save peer alias");
    }
}

fn configure_peer_http_source(db_path: &std::path::Path) {
    let assembly = open_sqlite_boundary(db_path).expect("sqlite boundary");
    assembly
        .peer_config_store()
        .save_interface(&HttpsInterface {
            // `new_for_test` does not start listeners. This supplies only the
            // explicitly configured source-host provenance required by AK.4.
            bind_addr: "127.0.0.1:43101".parse().expect("bind address"),
            advertise_host: "origin.example.test".parse().expect("source host"),
            enabled: true,
        })
        .expect("save configured peer HTTP source");
}

fn serve_direct_peer_responses(request_count: usize) -> (NonZeroU16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("peer listener");
    let port = NonZeroU16::new(listener.local_addr().expect("peer address").port())
        .expect("non-zero peer port");
    let server = thread::spawn(move || {
        for _ in 0..request_count {
            let (mut stream, _) = listener.accept().expect("peer accept");
            let raw_request = HttpFrameReader::new()
                .read_request(&mut stream)
                .expect("read peer HTTP request")
                .expect("one peer HTTP request");
            let request = decode_request(raw_request).expect("decode peer HTTP request");
            let ApiRequest::Write(write) = request else {
                panic!("direct peer sender must issue a write request");
            };
            let message_id = write
                .origin_message_id
                .expect("direct peer write preserves immutable message ID");
            write_http_response(
                &mut stream,
                &ResponseEnvelope::Send(SendResponseEnvelope::Sent(SendOutcome {
                    action: CommandAction::Send,
                    team: TEST_TEAM.parse().expect("team"),
                    agent: "remote-agent".parse().expect("recipient"),
                    sender: ROLE_TEAM_LEAD.parse().expect("sender"),
                    outcome: SendCommandOutcome::Sent,
                    message_id,
                    requires_ack: false,
                    task_id: None,
                    summary: None,
                    message: None,
                    warnings: Vec::new(),
                    dry_run: false,
                })),
            )
            .expect("write peer HTTP response");
            stream.flush().expect("flush peer HTTP response");
        }
    });
    (port, server)
}

#[test]
#[serial_test::serial(env)]
fn host_qualified_write_persists_then_confirms_direct_peer_http_delivery() {
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
    let (port, server) = serve_direct_peer_responses(1);
    configure_trusted_peer(&db_path, "localhost", &["peer.example.test", "127.0.0.1"]);
    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");
    assembly
        .peer_config_store()
        .save_trusted_peer(&TrustedPeer {
            host: "localhost".parse().expect("canonical host"),
            fingerprint: "sha256:test".parse().expect("fingerprint"),
            enabled: true,
            https_port: port,
        })
        .expect("replace peer endpoint with test listener");
    configure_peer_http_source(&db_path);
    let dispatcher = DaemonRequestDispatcher::new_for_test(
        atm_home.clone(),
        RuntimeStatusCache::new(),
        db_path.clone(),
    );

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
        .expect("local admission and direct peer delivery must succeed");
    let ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) = response else {
        panic!("host-qualified admission must return sent");
    };
    let message_store = open_sqlite_boundary(&db_path)
        .expect("open durable store")
        .message_store_arc();
    let stored = message_store
        .load_message(&MessageKey::from(outcome.message_id))
        .expect("load persisted origin write")
        .expect("host-qualified origin write is durable");
    assert_eq!(
        stored
            .envelope
            .extra
            .get("peerOutbound")
            .and_then(|value| value.get("host"))
            .and_then(serde_json::Value::as_str),
        None,
        "AK.4 removes the peer marker only after the direct peer confirms the immutable message ID"
    );
    server.join().expect("peer server");
}

#[test]
#[serial_test::serial(env)]
fn failed_direct_peer_response_retains_the_unconfirmed_marker() {
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
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("peer listener");
    let port = NonZeroU16::new(listener.local_addr().expect("peer address").port())
        .expect("non-zero peer port");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("peer accept");
        let _ = HttpFrameReader::new()
            .read_request(&mut stream)
            .expect("read peer request");
        // Closing after receipt models an unknown receiver outcome. The origin
        // must not treat it as confirmation or remove its durable marker.
    });
    configure_trusted_peer(&db_path, "localhost", &["peer.example.test", "127.0.0.1"]);
    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");
    assembly
        .peer_config_store()
        .save_trusted_peer(&TrustedPeer {
            host: "localhost".parse().expect("canonical host"),
            fingerprint: "sha256:test".parse().expect("fingerprint"),
            enabled: true,
            https_port: port,
        })
        .expect("replace peer endpoint with test listener");
    configure_peer_http_source(&db_path);
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), RuntimeStatusCache::new(), db_path);

    let error = dispatcher
        .dispatch(RequestEnvelope::Write(Box::new(
            SendRequest::new(
                atm_home,
                workspace_dir,
                ROLE_TEAM_LEAD.parse().expect("caller"),
                "remote-agent@remote-team.peer.example.test",
                TEST_TEAM.parse().expect("team"),
                SendMessageSource::Inline("unconfirmed peer write".to_owned()),
                None,
                false,
                None,
                false,
            )
            .expect("remote write request"),
        )))
        .expect_err("closed peer response is unconfirmed delivery");
    assert_eq!(error.code(), AtmErrorCode::RemoteDeliveryUnconfirmed);

    let stored = assembly
        .message_store_arc()
        .list_messages(&MessageQuery {
            team: "remote-team".parse().expect("remote team"),
            agent: "remote-agent".parse().expect("remote agent"),
            sender: None,
            task_id: None,
            limit: None,
        })
        .expect("load durable remote origin record");
    assert_eq!(stored.len(), 1);
    assert_eq!(
        stored[0]
            .envelope
            .extra
            .get("peerOutbound")
            .and_then(|value| value.get("host"))
            .and_then(serde_json::Value::as_str),
        Some("localhost")
    );
    server.join().expect("peer server");
}

#[test]
#[serial_test::serial(env)]
fn peer_alias_reload_swaps_the_admission_directory_without_a_message_store_lookup() {
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
    let (port, server) = serve_direct_peer_responses(1);
    configure_trusted_peer(&db_path, "localhost", &[]);
    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");
    assembly
        .peer_config_store()
        .save_trusted_peer(&TrustedPeer {
            host: "localhost".parse().expect("canonical host"),
            fingerprint: "sha256:test".parse().expect("fingerprint"),
            enabled: true,
            https_port: port,
        })
        .expect("replace peer endpoint with test listener");
    configure_peer_http_source(&db_path);
    let dispatcher = DaemonRequestDispatcher::new_for_test(
        atm_home.clone(),
        RuntimeStatusCache::new(),
        db_path.clone(),
    );

    let request = || {
        SendRequest::new(
            atm_home.clone(),
            workspace_dir.clone(),
            ROLE_TEAM_LEAD.parse().expect("caller"),
            "remote-agent@remote-team.peer.example.test",
            TEST_TEAM.parse().expect("team"),
            SendMessageSource::Inline("reload alias".to_string()),
            None,
            false,
            None,
            false,
        )
        .expect("host-qualified request")
    };
    let error = dispatcher
        .dispatch(RequestEnvelope::Write(Box::new(request())))
        .expect_err("unknown aliases fail before admission");
    assert_eq!(error.code(), AtmErrorCode::PeerConfigValidationFailed);

    assembly
        .peer_config_store()
        .save_peer_alias(
            "peer.example.test".parse().expect("host alias"),
            "localhost".parse().expect("canonical host"),
        )
        .expect("save alias");
    dispatcher
        .reload_runtime_view()
        .expect("swap the immutable peer directory");

    let response = dispatcher
        .dispatch(RequestEnvelope::Write(Box::new(request())))
        .expect("reloaded alias is delivered through the direct peer path");
    let ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) = response else {
        panic!("expected persisted host-qualified send");
    };
    let message_store = assembly.message_store_arc();
    let stored = message_store
        .load_message(&MessageKey::from(outcome.message_id))
        .expect("load persisted origin write")
        .expect("origin write is durable");
    assert_eq!(
        stored
            .envelope
            .extra
            .get("peerOutbound")
            .and_then(|value| value.get("host"))
            .and_then(serde_json::Value::as_str),
        None,
        "a direct peer confirmation clears only the matching marker"
    );
    server.join().expect("peer server");
}

#[test]
#[serial_test::serial(env)]
fn authenticated_peer_ingress_uses_canonical_writer_without_reforwarding() {
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
    add_member_via_retained_admin(
        &db_path,
        &atm_home,
        TEST_TEAM,
        "local-recipient",
        &workspace_dir,
    );

    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), RuntimeStatusCache::new(), db_path);
    let origin_message_id = atm_core::schema::AtmMessageId::new();
    let origin_timestamp = atm_core::types::IsoTimestamp::now();
    let write = SendRequest::new(
        atm_home,
        workspace_dir,
        "remote-sender".parse().expect("peer sender"),
        "local-recipient@test-team",
        TEST_TEAM.parse().expect("team"),
        SendMessageSource::Inline("inbound peer write".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("peer write request")
    .with_origin_metadata(origin_message_id, origin_timestamp);
    let mut peer_write = write.clone();
    peer_write.authenticated_source_host = Some("peer.example.test".parse().expect("peer host"));

    let response = ApiRouter::route(
        &dispatcher,
        ApiRequest::new(RequestEnvelope::Write(Box::new(peer_write.clone()))),
        AuthenticatedIngress::Peer,
        RequestDeadline::after(Duration::from_secs(1)),
    )
    .expect("authenticated peer write must enter the shared writer/router");
    assert!(matches!(
        response.into_inner(),
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(_))
    ));

    let replay = ApiRouter::route(
        &dispatcher,
        ApiRequest::new(RequestEnvelope::Write(Box::new(peer_write))),
        AuthenticatedIngress::Peer,
        RequestDeadline::after(Duration::from_secs(1)),
    )
    .expect("same immutable peer delivery is idempotent");
    assert!(matches!(
        replay.into_inner(),
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(_))
    ));
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
        .dispatch(RequestEnvelope::Write(Box::new(
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
    serde_json::to_vec(&response).expect("encode HTTP response body");
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
            .dispatch(RequestEnvelope::Write(Box::new(
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
        serde_json::to_vec(&response).expect("encode HTTP response body");
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
        .dispatch(RequestEnvelope::Write(Box::new(
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

    assert_eq!(error.code(), AtmErrorCode::SelfAddressedSendInvalid);
    assert_eq!(error.code(), AtmErrorCode::SelfAddressedSendInvalid);
}

#[test]
#[serial_test::serial(env)]
fn host_qualified_self_addresses_use_the_ordinary_admission_route() {
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
    let (port, server) = serve_direct_peer_responses(2);
    configure_trusted_peer(&db_path, "localhost", &["127.0.0.1"]);
    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");
    assembly
        .peer_config_store()
        .save_trusted_peer(&TrustedPeer {
            host: "localhost".parse().expect("canonical host"),
            fingerprint: "sha256:test".parse().expect("fingerprint"),
            enabled: true,
            https_port: port,
        })
        .expect("replace peer endpoint with test listener");
    configure_peer_http_source(&db_path);
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), RuntimeStatusCache::new(), db_path);

    for host in ["localhost", "127.0.0.1"] {
        let address = format!("{ROLE_TEAM_LEAD}@{TEST_TEAM}.{host}");
        let response = dispatcher
            .dispatch(RequestEnvelope::Write(Box::new(
                SendRequest::new(
                    atm_home.clone(),
                    workspace_dir.clone(),
                    ROLE_TEAM_LEAD.parse().expect("caller"),
                    &address,
                    TEST_TEAM.parse().expect("team"),
                    SendMessageSource::Inline(format!("ordinary route {host}")),
                    None,
                    false,
                    None,
                    false,
                )
                .expect("host-qualified request"),
            )))
            .expect("host-qualified self address must be admitted");
        assert!(matches!(
            response,
            ResponseEnvelope::Send(SendResponseEnvelope::Sent(_))
        ));
    }
    server.join().expect("peer server");
}

#[test]
#[serial_test::serial(env)]
fn local_forged_peer_provenance_cannot_bypass_self_send_rejection() {
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

    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), RuntimeStatusCache::new(), db_path);
    let self_address = format!("{ROLE_TEAM_LEAD}@{TEST_TEAM}");
    let mut request = SendRequest::new(
        atm_home,
        workspace_dir,
        ROLE_TEAM_LEAD.parse().expect("caller"),
        &self_address,
        TEST_TEAM.parse().expect("team"),
        SendMessageSource::Inline("forged peer provenance".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("send request");
    request.authenticated_source_host = Some("forged.example.test".parse().expect("host"));

    let error = ApiRouter::route(
        &dispatcher,
        ApiRequest::new(RequestEnvelope::Write(Box::new(request))),
        AuthenticatedIngress::Local,
        RequestDeadline::after(Duration::from_secs(1)),
    )
    .expect_err("local peer-provenance claim must not bypass self-send validation");

    assert_eq!(error.code(), AtmErrorCode::SelfAddressedSendInvalid);
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

    assert_eq!(error.code(), AtmErrorCode::MessageValidationFailed);
    assert!(
        error.message().contains("owner-only `atm read`"),
        "{error:?}"
    );
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
    let dispatcher: Arc<dyn ApiRouter + Send + Sync> =
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

    let mut stream = connect_daemon_local_ipc_until_ready(&socket_path, ready_rx);
    configure_test_local_ipc_timeouts(&stream);
    let request = RequestEnvelope::Write(Box::new(
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
    write_test_local_ipc_request(&mut stream, &request).expect("write send request");
    let response =
        atm_core::api::read_http_response(&mut stream, &request).expect("read send response");
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
