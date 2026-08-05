use super::runtime_root::{
    add_member_via_retained_admin, configure_peer_http_source, configure_trusted_peer,
    remote_write_request, serve_direct_peer_responses,
};
use super::{TEST_TEAM, install_retained_runtime_factory, write_team_config};
use atm_core::api::HttpFrameReader;
use atm_core::error_codes::AtmErrorCode;
use atm_core::test_support::ROLE_TEAM_LEAD;
use atm_runtime_test_support::open_sqlite_boundary;
use atm_storage::{MessageKey, PeerResendCacheSetting, TrustedPeer};
use std::net::{Ipv6Addr, TcpListener};
use std::num::NonZeroU16;
use std::thread;
use tempfile::TempDir;

use crate::runtime_health::{DaemonRequestDispatcher, RuntimeStatusCache};

fn serve_dropped_peer_requests(request_count: usize) -> (NonZeroU16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).expect("peer listener");
    let port = NonZeroU16::new(listener.local_addr().expect("peer address").port())
        .expect("non-zero peer port");
    let server = thread::spawn(move || {
        for _ in 0..request_count {
            let (mut stream, _) = listener.accept().expect("peer accept");
            let _ = HttpFrameReader::new()
                .read_request(&mut stream)
                .expect("read peer request")
                .expect("one peer request");
        }
    });
    (port, server)
}

#[test]
#[serial_test::serial(env)]
fn resend_cache_reload_keeps_disabled_delivery_on_the_direct_no_retry_path() {
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
    let (port, server) = serve_dropped_peer_requests(1);
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
    assembly
        .peer_config_store()
        .save_peer_resend_cache_setting(PeerResendCacheSetting { enabled: false })
        .expect("disable cache");
    configure_peer_http_source(&db_path);
    let dispatcher = DaemonRequestDispatcher::new_for_test(
        atm_home.clone(),
        RuntimeStatusCache::new(),
        db_path.clone(),
    );

    let disabled_error = dispatcher
        .dispatch(remote_write_request(
            atm_home.clone(),
            workspace_dir.clone(),
            "disabled direct peer write".to_owned(),
        ))
        .expect_err("disabled caching returns the direct peer failure");
    assert_eq!(
        disabled_error.code(),
        AtmErrorCode::RemoteDeliveryUnconfirmed
    );
    assert_eq!(
        dispatcher.next_peer_resend_due(),
        None,
        "disabled caching creates neither an aggregate nor a due event"
    );

    assembly
        .peer_config_store()
        .save_peer_resend_cache_setting(PeerResendCacheSetting { enabled: true })
        .expect("enable cache");
    dispatcher
        .reload_runtime_view()
        .expect("reload enabled cache");
    let enabled_error = dispatcher
        .dispatch(remote_write_request(
            atm_home,
            workspace_dir,
            "enabled direct peer write".to_owned(),
        ))
        .expect_err("enabled cache leaves the new admission queued behind retained work");
    assert_eq!(
        enabled_error.code(),
        AtmErrorCode::RemoteDeliveryUnconfirmed
    );
    assert!(
        dispatcher.next_peer_resend_due().is_some(),
        "enabled reload bootstraps retained work into one later recovery attempt"
    );
    server.join().expect("peer server");
}

#[test]
#[serial_test::serial(env)]
fn disabled_cache_success_confirms_one_peer_array_without_recovery_work() {
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
    assembly
        .peer_config_store()
        .save_peer_resend_cache_setting(PeerResendCacheSetting { enabled: false })
        .expect("disable cache");
    configure_peer_http_source(&db_path);
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), RuntimeStatusCache::new(), db_path);

    let response = dispatcher
        .dispatch(remote_write_request(
            atm_home,
            workspace_dir,
            "cache-disabled successful peer write".to_owned(),
        ))
        .expect("direct peer write succeeds after one peer array response");
    let atm_core::protocol::ResponseEnvelope::Send(atm_core::protocol::SendResponseEnvelope::Sent(
        outcome,
    )) = response
    else {
        panic!("cache-disabled direct peer write returns sent");
    };
    assert_eq!(
        dispatcher.next_peer_resend_due(),
        None,
        "the direct path does not create recovery work or a sender-side local nudge"
    );
    let stored = assembly
        .message_store_arc()
        .load_message(&MessageKey::from(outcome.message_id))
        .expect("load durable origin write")
        .expect("origin write remains durable");
    assert!(
        !stored.envelope.extra.contains_key("peerOutbound"),
        "the one successful messages[] response atomically retires the outbound marker"
    );
    server.join().expect("peer server");
}
