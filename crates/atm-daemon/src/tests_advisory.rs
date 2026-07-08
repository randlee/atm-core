use atm_core::ack::AckRequest;
use atm_core::boundary::{ReplaySource, RequestDispatcher, RosterHarness};
use atm_core::error_codes::AtmErrorCode;
use atm_core::protocol::{
    RequestEnvelope, ResponseEnvelope, SendRequestEnvelope, SendResponseEnvelope,
};
use atm_core::schema::{AgentMember, TeamConfig};
use atm_core::send::{SendMessageSource, SendRequest};
use atm_core::test_support::ROLE_TEAM_LEAD;
use atm_core::types::TeamName;
use atm_runtime_test_support::open_sqlite_boundary;
use tempfile::TempDir;

use crate::runtime_health::{DaemonRequestDispatcher, RuntimeStatusCache};

const TEST_TEAM: &str = "test-team";

fn replay_source_static(label: &'static str) -> ReplaySource {
    ReplaySource::new(label).unwrap_or_else(|_| unreachable!("static replay source must validate"))
}

fn install_test_roster_with_harness(db_path: &std::path::Path, members: &[(&str, RosterHarness)]) {
    let assembly = open_sqlite_boundary(db_path).expect("assemble boundary");
    let roster_store = assembly.roster_store_arc();
    let team = TEST_TEAM.parse::<TeamName>().expect("team");
    let members = members
        .iter()
        .map(|(name, harness)| {
            let mut record = atm_core::boundary::roster_member_record_from_claude_code_member(
                team.clone(),
                AgentMember::with_name((*name).parse().expect("member")),
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
            (ROLE_TEAM_LEAD, RosterHarness::ClaudeCode),
            ("qa-a", RosterHarness::CodexCli),
        ],
    );
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD, "qa-a"]);
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), RuntimeStatusCache::new(), db_path);
    (tempdir, atm_home, workspace_dir, dispatcher)
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_send_surfaces_typed_warning_when_graft_receiver_path_is_unavailable() {
    let (_tempdir, atm_home, workspace_dir, dispatcher) = graft_warning_dispatcher();

    let response = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(
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
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(
            SendRequest::new(
                atm_home.clone(),
                workspace_dir.clone(),
                "qa-a".parse().expect("caller"),
                "team-lead@test-team",
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
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Acknowledge(
            AckRequest {
                home_dir: atm_home,
                current_dir: workspace_dir,
                caller_identity: ROLE_TEAM_LEAD.parse().expect("caller"),
                caller_team: TEST_TEAM.parse().expect("team"),
                message_id: source_message_id,
                reply_body: "ack reply".to_string(),
            },
        )))
        .expect("ack response");

    let ack_outcome = match ack_response {
        ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome)) => outcome,
        other => panic!("expected ack response, got {other:?}"),
    };
    assert_eq!(ack_outcome.warnings.len(), 1);
    assert_eq!(
        ack_outcome.warnings[0].code,
        Some(AtmErrorCode::PostSendGraftUnavailable)
    );
    assert!(ack_outcome.warnings[0].recovery.is_some());
}
