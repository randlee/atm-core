use crate::graft_rpc::{
    AdvisoryBatchLimit, AdvisoryDrainRequest, AdvisoryFetchRequest, AdvisorySessionId,
    AdvisorySessionRegistrationRequest, AdvisorySessionUnregistrationRequest,
    RequestEnvelope as GraftRequestEnvelope, ResponseEnvelope as GraftResponseEnvelope,
};
use atm_core::ack::AckRequest;
use atm_core::boundary::{ReplaySource, RequestDispatcher, RosterHarness};
use atm_core::protocol::{
    RequestEnvelope, ResponseEnvelope, SendRequestEnvelope, SendResponseEnvelope,
};
use atm_core::schema::{AgentMember, TeamConfig};
use atm_core::send::{SendMessageSource, SendRequest};
use atm_core::test_support::ROLE_TEAM_LEAD;
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use atm_runtime_test_support::open_sqlite_boundary;
use tempfile::TempDir;

use crate::GraftRequestDispatcher;
use crate::runtime_health::{DaemonRequestDispatcher, RuntimeStatusCache};

const TEST_TEAM: &str = "test-team";

fn replay_source_static(label: &'static str) -> ReplaySource {
    ReplaySource::new(label).unwrap_or_else(|_| unreachable!("static replay source must validate"))
}

fn install_test_roster(db_path: &std::path::Path, members: &[&str]) {
    let assembly = open_sqlite_boundary(db_path).expect("assemble boundary");
    let roster_store = assembly.roster_store_arc();
    let team = TEST_TEAM
        .parse::<atm_core::types::TeamName>()
        .expect("team");
    let members = members
        .iter()
        .map(|name| {
            atm_core::boundary::roster_member_record_from_claude_code_member(
                team.clone(),
                AgentMember::with_name((*name).parse().expect("member")),
            )
        })
        .collect::<Vec<_>>();
    roster_store
        .replace_roster(
            &team,
            &members,
            Some(&replay_source_static("daemon-graft-test")),
        )
        .expect("replace roster");
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
            Some(&replay_source_static("daemon-graft-test")),
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

fn advisory_registration_request(session_id: &str) -> AdvisorySessionRegistrationRequest {
    advisory_registration_request_for(ROLE_TEAM_LEAD, session_id)
}

fn advisory_registration_request_for(
    agent: &str,
    session_id: &str,
) -> AdvisorySessionRegistrationRequest {
    AdvisorySessionRegistrationRequest {
        team: TEST_TEAM.parse().expect("team"),
        agent: agent.parse().expect("agent"),
        session_id: AdvisorySessionId::new(session_id).expect("session id"),
        pid: std::process::id(),
        started_at: IsoTimestamp::now(),
    }
}

fn advisory_test_dispatcher() -> (TempDir, DaemonRequestDispatcher) {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");
    install_test_roster(&db_path, &[ROLE_TEAM_LEAD]);
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD]);
    // TempDir keeps filesystem state process-local across platforms, but this
    // helper still exercises the same POSIX-style local IPC/runtime globals as
    // the rest of the advisory dispatcher tests, so the helper remains
    // #[serial] until the test harness stops sharing that process-wide state.
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home, RuntimeStatusCache::new(), db_path);
    (tempdir, dispatcher)
}

fn write_workspace_config(workspace_dir: &std::path::Path) {
    std::fs::create_dir_all(workspace_dir).expect("workspace dir");
    std::fs::write(workspace_dir.join(".atm.toml"), "[atm]\n").expect("workspace config");
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_routes_advisory_register_requests() {
    let (_tempdir, dispatcher) = advisory_test_dispatcher();
    let request = advisory_registration_request("session-register");

    let response = dispatcher
        .dispatch_graft(GraftRequestEnvelope::AdvisoryRegister(request.clone()))
        .expect("register response");

    match response {
        GraftResponseEnvelope::AdvisoryRegister(registered) => {
            assert_eq!(registered.team, request.team);
            assert_eq!(registered.agent, request.agent);
            assert_eq!(registered.session_id, request.session_id);
            assert_eq!(registered.queue_capacity, 256);
        }
        other => panic!("expected advisory register response, got {other:?}"),
    }
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_routes_advisory_unregister_requests() {
    let (_tempdir, dispatcher) = advisory_test_dispatcher();
    let request = advisory_registration_request("session-unregister");
    dispatcher
        .dispatch_graft(GraftRequestEnvelope::AdvisoryRegister(request.clone()))
        .expect("register response");

    let response = dispatcher
        .dispatch_graft(GraftRequestEnvelope::AdvisoryUnregister(
            AdvisorySessionUnregistrationRequest {
                session_id: request.session_id.clone(),
            },
        ))
        .expect("unregister response");

    match response {
        GraftResponseEnvelope::AdvisoryUnregister(unregistered) => {
            assert_eq!(unregistered.session_id, request.session_id);
            assert!(unregistered.closed);
        }
        other => panic!("expected advisory unregister response, got {other:?}"),
    }
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_routes_advisory_fetch_requests() {
    let (_tempdir, dispatcher) = advisory_test_dispatcher();
    let request = advisory_registration_request("session-fetch");
    dispatcher
        .dispatch_graft(GraftRequestEnvelope::AdvisoryRegister(request.clone()))
        .expect("register response");

    let response = dispatcher
        .dispatch_graft(GraftRequestEnvelope::AdvisoryFetch(AdvisoryFetchRequest {
            session_id: request.session_id.clone(),
            limit: AdvisoryBatchLimit::new(8).expect("limit"),
        }))
        .expect("fetch response");

    match response {
        GraftResponseEnvelope::AdvisoryFetch(fetch) => {
            assert_eq!(fetch.session_id, request.session_id);
            assert!(fetch.nudges.is_empty());
            assert_eq!(fetch.remaining, 0);
            assert_eq!(fetch.dropped_count, 0);
        }
        other => panic!("expected advisory fetch response, got {other:?}"),
    }
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_routes_advisory_drain_requests() {
    let (_tempdir, dispatcher) = advisory_test_dispatcher();
    let request = advisory_registration_request("session-drain");
    dispatcher
        .dispatch_graft(GraftRequestEnvelope::AdvisoryRegister(request.clone()))
        .expect("register response");

    let response = dispatcher
        .dispatch_graft(GraftRequestEnvelope::AdvisoryDrain(AdvisoryDrainRequest {
            session_id: request.session_id.clone(),
            limit: AdvisoryBatchLimit::new(8).expect("limit"),
        }))
        .expect("drain response");

    match response {
        GraftResponseEnvelope::AdvisoryDrain(drain) => {
            assert_eq!(drain.session_id, request.session_id);
            assert!(drain.nudges.is_empty());
            assert_eq!(drain.remaining, 0);
            assert_eq!(drain.dropped_count, 0);
        }
        other => panic!("expected advisory drain response, got {other:?}"),
    }
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_send_queues_graft_post_send_event_for_registered_non_claude_recipient() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    let workspace_dir = tempdir.path().join("workspace");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    write_workspace_config(&workspace_dir);
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
    let registration = advisory_registration_request_for("qa-a", "session-send");
    dispatcher
        .dispatch_graft(GraftRequestEnvelope::AdvisoryRegister(registration.clone()))
        .expect("register response");

    let response = dispatcher
        .dispatch(RequestEnvelope::Send(SendRequestEnvelope::Compose(
            SendRequest::new(
                atm_home.clone(),
                workspace_dir.clone(),
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
    assert!(outcome.warnings.is_empty());

    let drain = dispatcher
        .dispatch_graft(GraftRequestEnvelope::AdvisoryDrain(AdvisoryDrainRequest {
            session_id: registration.session_id.clone(),
            limit: AdvisoryBatchLimit::new(8).expect("limit"),
        }))
        .expect("drain response");
    match drain {
        GraftResponseEnvelope::AdvisoryDrain(drain) => {
            assert_eq!(drain.nudges.len(), 1);
            assert_eq!(drain.nudges[0].message_id, outcome.message_id);
            assert_eq!(drain.nudges[0].message, "hello graft");
        }
        other => panic!("expected advisory drain response, got {other:?}"),
    }
}

#[test]
#[serial_test::serial(env)]
fn dispatcher_ack_queues_graft_post_send_event_for_registered_reply_target() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    let workspace_dir = tempdir.path().join("workspace");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    write_workspace_config(&workspace_dir);
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
    let registration = advisory_registration_request_for("qa-a", "session-ack");
    dispatcher
        .dispatch_graft(GraftRequestEnvelope::AdvisoryRegister(registration.clone()))
        .expect("register response");

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
                home_dir: atm_home.clone(),
                current_dir: workspace_dir.clone(),
                caller_identity: ROLE_TEAM_LEAD.parse::<AgentName>().expect("caller"),
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
    assert!(ack_outcome.warnings.is_empty());

    let drain = dispatcher
        .dispatch_graft(GraftRequestEnvelope::AdvisoryDrain(AdvisoryDrainRequest {
            session_id: registration.session_id.clone(),
            limit: AdvisoryBatchLimit::new(8).expect("limit"),
        }))
        .expect("drain response");
    match drain {
        GraftResponseEnvelope::AdvisoryDrain(drain) => {
            assert_eq!(drain.nudges.len(), 1);
            assert_eq!(drain.nudges[0].message_id, ack_outcome.reply_message_id);
            assert_eq!(drain.nudges[0].message, "ack reply");
        }
        other => panic!("expected advisory drain response, got {other:?}"),
    }
}
