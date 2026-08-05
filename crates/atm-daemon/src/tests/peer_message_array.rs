use super::super::runtime_health::{DaemonRequestDispatcher, RuntimeStatusCache};
use super::{TEST_TEAM, install_retained_runtime_factory, write_team_config};
use atm_core::api::PeerMessageArray;
use atm_core::boundary::RosterHarness;
use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::graft::{
    GraftPostSendResponse, GraftReceiverListener, graft_receiver_record_path_from_home,
};
use atm_core::protocol::{ResponseEnvelope, SendResponseEnvelope};
use atm_core::schema::AgentMember;
use atm_core::send::{SendMessageSource, SendRequest};
use atm_core::test_support::ROLE_TEAM_LEAD;
use atm_core::{ApiRequest, ApiRouter, AuthenticatedIngress, RequestDeadline};
use atm_runtime_test_support::open_sqlite_boundary;
use atm_storage::{AtmMessageId, MessageKey};
use std::time::Duration;
use tempfile::TempDir;

use super::runtime_root::add_member_via_retained_admin;

fn inbound_peer_write(
    atm_home: std::path::PathBuf,
    workspace_dir: std::path::PathBuf,
    body: String,
) -> SendRequest {
    let mut request = SendRequest::new(
        atm_home,
        workspace_dir,
        "remote-agent".parse().expect("peer caller"),
        &format!("{ROLE_TEAM_LEAD}@{TEST_TEAM}"),
        "remote-team".parse().expect("peer team"),
        SendMessageSource::Inline(body),
        None,
        false,
        None,
        false,
    )
    .expect("inbound peer write")
    .with_origin_metadata(AtmMessageId::new(), atm_core::types::IsoTimestamp::now());
    request.authenticated_source_host = Some("origin.example.test".parse().expect("peer host"));
    request
}

fn peer_array_dispatcher() -> (
    TempDir,
    std::path::PathBuf,
    std::path::PathBuf,
    std::path::PathBuf,
    DaemonRequestDispatcher,
) {
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
    let dispatcher = DaemonRequestDispatcher::new_for_test(
        atm_home.clone(),
        RuntimeStatusCache::new(),
        db_path.clone(),
    );
    (tempdir, atm_home, workspace_dir, db_path, dispatcher)
}

#[allow(
    deprecated,
    reason = "the dispatcher fixture must install a Codex recipient through the legacy boundary adapter"
)]
fn peer_array_graft_dispatcher() -> (
    TempDir,
    std::path::PathBuf,
    std::path::PathBuf,
    std::path::PathBuf,
    DaemonRequestDispatcher,
) {
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    let workspace_dir = tempdir.path().join("workspace");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir");
    let db_path = tempdir.path().join("mail.db");
    write_team_config(&atm_home, &[]);

    let assembly = open_sqlite_boundary(&db_path).expect("open sqlite boundary");
    let roster_store = assembly.roster_store_arc();
    let team: atm_core::types::TeamName = TEST_TEAM.parse().expect("team");
    let mut member = AgentMember::with_name(ROLE_TEAM_LEAD.parse().expect("member"));
    member.home_dir = workspace_dir.clone().into();
    let mut record =
        atm_core::boundary::roster_member_record_from_claude_code_member(team.clone(), member);
    record.harness = RosterHarness::CodexCli;
    roster_store
        .replace_roster(&team, &[record])
        .expect("install graft-capable recipient");

    let dispatcher = DaemonRequestDispatcher::new_for_test(
        atm_home.clone(),
        RuntimeStatusCache::new(),
        db_path.clone(),
    );
    (tempdir, atm_home, workspace_dir, db_path, dispatcher)
}

#[test]
#[serial_test::serial(env)]
fn post_write_router_uses_receive_hook_for_receipts_and_never_falls_back_for_hosts() {
    let (_tempdir, atm_home, workspace_dir, _db_path, dispatcher) = peer_array_dispatcher();

    let peer_receipt = inbound_peer_write(
        atm_home.clone(),
        workspace_dir.clone(),
        "peer receipt takes the canonical local route".to_string(),
    );
    let peer_receipt_id = peer_receipt.origin_message_id.expect("peer receipt id");
    let peer_response = dispatcher
        .route(
            ApiRequest::PeerMessages(Box::new(PeerMessageArray {
                messages: vec![peer_receipt],
            })),
            AuthenticatedIngress::Peer,
            RequestDeadline::after(Duration::from_secs(1)),
        )
        .expect("peer receipt succeeds after durable admission")
        .into_inner();
    assert!(matches!(
        peer_response,
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) if outcome.message_id == peer_receipt_id
    ));

    let hostless_write = SendRequest::new(
        atm_home.clone(),
        workspace_dir.clone(),
        ROLE_TEAM_LEAD.parse().expect("local caller"),
        &format!("local-recipient@{TEST_TEAM}"),
        TEST_TEAM.parse().expect("local team"),
        SendMessageSource::Inline("hostless write takes local route".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("hostless local write");
    let hostless_response = dispatcher
        .route(
            ApiRequest::Write(Box::new(hostless_write)),
            AuthenticatedIngress::Local,
            RequestDeadline::after(Duration::from_secs(1)),
        )
        .expect("hostless write succeeds after durable admission")
        .into_inner();
    let ResponseEnvelope::Send(SendResponseEnvelope::Sent(hostless_outcome)) = hostless_response
    else {
        panic!("hostless write must return sent");
    };

    assert_ne!(peer_receipt_id, hostless_outcome.message_id);

    let host_qualified_write = SendRequest::new(
        atm_home,
        workspace_dir,
        ROLE_TEAM_LEAD.parse().expect("local caller"),
        "remote-agent@remote-team.peer.example.test",
        TEST_TEAM.parse().expect("local team"),
        SendMessageSource::Inline("host-qualified write has no local fallback".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("host-qualified write");
    let _error = dispatcher
        .route(
            ApiRequest::Write(Box::new(host_qualified_write)),
            AuthenticatedIngress::Local,
            RequestDeadline::after(Duration::from_secs(1)),
        )
        .expect_err("unconfigured host remains an explicit outbound uncertainty");
}

#[test]
#[serial_test::serial(env)]
fn peer_message_array_commits_all_items_or_none_through_the_existing_router() {
    let (_tempdir, atm_home, workspace_dir, db_path, dispatcher) = peer_array_dispatcher();
    let valid_first = inbound_peer_write(
        atm_home.clone(),
        workspace_dir.clone(),
        "first peer array member".to_string(),
    );
    let valid_second = inbound_peer_write(
        atm_home.clone(),
        workspace_dir.clone(),
        "second peer array member".to_string(),
    );
    let first_id = valid_first.origin_message_id.expect("origin id");
    let second_id = valid_second.origin_message_id.expect("origin id");

    let response = dispatcher
        .route(
            ApiRequest::PeerMessages(Box::new(PeerMessageArray {
                messages: vec![valid_first, valid_second],
            })),
            AuthenticatedIngress::Peer,
            RequestDeadline::after(Duration::from_secs(1)),
        )
        .expect("complete peer array succeeds")
        .into_inner();
    assert!(matches!(
        response,
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) if outcome.message_id == first_id
    ));

    let message_store = open_sqlite_boundary(&db_path)
        .expect("open durable store")
        .message_store_arc();
    assert!(
        message_store
            .load_message(&MessageKey::from(first_id))
            .expect("load first")
            .is_some()
    );
    assert!(
        message_store
            .load_message(&MessageKey::from(second_id))
            .expect("load second")
            .is_some()
    );

    let valid = inbound_peer_write(
        atm_home.clone(),
        workspace_dir.clone(),
        "must not persist".to_string(),
    );
    let valid_id = valid.origin_message_id.expect("origin id");
    let mut invalid = inbound_peer_write(atm_home, workspace_dir, "invalid".to_string());
    let invalid_id = invalid.origin_message_id.expect("origin id");
    invalid.to = None;
    let error = dispatcher
        .route(
            ApiRequest::PeerMessages(Box::new(PeerMessageArray {
                messages: vec![valid, invalid],
            })),
            AuthenticatedIngress::Peer,
            RequestDeadline::after(Duration::from_secs(1)),
        )
        .expect_err("invalid array member rejects the complete request");
    assert!(error.is_validation());
    assert!(
        message_store
            .load_message(&MessageKey::from(valid_id))
            .expect("load rejected valid member")
            .is_none()
    );
    assert!(
        message_store
            .load_message(&MessageKey::from(invalid_id))
            .expect("load rejected invalid member")
            .is_none()
    );
}

#[test]
#[serial_test::serial(env)]
fn peer_message_array_post_commit_graft_failure_is_warning_only_after_admission() {
    let (_tempdir, atm_home, workspace_dir, db_path, dispatcher) = peer_array_graft_dispatcher();
    let team = TEST_TEAM.parse().expect("team");
    let recipient = ROLE_TEAM_LEAD.parse().expect("recipient");
    let receiver_path = graft_receiver_record_path_from_home(&workspace_dir, &team, &recipient);
    let listener = GraftReceiverListener::bind(&receiver_path, None).expect("bind graft receiver");
    let (event_tx, event_rx) = std::sync::mpsc::sync_channel(2);
    let receiver_thread = std::thread::spawn(move || {
        for _ in 0..2 {
            let mut stream = loop {
                if let Some(stream) = listener.poll_accept().expect("poll graft receiver") {
                    break stream;
                }
                std::thread::yield_now();
            };
            let request = listener
                .read_request(&mut stream, Duration::from_secs(5))
                .expect("read graft nudge");
            event_tx
                .send(request.event)
                .expect("record injected graft nudge");
            listener
                .write_response(
                    &mut stream,
                    &GraftPostSendResponse::Error(AtmError::new(
                        AtmErrorCode::PostSendGraftUnavailable,
                        "injected peer-array graft failure",
                    )),
                )
                .expect("return injected graft failure");
        }
    });

    let first = inbound_peer_write(
        atm_home.clone(),
        workspace_dir.clone(),
        "first warning-only peer receipt".to_string(),
    );
    let first_id = first.origin_message_id.expect("first origin id");
    let second = inbound_peer_write(
        atm_home,
        workspace_dir,
        "second warning-only peer receipt".to_string(),
    );
    let second_id = second.origin_message_id.expect("second origin id");

    let response = dispatcher
        .route(
            ApiRequest::PeerMessages(Box::new(PeerMessageArray {
                messages: vec![first, second],
            })),
            AuthenticatedIngress::Peer,
            RequestDeadline::after(Duration::from_secs(1)),
        )
        .expect("post-commit graft failure cannot fail peer-array admission")
        .into_inner();
    assert!(matches!(
        response,
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome))
            if outcome.message_id == first_id
                && outcome.warnings.len() == 2
                && outcome.warnings.iter().all(|warning| warning.code == Some(AtmErrorCode::PostSendGraftUnavailable))
    ));

    let message_store = open_sqlite_boundary(&db_path)
        .expect("open durable store")
        .message_store_arc();
    for message_id in [first_id, second_id] {
        assert!(
            message_store
                .load_message(&MessageKey::from(message_id))
                .expect("load admitted peer-array member")
                .is_some(),
            "post-commit warning cannot roll back a received peer-array member"
        );
    }
    for expected_description in [
        "first warning-only peer receipt",
        "second warning-only peer receipt",
    ] {
        let event = event_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("post-commit graft nudge was attempted after receive success");
        assert_eq!(event.description, expected_description);
    }
    receiver_thread
        .join()
        .expect("join injected-failure graft receiver");
}

#[test]
#[serial_test::serial(env)]
fn one_element_peer_acknowledgement_array_uses_the_canonical_atomic_ack_path() {
    let (_tempdir, atm_home, workspace_dir, _db_path, dispatcher) = peer_array_dispatcher();
    let mut source = inbound_peer_write(
        atm_home.clone(),
        workspace_dir.clone(),
        "requires acknowledgement".to_string(),
    );
    source.requires_ack = true;
    let source_id = source.origin_message_id.expect("source id");
    dispatcher
        .route(
            ApiRequest::Write(Box::new(source)),
            AuthenticatedIngress::Peer,
            RequestDeadline::after(Duration::from_secs(1)),
        )
        .expect("peer source write");

    let mut acknowledgement =
        inbound_peer_write(atm_home, workspace_dir, "acknowledged".to_string());
    acknowledgement.acknowledges_message_id = Some(source_id);
    let response = dispatcher
        .route(
            ApiRequest::PeerMessages(Box::new(PeerMessageArray {
                messages: vec![acknowledgement],
            })),
            AuthenticatedIngress::Peer,
            RequestDeadline::after(Duration::from_secs(1)),
        )
        .expect("one-element peer ACK array")
        .into_inner();
    assert!(matches!(
        response,
        ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome))
            if outcome.message_id == source_id
    ));
}

#[test]
#[serial_test::serial(env)]
fn multi_item_peer_message_array_rejects_acknowledgement_members_without_admission() {
    let (_tempdir, atm_home, workspace_dir, db_path, dispatcher) = peer_array_dispatcher();
    let ordinary = inbound_peer_write(
        atm_home.clone(),
        workspace_dir.clone(),
        "ordinary peer array member".to_string(),
    );
    let ordinary_id = ordinary.origin_message_id.expect("ordinary origin id");
    let mut acknowledgement = inbound_peer_write(
        atm_home,
        workspace_dir,
        "acknowledgement member".to_string(),
    );
    acknowledgement.acknowledges_message_id = Some(AtmMessageId::new());
    let acknowledgement_id = acknowledgement
        .origin_message_id
        .expect("acknowledgement origin id");

    let error = dispatcher
        .route(
            ApiRequest::PeerMessages(Box::new(PeerMessageArray {
                messages: vec![ordinary, acknowledgement],
            })),
            AuthenticatedIngress::Peer,
            RequestDeadline::after(Duration::from_secs(1)),
        )
        .expect_err("multi-item arrays cannot carry acknowledgement writes");
    assert!(error.is_validation());
    assert!(
        error
            .message()
            .contains("peer message arrays cannot contain acknowledgement writes")
    );

    let message_store = open_sqlite_boundary(&db_path)
        .expect("open durable store")
        .message_store_arc();
    for message_id in [ordinary_id, acknowledgement_id] {
        assert!(
            message_store
                .load_message(&MessageKey::from(message_id))
                .expect("load rejected peer-array member")
                .is_none(),
            "invalid multi-item acknowledgement arrays must not admit any member"
        );
    }
}
