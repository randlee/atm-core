use super::super::runtime_health::{
    DaemonRequestDispatcher, PostCommitWorkKey, PostCommitWorkQueue, RuntimeStatusCache,
};
use super::{TEST_TEAM, install_retained_runtime_factory, write_team_config};
use atm_core::api::PeerMessageArray;
use atm_core::protocol::{ResponseEnvelope, SendResponseEnvelope};
use atm_core::send::{SendMessageSource, SendRequest};
use atm_core::test_support::ROLE_TEAM_LEAD;
use atm_core::{ApiRequest, ApiRouter, AuthenticatedIngress, RequestDeadline};
use atm_runtime_test_support::open_sqlite_boundary;
use atm_storage::{AtmMessageId, MessageKey};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;

use super::runtime_root::add_member_via_retained_admin;

#[derive(Default)]
struct RecordingPostCommitWorkQueue {
    signals: Mutex<Vec<PostCommitWorkKey>>,
}

impl RecordingPostCommitWorkQueue {
    fn signals(&self) -> Vec<PostCommitWorkKey> {
        self.signals.lock().expect("recorded signals").clone()
    }
}

impl PostCommitWorkQueue for RecordingPostCommitWorkQueue {
    fn signal(&self, work: PostCommitWorkKey) {
        self.signals
            .lock()
            .expect("record post-commit signal")
            .push(work);
    }
}

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

#[test]
#[serial_test::serial(env)]
fn post_write_router_runtime_selects_local_signals_and_never_falls_back_for_hosts() {
    let (_tempdir, atm_home, workspace_dir, _db_path, mut dispatcher) = peer_array_dispatcher();
    let queue = Arc::new(RecordingPostCommitWorkQueue::default());
    dispatcher.replace_post_commit_work_queue_for_test(queue.clone());

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

    assert_eq!(
        queue.signals(),
        vec![
            PostCommitWorkKey::LocalNudge(peer_receipt_id),
            PostCommitWorkKey::LocalNudge(hostless_outcome.message_id),
        ],
        "peer receipts and hostless origins select ordinary local post-write work"
    );

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
    assert_eq!(
        queue.signals().len(),
        2,
        "a host-qualified write must never fall back to a local post-write signal"
    );
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
