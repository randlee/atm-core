use atm_core::ack::AckRequest;
use atm_core::boundary::RosterHarness;
use atm_core::graft::{
    GraftPostSendResponse, GraftReceiverListener, graft_receiver_record_path_from_home,
};
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, SendResponseEnvelope};
use atm_core::schema::{AgentMember, TeamConfig};
use atm_core::send::{SendMessageSource, SendRequest};
use atm_core::test_support::{EnvGuard, ROLE_TEAM_LEAD};
use atm_core::types::{AgentName, TeamName};
use atm_runtime_test_support::open_sqlite_boundary;
use std::time::Duration;
use tempfile::TempDir;

use crate::runtime_health::{DaemonRequestDispatcher, RuntimeStatusCache};

const TEST_TEAM: &str = "test-team";

fn install_test_roster_with_harness(
    db_path: &std::path::Path,
    members: &[(
        &str,
        RosterHarness,
        Option<&std::path::Path>,
        Option<&std::path::Path>,
    )],
) {
    let assembly = open_sqlite_boundary(db_path).expect("assemble boundary");
    let roster_store = assembly.roster_store_arc();
    let team = TEST_TEAM.parse::<TeamName>().expect("team");
    let members = members
        .iter()
        .map(|(name, harness, home_dir, workspace_root)| {
            let mut member = AgentMember::with_name((*name).parse().expect("member"));
            if let Some(home_dir) = home_dir {
                member.home_dir = home_dir.to_path_buf().into();
            }
            if let Some(workspace_root) = workspace_root {
                member.extra.insert(
                    "workspace_root".to_string(),
                    serde_json::Value::String(workspace_root.display().to_string()),
                );
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
        .replace_roster(&team, &members)
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
    let profile_home = tempdir.path().join("recipient-profile");
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
                None,
            ),
            (
                "qa-a",
                RosterHarness::CodexCli,
                Some(profile_home.as_path()),
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
fn dispatcher_send_keeps_post_commit_graft_failure_out_of_admission_response() {
    let (_tempdir, atm_home, workspace_dir, dispatcher) = graft_warning_dispatcher();

    let response = dispatcher
        .dispatch(RequestEnvelope::Write(Box::new(
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

    let outcome = match response {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => outcome,
        other => panic!("expected send response, got {other:?}"),
    };
    assert!(
        outcome.warnings.is_empty(),
        "post-commit graft failure must not relabel a successful local admission"
    );
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_ack_keeps_post_commit_graft_failure_out_of_admission_response() {
    let (_tempdir, atm_home, workspace_dir, dispatcher) = graft_warning_dispatcher();

    let source_response = dispatcher
        .dispatch(RequestEnvelope::Write(Box::new(
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
    let source_message_id = match source_response {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => outcome.message_id,
        other => panic!("expected send response, got {other:?}"),
    };

    let ack_response = dispatcher
        .dispatch(RequestEnvelope::Write(Box::new(
            AckRequest {
                home_dir: atm_home,
                current_dir: workspace_dir,
                caller_identity: ROLE_TEAM_LEAD.parse().expect("caller"),
                caller_chat_id: None,
                caller_team: TEST_TEAM.parse().expect("team"),
                message_id: source_message_id,
                reply_body: "ack reply".to_string(),
            }
            .into_write_request(),
        )))
        .expect("ack response");

    let ack_outcome = match ack_response {
        ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome)) => outcome,
        other => panic!("expected ack response, got {other:?}"),
    };
    assert!(
        ack_outcome.warnings.is_empty(),
        "post-commit graft failure must not relabel a successful ACK admission"
    );
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_send_delivers_direct_graft_nudge_without_warning() {
    let (_tempdir, atm_home, workspace_dir, dispatcher) = graft_warning_dispatcher();
    write_graft_enabled_config(&workspace_dir);
    let recipient_team = TEST_TEAM.parse::<TeamName>().expect("team");
    let recipient_agent = "qa-a".parse::<AgentName>().expect("agent");
    let receiver_path =
        graft_receiver_record_path_from_home(&workspace_dir, &recipient_team, &recipient_agent);
    let listener = GraftReceiverListener::bind(&receiver_path).expect("bind fake graft receiver");
    let (event_tx, event_rx) = std::sync::mpsc::sync_channel(1);
    let receiver_thread = std::thread::spawn(move || {
        let mut stream = loop {
            if let Some(stream) = listener.poll_accept().expect("poll graft receiver") {
                break stream;
            }
            std::thread::yield_now();
        };
        let request = listener
            .read_request(&mut stream, Duration::from_secs(5))
            .expect("read graft request");
        event_tx
            .send(request.event.clone())
            .expect("send captured event");
        listener
            .write_response(&mut stream, &GraftPostSendResponse::Delivered)
            .expect("write graft response");
    });
    let _env = EnvGuard::set_many([
        ("ATM_HOME", Some(atm_home.to_str().expect("utf8 atm home"))),
        ("HOME", Some(atm_home.to_str().expect("utf8 home"))),
        ("USERPROFILE", None),
    ]);
    let response = dispatcher
        .dispatch(RequestEnvelope::Write(Box::new(
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
    let response = match response {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => outcome,
        other => panic!("expected send response, got {other:?}"),
    };

    assert!(response.warnings.is_empty());
    let nudge = event_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("receive graft nudge");
    assert_eq!(nudge.recipient.as_str(), "qa-a");
    assert_eq!(nudge.recipient_team.as_str(), TEST_TEAM);
    assert_eq!(nudge.description, "hello graft");
    receiver_thread.join().expect("join fake graft receiver");
}
