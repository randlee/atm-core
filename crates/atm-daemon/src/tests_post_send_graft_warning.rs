use atm_core::boundary::{ReplaySource, RequestDispatcher, RosterHarness};
use atm_core::error_codes::AtmErrorCode;
use atm_core::graft::{
    GraftPostSendRequest, GraftPostSendResponse, graft_receiver_socket_path_from_home,
    read_graft_post_send_message, write_graft_post_send_message,
};
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, SendResponseEnvelope};
use atm_core::schema::{AgentMember, TeamConfig};
use atm_core::send::{SendMessageSource, SendRequest};
use atm_core::test_support::{EnvGuard, ROLE_TEAM_LEAD};
use atm_core::types::{AgentName, TeamName};
use atm_runtime_test_support::open_sqlite_boundary;
use interprocess::local_socket::ListenerOptions;
use interprocess::local_socket::traits::Listener as _;
use tempfile::TempDir;

use crate::runtime_health::{DaemonRequestDispatcher, RuntimeStatusCache};

const TEST_TEAM: &str = "test-team";

fn canonical_ack_request(
    home_dir: std::path::PathBuf,
    current_dir: std::path::PathBuf,
    caller_identity: &str,
    caller_team: &str,
    message_id: atm_core::schema::AtmMessageId,
    body: &str,
) -> RequestEnvelope {
    let mut request = SendRequest::new(
        home_dir,
        current_dir,
        caller_identity.parse().expect("caller"),
        &format!("{caller_identity}@{caller_team}"),
        caller_team.parse().expect("team"),
        SendMessageSource::Inline(body.to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("ack request");
    request.acknowledges_message_id = Some(message_id);
    RequestEnvelope::Send(Box::new(request))
}

fn replay_source_static(label: &'static str) -> ReplaySource {
    ReplaySource::new(label).unwrap_or_else(|_| unreachable!("static replay source must validate"))
}

fn expect_sent_response(response: ResponseEnvelope) -> atm_core::send::SendOutcome {
    match response {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => outcome,
        other => panic!("expected send response, got {other:?}"),
    }
}

fn expect_ack_response(response: ResponseEnvelope) -> atm_core::ack::AckOutcome {
    match response {
        ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome)) => outcome,
        other => panic!("expected ack response, got {other:?}"),
    }
}

fn install_test_roster_with_harness(
    db_path: &std::path::Path,
    members: &[(&str, RosterHarness, Option<&std::path::Path>)],
) {
    let assembly = open_sqlite_boundary(db_path).expect("assemble boundary");
    let roster_store = assembly.roster_store_arc();
    let team = TEST_TEAM.parse::<TeamName>().expect("team");
    let members = members
        .iter()
        .map(|(name, harness, home_dir)| {
            let mut member = AgentMember::with_name((*name).parse().expect("member"));
            if let Some(home_dir) = home_dir {
                member.home_dir = home_dir.to_path_buf().into();
            }
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
            Some(&replay_source_static("daemon-graft-warning-test")),
        )
        .expect("replace roster");
}

fn write_team_config(home_dir: &std::path::Path, members: &[&str]) {
    let team_dir = home_dir.join(".claude").join("teams").join(TEST_TEAM);
    std::fs::create_dir_all(team_dir.join("inboxes")).expect("inboxes dir");
    let config = TeamConfig {
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

fn graft_warning_dispatcher() -> (
    TempDir,
    std::path::PathBuf,
    std::path::PathBuf,
    DaemonRequestDispatcher,
) {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    let workspace_dir = tempdir.path().join("workspace");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir");
    let db_path = tempdir.path().join("mail.db");
    install_test_roster_with_harness(
        &db_path,
        &[
            (
                ROLE_TEAM_LEAD,
                RosterHarness::ClaudeCode,
                Some(workspace_dir.as_path()),
            ),
            (
                "qa-a",
                RosterHarness::CodexCli,
                Some(workspace_dir.as_path()),
            ),
        ],
    );
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD, "qa-a"]);
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), RuntimeStatusCache::new(), db_path);
    (tempdir, atm_home, workspace_dir, dispatcher)
}

fn write_graft_enabled_config(workspace_dir: &std::path::Path) {
    std::fs::write(
        workspace_dir.join(".atm.toml"),
        "[atm.graft]\nenabled = true\n",
    )
    .expect("write graft config");
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_send_surfaces_typed_warning_when_graft_receiver_path_is_unavailable() {
    let (_tempdir, atm_home, workspace_dir, dispatcher) = graft_warning_dispatcher();

    let response = dispatcher
        .dispatch(RequestEnvelope::Send(Box::new(
            SendRequest::new(
                atm_home.clone(),
                workspace_dir,
                ROLE_TEAM_LEAD.parse().expect("caller"),
                "qa-a@test-team",
                TEST_TEAM.parse().expect("team"),
                SendMessageSource::Inline("hello graft".to_string()),
                None,
                false,
                None,
                false,
            )
            .expect("send request"),
        )))
        .expect("send response");
    let outcome = expect_sent_response(response);
    assert_eq!(outcome.warnings.len(), 1);
    assert_eq!(
        outcome.warnings[0].code,
        Some(AtmErrorCode::PostSendGraftUnavailable)
    );
    assert!(outcome.warnings[0].recovery.is_some());
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_ack_surfaces_typed_warning_when_graft_reply_target_is_unavailable() {
    let (_tempdir, atm_home, workspace_dir, dispatcher) = graft_warning_dispatcher();

    let source_response = dispatcher
        .dispatch(RequestEnvelope::Send(Box::new(
            SendRequest::new(
                atm_home.clone(),
                workspace_dir.clone(),
                "qa-a".parse().expect("caller"),
                &format!("{ROLE_TEAM_LEAD}@{TEST_TEAM}"),
                TEST_TEAM.parse().expect("team"),
                SendMessageSource::Inline("please ack".to_string()),
                None,
                true,
                None,
                false,
            )
            .expect("source send request"),
        )))
        .expect("source send response");
    let source_message_id = expect_sent_response(source_response).message_id;

    let ack_response = dispatcher
        .dispatch(canonical_ack_request(
            atm_home,
            workspace_dir,
            ROLE_TEAM_LEAD,
            TEST_TEAM,
            source_message_id,
            "ack reply",
        ))
        .expect("ack response");
    let ack_outcome = expect_ack_response(ack_response);
    assert_eq!(ack_outcome.warnings.len(), 1);
    assert_eq!(
        ack_outcome.warnings[0].code,
        Some(AtmErrorCode::PostSendGraftUnavailable)
    );
    assert!(ack_outcome.warnings[0].recovery.is_some());
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_send_delivers_direct_graft_nudge_without_warning() {
    let (_tempdir, atm_home, workspace_dir, dispatcher) = graft_warning_dispatcher();
    write_graft_enabled_config(&workspace_dir);
    let recipient_team = TEST_TEAM.parse::<TeamName>().expect("team");
    let recipient_agent = "qa-a".parse::<AgentName>().expect("agent");
    let receiver_path =
        graft_receiver_socket_path_from_home(&workspace_dir, &recipient_team, &recipient_agent);
    if let Some(parent) = receiver_path.parent() {
        std::fs::create_dir_all(parent).expect("receiver dir");
    }
    #[cfg(unix)]
    if receiver_path.exists() {
        std::fs::remove_file(&receiver_path).expect("remove stale receiver");
    }
    let receiver_name =
        atm_core::protocol::daemon_local_ipc_name_from_path(&receiver_path).expect("receiver name");
    let listener = ListenerOptions::new()
        .name(receiver_name)
        .create_sync()
        .expect("bind fake graft receiver");
    let (event_tx, event_rx) = std::sync::mpsc::sync_channel(1);
    let receiver_thread = std::thread::spawn(move || {
        let mut stream = listener.accept().expect("accept graft receiver");
        let request: GraftPostSendRequest = read_graft_post_send_message(
            &mut stream,
            "failed to read graft post-send request",
            "graft post-send request exceeded the bounded payload cap",
        )
        .expect("read graft request");
        event_tx
            .send(request.event.clone())
            .expect("send captured event");
        write_graft_post_send_message(
            &mut stream,
            &GraftPostSendResponse::Delivered,
            "failed to write graft post-send response",
            "graft post-send response exceeded the bounded payload cap",
        )
        .expect("write graft response");
        use std::io::Write as _;
        stream.flush().expect("flush graft response");
    });
    let _env = EnvGuard::set_many([
        ("ATM_HOME", Some(atm_home.to_str().expect("utf8 atm home"))),
        ("HOME", Some(atm_home.to_str().expect("utf8 home"))),
        ("USERPROFILE", None),
    ]);
    let response = dispatcher
        .dispatch(RequestEnvelope::Send(Box::new(
            SendRequest::new(
                atm_home,
                workspace_dir,
                ROLE_TEAM_LEAD.parse().expect("caller"),
                "qa-a@test-team",
                TEST_TEAM.parse().expect("team"),
                SendMessageSource::Inline("hello graft".to_string()),
                None,
                false,
                None,
                false,
            )
            .expect("send request"),
        )))
        .expect("send response");
    let response = expect_sent_response(response);

    assert!(response.warnings.is_empty());
    let nudge = event_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("receive graft nudge");
    assert_eq!(nudge.recipient.as_str(), "qa-a");
    assert_eq!(nudge.recipient_team.as_str(), TEST_TEAM);
    assert_eq!(nudge.description, "hello graft");
    receiver_thread.join().expect("join fake graft receiver");
}
