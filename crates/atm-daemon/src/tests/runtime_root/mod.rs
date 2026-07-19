use super::*;
use atm_core::boundary::{AtmProtocol, RequestDispatcher};
use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::protocol::{
    JsonAtmProtocolCodec, RequestEnvelope, ResponseEnvelope, SendResponseEnvelope, next_request_id,
};
use atm_core::read::ReadQuery;
use atm_core::send::{SendMessageSource, SendRequest};
use atm_core::team_admin::{AddMemberRequest, add_member_with_roster_store};
use atm_core::test_support::{EnvGuard, ROLE_TEAM_LEAD};
use atm_core::types::ReadSelection;
use atm_runtime_test_support::{SQLITE_RUNTIME_PATH_ENV, open_sqlite_boundary};
use atm_storage::{PeerSecurityMode, SetPeerSecurityModeCommand, UpsertTrustedPeerCommand};
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

pub(super) fn canonical_ack_request(
    home_dir: &std::path::Path,
    current_dir: &std::path::Path,
    caller_identity: &str,
    caller_team: &str,
    message_id: atm_core::schema::AtmMessageId,
    body: &str,
) -> RequestEnvelope {
    let mut request = SendRequest::new(
        home_dir.to_path_buf(),
        current_dir.to_path_buf(),
        caller_identity.parse().expect("caller identity"),
        &format!("{caller_identity}@{caller_team}"),
        caller_team.parse().expect("caller team"),
        SendMessageSource::Inline(body.to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("ack send request");
    request.acknowledges_message_id = Some(message_id);
    RequestEnvelope::Send(Box::new(request))
}
use tempfile::TempDir;

use crate::test_support::{
    configure_test_local_ipc_timeouts, connect_daemon_local_ipc_until_ready,
};

mod cross_host;
mod dispatch;
mod local_ipc;
mod loopback;
mod self_ip;

pub(super) fn add_member_via_retained_admin(
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

pub(super) fn configure_secure_loopback(db_path: &std::path::Path, host: &str) {
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

pub(super) fn discover_non_loopback_ipv4_for_test() -> Ipv4Addr {
    let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
        .expect("bind udp socket for self-ip discovery");
    socket
        .connect(SocketAddr::from(([198, 51, 100, 1], 9)))
        .expect("connect udp socket for self-ip discovery");
    match socket
        .local_addr()
        .expect("local addr for self-ip discovery")
        .ip()
    {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => ip,
        other => panic!("expected non-loopback local IPv4 for self-ip fixture, got {other}"),
    }
}
