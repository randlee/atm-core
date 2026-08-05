use super::*;
use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::protocol::{
    PeerSyncDisposition, PeerSyncOutcome, PeerSyncRequest, RequestEnvelope, ResponseEnvelope,
    SendResponseEnvelope,
};
use atm_core::read::ReadQuery;
use atm_core::send::{SendMessageSource, SendRequest, WriteRequest};
use atm_core::team_admin::{AddMemberRequest, add_member_with_roster_store};
use atm_core::test_support::{EnvGuard, ROLE_TEAM_LEAD};
use atm_core::types::ReadSelection;
use atm_core::{ApiRequest, ApiRouter, AuthenticatedIngress, RequestDeadline};
use atm_runtime_test_support::{SQLITE_RUNTIME_PATH_ENV, open_sqlite_boundary};
use atm_storage::{PeerSyncPolicy, TrustedPeer};
use std::num::NonZeroU16;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;
use tempfile::TempDir;

use crate::https_transport::HttpsMessageTransport;
mod local_ipc;
mod peer_failure;
mod peer_observability;
mod peer_reconciliation;
use crate::test_support::{
    configure_test_local_ipc_timeouts, connect_daemon_local_ipc_until_ready,
    write_test_local_ipc_request,
};

#[derive(Default)]
struct RecordingHttpsDelivery {
    delivered: std::sync::Mutex<Vec<WriteRequest>>,
    remaining_budgets: std::sync::Mutex<Vec<Duration>>,
}

#[test]
#[serial_test::serial(env)]
fn local_write_uses_post_write_router_without_peer_delivery() {
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
    let transport = Arc::new(RecordingHttpsDelivery::default());
    dispatcher
        .install_https_transport(transport.clone())
        .expect("install test HTTPS delivery");

    let response = dispatcher
        .dispatch(RequestEnvelope::Write(Box::new(
            SendRequest::new(
                atm_home,
                workspace_dir,
                ROLE_TEAM_LEAD.parse().expect("caller"),
                "local-recipient@test-team",
                TEST_TEAM.parse().expect("team"),
                SendMessageSource::Inline("local write".to_string()),
                None,
                false,
                None,
                false,
            )
            .expect("local write request"),
        )))
        .expect("local write must complete through the shared writer/router");
    assert!(matches!(
        response,
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(_))
    ));
    assert!(
        transport
            .delivered
            .lock()
            .expect("HTTPS delivery recording lock")
            .is_empty(),
        "the post-write router must choose local delivery for an unqualified destination"
    );
}

impl HttpsMessageTransport for RecordingHttpsDelivery {
    fn deliver(
        &self,
        request: WriteRequest,
        _peer: &TrustedPeer,
        deadline: RequestDeadline,
    ) -> Result<ResponseEnvelope, AtmError> {
        self.delivered
            .lock()
            .expect("peer transport recording lock")
            .push(request);
        self.remaining_budgets
            .lock()
            .expect("peer transport deadline recording lock")
            .push(deadline.remaining().unwrap_or(Duration::ZERO));
        Ok(ResponseEnvelope::CompatibilityVerdict(
            atm_core::protocol::CompatibilityVerdict::Compatible {
                daemon_release: atm_core::protocol::ReleaseVersion::current(),
                daemon_schema_version: atm_core::protocol::CLI_SCHEMA_VERSION,
                daemon_http_api_version: atm_core::protocol::HttpApiVersion::current(),
            },
        ))
    }
}

#[derive(Default)]
struct ConnectionHandlerFailure {
    attempted: std::sync::Mutex<Vec<WriteRequest>>,
}

#[derive(Default)]
struct RouteFailure {
    attempted: std::sync::Mutex<Vec<WriteRequest>>,
    attempted_tx: std::sync::Mutex<Option<mpsc::SyncSender<WriteRequest>>>,
}

#[derive(Default)]
struct ResponseWriteFailure {
    attempted: std::sync::Mutex<Vec<WriteRequest>>,
}

fn record_failed_delivery(attempted: &std::sync::Mutex<Vec<WriteRequest>>, request: WriteRequest) {
    attempted
        .lock()
        .expect("peer transport recording lock")
        .push(request);
}

impl HttpsMessageTransport for ConnectionHandlerFailure {
    fn deliver(
        &self,
        request: WriteRequest,
        _peer: &TrustedPeer,
        _deadline: RequestDeadline,
    ) -> Result<ResponseEnvelope, AtmError> {
        record_failed_delivery(&self.attempted, request);
        Err(AtmError::daemon_unavailable(
            "intentional connection-handler failure",
        ))
    }
}

impl HttpsMessageTransport for RouteFailure {
    fn deliver(
        &self,
        request: WriteRequest,
        _peer: &TrustedPeer,
        _deadline: RequestDeadline,
    ) -> Result<ResponseEnvelope, AtmError> {
        record_failed_delivery(&self.attempted, request.clone());
        if let Some(sender) = self
            .attempted_tx
            .lock()
            .expect("peer transport notification lock")
            .take()
        {
            sender.send(request).expect("report failed peer delivery");
        }
        Err(AtmError::remote_delivery_unconfirmed(
            "intentional route failure",
        ))
    }
}

impl HttpsMessageTransport for ResponseWriteFailure {
    fn deliver(
        &self,
        request: WriteRequest,
        _peer: &TrustedPeer,
        _deadline: RequestDeadline,
    ) -> Result<ResponseEnvelope, AtmError> {
        record_failed_delivery(&self.attempted, request);
        Err(AtmError::remote_delivery_unconfirmed(
            "intentional response-write failure",
        ))
    }
}

struct RejectingHttpsDelivery;

impl HttpsMessageTransport for RejectingHttpsDelivery {
    fn deliver(
        &self,
        _request: WriteRequest,
        _peer: &TrustedPeer,
        _deadline: RequestDeadline,
    ) -> Result<ResponseEnvelope, AtmError> {
        Ok(ResponseEnvelope::Error(AtmError::validation(
            "remote roster rejected the recipient",
        )))
    }
}

struct BlockingPeerDelivery {
    started: mpsc::SyncSender<()>,
    release: std::sync::Mutex<mpsc::Receiver<()>>,
}

impl HttpsMessageTransport for BlockingPeerDelivery {
    fn deliver(
        &self,
        _request: WriteRequest,
        _peer: &TrustedPeer,
        _deadline: RequestDeadline,
    ) -> Result<ResponseEnvelope, AtmError> {
        self.started.send(()).expect("report blocked peer worker");
        self.release
            .lock()
            .expect("release lock")
            .recv()
            .expect("release blocked peer worker");
        Ok(ResponseEnvelope::CompatibilityVerdict(
            atm_core::protocol::CompatibilityVerdict::Compatible {
                daemon_release: atm_core::protocol::ReleaseVersion::current(),
                daemon_schema_version: atm_core::protocol::CLI_SCHEMA_VERSION,
                daemon_http_api_version: atm_core::protocol::HttpApiVersion::current(),
            },
        ))
    }
}

#[test]
#[serial_test::serial(env)]
fn local_admission_returns_before_the_post_commit_peer_worker_is_released() {
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
    peer_store
        .save_trusted_peer(&TrustedPeer {
            host: "peer.example.test".parse().expect("peer host"),
            fingerprint: "sha256:test-peer".parse().expect("fingerprint"),
            enabled: true,
            https_port: NonZeroU16::new(43101).expect("non-zero"),
        })
        .expect("save trusted peer");
    let peer_host = "peer.example.test".parse().expect("peer host");
    peer_store
        .save_peer_sync_policy(
            &peer_host,
            PeerSyncPolicy {
                max_message_age: Duration::from_secs(60),
                max_batch_messages: NonZeroU16::new(1).expect("non-zero batch"),
            },
        )
        .expect("enable peer recovery for the blocked-worker test");

    let dispatcher = Arc::new(DaemonRequestDispatcher::new_for_test(
        atm_home.clone(),
        RuntimeStatusCache::new(),
        db_path,
    ));
    let (started, started_rx) = mpsc::sync_channel(1);
    let (release, release_rx) = mpsc::sync_channel(1);
    dispatcher
        .install_https_transport(Arc::new(BlockingPeerDelivery {
            started,
            release: std::sync::Mutex::new(release_rx),
        }))
        .expect("install blocked peer worker");
    dispatcher
        .start_peer_drain_coordinator()
        .expect("start post-commit peer worker");

    let (response_tx, response_rx) = mpsc::sync_channel(1);
    let admission_dispatcher = Arc::clone(&dispatcher);
    let admission = std::thread::spawn(move || {
        let response = admission_dispatcher.dispatch(RequestEnvelope::Write(Box::new(
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
        )));
        response_tx.send(response).expect("report admission result");
    });

    let response = response_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("local admission must not wait for the blocked peer worker")
        .expect("local admission response");
    assert!(matches!(
        response,
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(_))
    ));
    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("post-commit worker must eventually start peer delivery");
    release.send(()).expect("release peer worker");
    admission.join().expect("join local admission");
    dispatcher
        .stop_peer_drain_coordinator()
        .expect("stop post-commit peer worker");
}

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

#[test]
#[serial_test::serial(env)]
fn host_qualified_write_is_admitted_without_foreground_https_delivery() {
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
    peer_store
        .save_trusted_peer(&TrustedPeer {
            host: "peer.example.test".parse().expect("peer host"),
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
        .expect("local admission must succeed without waiting for peer delivery");
    assert!(matches!(
        response,
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(_))
    ));
    let delivered = transport
        .delivered
        .lock()
        .expect("HTTPS delivery recording lock");
    assert!(
        delivered.is_empty(),
        "the admission path must not open a peer transport before its local response"
    );
}

#[test]
#[serial_test::serial(env)]
fn peer_rejection_does_not_prevent_local_admission() {
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
            https_port: std::num::NonZeroU16::new(43101).expect("non-zero"),
        })
        .expect("save trusted peer");
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), RuntimeStatusCache::new(), db_path);
    dispatcher
        .install_https_transport(Arc::new(RejectingHttpsDelivery))
        .expect("install rejecting transport");

    let response = dispatcher
        .dispatch(RequestEnvelope::Write(Box::new(
            SendRequest::new(
                atm_home,
                workspace_dir,
                ROLE_TEAM_LEAD.parse().expect("caller"),
                "unknown@remote-team.peer.example.test",
                TEST_TEAM.parse().expect("team"),
                SendMessageSource::Inline("peer write".to_string()),
                None,
                false,
                None,
                false,
            )
            .expect("remote write request"),
        )))
        .expect("remote rejection must be handled after the local response");
    assert!(matches!(
        response,
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(_))
    ));
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
    let transport = Arc::new(RecordingHttpsDelivery::default());
    dispatcher
        .install_https_transport(transport.clone())
        .expect("install test HTTPS delivery");
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
    assert!(
        transport
            .delivered
            .lock()
            .expect("HTTPS delivery recording lock")
            .is_empty(),
        "the receiving peer write has no destination host and must not re-forward"
    );

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
    assert!(
        transport
            .delivered
            .lock()
            .expect("HTTPS delivery recording lock")
            .is_empty(),
        "a peer replay must not create a reverse peer delivery"
    );
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
    let peer_store = open_sqlite_boundary(&db_path)
        .expect("sqlite boundary")
        .peer_config_store();
    for host in ["localhost", "127.0.0.1"] {
        peer_store
            .save_trusted_peer(&TrustedPeer {
                host: host.parse().expect("host"),
                fingerprint: "sha256:test-peer".parse().expect("fingerprint"),
                enabled: true,
                https_port: NonZeroU16::new(43101).expect("non-zero"),
            })
            .expect("save trusted peer");
    }
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
    let status_cache = RuntimeStatusCache::new();
    let dispatcher: Arc<dyn ApiRouter + Send + Sync> =
        Arc::new(DaemonRequestDispatcher::new_for_test(
            atm_home.clone(),
            status_cache.clone(),
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
        .expect("send request")
        .with_activity_observation(Some(atm_core::caller_context::ActivityObservation {
            team: TEST_TEAM.parse().expect("team"),
            member: ROLE_TEAM_LEAD.parse().expect("member"),
            session_id: Some(
                atm_core::types::SessionId::new("transport-session").expect("session"),
            ),
            pid: Some(42),
        })),
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
    assert_eq!(
        status_cache.cached_session_id(test_team(), &ROLE_TEAM_LEAD.parse().expect("member")),
        Some(atm_core::types::SessionId::new("transport-session").expect("session")),
    );

    #[cfg(windows)]
    let record_path = atm_daemon_client::resolve_daemon_local_ipc_endpoint()
        .expect("resolve loopback endpoint record")
        .as_ref()
        .to_path_buf();
    #[cfg(not(windows))]
    let record_path = socket_path
        .parent()
        .expect("socket path parent")
        .join(atm_core::local_http::LOCAL_HTTP_RECORD_FILENAME);
    let record: atm_core::local_http::LocalHttpEndpointRecord =
        serde_json::from_slice(&std::fs::read(record_path).expect("read loopback endpoint record"))
            .expect("parse loopback endpoint record");
    let capability = record
        .capability()
        .expect("local capability")
        .to_base64url();
    let mut tcp_stream = std::net::TcpStream::connect(record.ipv4_loopback.expect("loopback"))
        .expect("connect loopback TCP");
    atm_core::api::write_http_request_with_headers(
        &mut tcp_stream,
        &request,
        &[(
            atm_core::local_http::LOCAL_CAPABILITY_HEADER,
            capability.as_str(),
        )],
    )
    .expect("write TCP send request");
    let tcp_response = atm_core::api::read_http_response(&mut tcp_stream, &request)
        .expect("read TCP send response");
    assert!(matches!(tcp_response, ResponseEnvelope::Send(_)));
    assert_eq!(
        status_cache.cached_session_id(test_team(), &ROLE_TEAM_LEAD.parse().expect("member")),
        Some(atm_core::types::SessionId::new("transport-session").expect("session")),
    );

    lifecycle.set_terminate_for_test(true);
    serve_result_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("recv serve result")
        .expect("serve runtime result");
    join.join().expect("join serve thread");
}
