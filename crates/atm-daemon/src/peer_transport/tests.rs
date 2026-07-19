use super::{
    PEER_REQUEST_DEADLINE, PeerClientTransport, PeerTransportConfig, PeerTransportRuntime,
};
use crate::lifecycle_control::LifecycleControlSourceAdapter;
use crate::runtime_health::DaemonRequestDispatcher;
use crate::runtime_status_cache::RuntimeStatusCache;
use crate::test_support::DoctorOnlyDispatcher;
use crate::test_support::LifecycleFlagResetGuard;
use crate::{DaemonSubsystem, SubsystemObservability};
use atm_core::boundary::{
    AtmProtocol, ClientTransport, MessageKey, ReplaySource, RequestDispatcher, RosterHarness,
};
use atm_core::doctor::DoctorQuery;
use atm_core::error::AtmErrorCode;
use atm_core::protocol::{
    HeartbeatActivity, ProtocolErrorEnvelope, RequestEnvelope, ResponseEnvelope,
    RuntimeMemberState, RuntimeReadinessState, TeamMemberHeartbeatRequest,
    TeamMemberHeartbeatResponse,
};
use atm_core::read::ReadQuery;
use atm_core::schema::AgentMember;
use atm_core::send::{SendMessageSource, SendRequest};
use atm_core::test_support::ROLE_TEAM_LEAD;
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use atm_runtime_test_support::{install_sqlite_retained_runtime_factory, open_sqlite_boundary};
use atm_storage::{PeerSecurityMode, SetPeerSecurityModeCommand, UpsertTrustedPeerCommand};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

#[path = "tests/authorization.rs"]
mod authorization;
#[path = "tests/harness.rs"]
mod harness;
#[path = "tests/peer_listener.rs"]
mod peer_listener;
#[path = "tests_transport.rs"]
mod tests_transport;
#[path = "tests/transport_client.rs"]
mod transport_client;

fn test_team_name() -> TeamName {
    atm_core::test_support::TEST_TEAM.parse().expect("team")
}

fn test_sender_name() -> AgentName {
    atm_core::test_support::TEST_SENDER.parse().expect("member")
}

fn test_recipient_name() -> AgentName {
    atm_core::test_support::TEST_RECIPIENT
        .parse()
        .expect("member")
}

fn test_sender_identity() -> String {
    format!(
        "{}@{}",
        atm_core::test_support::TEST_SENDER,
        atm_core::test_support::TEST_TEAM
    )
}

fn install_retained_runtime_factory() {
    install_sqlite_retained_runtime_factory();
}

fn write_workspace_config(workspace_dir: &std::path::Path) {
    std::fs::write(workspace_dir.join(".atm.toml"), "[atm]\n").expect("workspace config");
}

fn write_team_config(home_dir: &std::path::Path, members: &[&str]) {
    let team_dir = home_dir
        .join(".claude")
        .join("teams")
        .join(atm_core::test_support::TEST_TEAM);
    std::fs::create_dir_all(team_dir.join("inboxes")).expect("inboxes dir");
    let config = atm_core::schema::TeamConfig {
        members: members
            .iter()
            .map(|name| AgentMember::with_name((*name).parse().expect("member")))
            .collect(),
        ..Default::default()
    };
    std::fs::write(
        team_dir.join("config.json"),
        serde_json::to_vec(&config).expect("team config"),
    )
    .expect("write team config");
}

fn replay_source_static(label: &'static str) -> ReplaySource {
    ReplaySource::new(label).unwrap_or_else(|_| unreachable!("static replay source must validate"))
}

fn install_test_roster_with_harness(
    db_path: &std::path::Path,
    members: &[(&str, RosterHarness, &std::path::Path)],
) {
    let assembly = open_sqlite_boundary(db_path).expect("assemble boundary");
    let roster_store = assembly.roster_store_arc();
    let team = test_team_name();
    let members = members
        .iter()
        .map(|(name, harness, home_dir)| {
            let mut member = AgentMember::with_name((*name).parse().expect("member"));
            member.home_dir = (*home_dir).to_path_buf().into();
            let mut record = atm_core::boundary::roster_member_record_from_claude_code_member(
                team.clone(),
                member,
            );
            record.harness = *harness;
            record
        })
        .collect::<Vec<_>>();
    roster_store
        .replace_roster(
            &team,
            &members,
            Some(&replay_source_static("peer-transport-ag7-test")),
        )
        .expect("replace roster");
}

fn read_request_frame(
    stream: &mut TcpStream,
) -> (
    atm_core::protocol::RequestId,
    RequestEnvelope,
    atm_core::protocol::JsonAtmProtocolCodec,
) {
    let codec = atm_core::protocol::JsonAtmProtocolCodec;
    let frame = atm_core::protocol::read_frame(
        stream,
        "read request",
        "request frame exceeded frame limit",
    )
    .expect("read frame")
    .expect("request frame");
    let (request_id, request) = codec.request_from_frame(frame).expect("decode request");
    (request_id, request, codec)
}

fn write_response_frame(
    stream: &mut TcpStream,
    codec: &atm_core::protocol::JsonAtmProtocolCodec,
    request_id: atm_core::protocol::RequestId,
    response: ResponseEnvelope,
) {
    let frame = codec
        .response_to_frame(request_id, response)
        .expect("response frame");
    atm_core::protocol::write_frame(stream, &frame, "write response").expect("write response");
    stream.flush().expect("flush response");
}

fn install_shared_lifecycle_reset_guard() -> LifecycleFlagResetGuard {
    let lifecycle = LifecycleControlSourceAdapter::install().expect("install lifecycle");
    LifecycleFlagResetGuard::install(lifecycle)
}

fn configure_secure_mode_and_trust_self(
    store: &Arc<dyn atm_storage::PeerSecurityStore + Send + Sync>,
    host: &str,
) {
    store
        .set_security_mode(
            SetPeerSecurityModeCommand::new(
                PeerSecurityMode::SecureRequired,
                format!(
                    "{}@{}",
                    atm_core::test_support::TEST_SENDER,
                    atm_core::test_support::TEST_TEAM
                ),
            )
            .expect("security mode command"),
        )
        .expect("set security mode");
    let identity = store
        .load_or_create_local_identity()
        .expect("load or create local identity");
    store
        .upsert_trusted_peer(
            UpsertTrustedPeerCommand::new(
                host,
                identity.fingerprint_sha256().to_string(),
                Some("loopback".to_string()),
                format!(
                    "{}@{}",
                    atm_core::test_support::TEST_SENDER,
                    atm_core::test_support::TEST_TEAM
                ),
            )
            .expect("trusted peer command"),
        )
        .expect("approve trusted peer");
}

#[derive(Debug)]
struct SleepingDispatcher {
    sleep_for: Duration,
}

impl atm_core::boundary::sealed::Sealed for SleepingDispatcher {}

impl RequestDispatcher for SleepingDispatcher {
    fn dispatch(
        &self,
        _request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, atm_core::error::AtmError> {
        thread::sleep(self.sleep_for);
        Ok(ResponseEnvelope::Heartbeat(TeamMemberHeartbeatResponse {
            team: atm_core::test_support::TEST_TEAM.parse().expect("team"),
            member: atm_core::test_support::TEST_SENDER.parse().expect("member"),
            pid: 7,
            pid_changed: false,
            state: RuntimeMemberState::Idle,
            last_active_at: Some(IsoTimestamp::now()),
        }))
    }
}

#[derive(Debug, Default)]
struct CountingDispatcher {
    count: AtomicUsize,
}

impl atm_core::boundary::sealed::Sealed for CountingDispatcher {}

impl RequestDispatcher for CountingDispatcher {
    fn dispatch(
        &self,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, atm_core::error::AtmError> {
        self.count.fetch_add(1, Ordering::SeqCst);
        DoctorOnlyDispatcher.dispatch(request)
    }
}
