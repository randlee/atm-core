use atm_core::boundary::RequestDispatcher;
use atm_core::graft::{
    GraftBatchLimit, GraftNudgeDrainRequest, GraftNudgeFetchRequest, GraftSessionId,
    GraftSessionRegistrationRequest, GraftSessionUnregistrationRequest,
};
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
use atm_core::schema::{AgentMember, TeamConfig};
use atm_core::test_support::ROLE_TEAM_LEAD;
use atm_core::types::IsoTimestamp;
use atm_rusqlite::assemble_boundary;
use serial_test::serial;
use tempfile::TempDir;

use crate::runtime_health::{DaemonRequestDispatcher, RuntimeStatusCache};

const TEST_TEAM: &str = "test-team";

fn install_test_roster(db_path: &std::path::Path, members: &[&str]) {
    let assembly = assemble_boundary(db_path).expect("assemble boundary");
    let roster_store = assembly.roster_store();
    roster_store
        .replace_roster(atm_core::boundary::RosterStoreReplaceRosterRequest {
            team: TEST_TEAM.parse().expect("team"),
            roster: TeamConfig {
                members: members
                    .iter()
                    .map(|name| AgentMember::with_name((*name).parse().expect("member")))
                    .collect(),
                ..Default::default()
            },
            source: Some("daemon-graft-test".to_string()),
        })
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

fn graft_registration_request(session_id: &str) -> GraftSessionRegistrationRequest {
    GraftSessionRegistrationRequest {
        team: TEST_TEAM.parse().expect("team"),
        agent: ROLE_TEAM_LEAD.parse().expect("agent"),
        session_id: GraftSessionId::new(session_id).expect("session id"),
        pid: std::process::id(),
        started_at: IsoTimestamp::now(),
    }
}

fn graft_test_dispatcher() -> (TempDir, DaemonRequestDispatcher) {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");
    install_test_roster(&db_path, &[ROLE_TEAM_LEAD]);
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD]);
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home, RuntimeStatusCache::new(), db_path);
    (tempdir, dispatcher)
}

#[test]
#[serial]
fn dispatcher_routes_graft_register_requests() {
    let (_tempdir, dispatcher) = graft_test_dispatcher();
    let request = graft_registration_request("session-register");

    let response = dispatcher
        .dispatch(RequestEnvelope::GraftRegister(request.clone()))
        .expect("register response");

    match response {
        ResponseEnvelope::GraftRegister(registered) => {
            assert_eq!(registered.team, request.team);
            assert_eq!(registered.agent, request.agent);
            assert_eq!(registered.session_id, request.session_id);
            assert_eq!(registered.queue_capacity, 256);
        }
        other => panic!("expected graft register response, got {other:?}"),
    }
}

#[test]
#[serial]
fn dispatcher_routes_graft_unregister_requests() {
    let (_tempdir, dispatcher) = graft_test_dispatcher();
    let request = graft_registration_request("session-unregister");
    dispatcher
        .dispatch(RequestEnvelope::GraftRegister(request.clone()))
        .expect("register response");

    let response = dispatcher
        .dispatch(RequestEnvelope::GraftUnregister(
            GraftSessionUnregistrationRequest {
                session_id: request.session_id.clone(),
            },
        ))
        .expect("unregister response");

    match response {
        ResponseEnvelope::GraftUnregister(unregistered) => {
            assert_eq!(unregistered.session_id, request.session_id);
            assert!(unregistered.closed);
        }
        other => panic!("expected graft unregister response, got {other:?}"),
    }
}

#[test]
#[serial]
fn dispatcher_routes_graft_fetch_requests() {
    let (_tempdir, dispatcher) = graft_test_dispatcher();
    let request = graft_registration_request("session-fetch");
    dispatcher
        .dispatch(RequestEnvelope::GraftRegister(request.clone()))
        .expect("register response");

    let response = dispatcher
        .dispatch(RequestEnvelope::GraftFetch(GraftNudgeFetchRequest {
            session_id: request.session_id.clone(),
            limit: GraftBatchLimit::new(8).expect("limit"),
        }))
        .expect("fetch response");

    match response {
        ResponseEnvelope::GraftFetch(fetch) => {
            assert_eq!(fetch.session_id, request.session_id);
            assert!(fetch.nudges.is_empty());
            assert_eq!(fetch.remaining, 0);
            assert_eq!(fetch.dropped_count, 0);
        }
        other => panic!("expected graft fetch response, got {other:?}"),
    }
}

#[test]
#[serial]
fn dispatcher_routes_graft_drain_requests() {
    let (_tempdir, dispatcher) = graft_test_dispatcher();
    let request = graft_registration_request("session-drain");
    dispatcher
        .dispatch(RequestEnvelope::GraftRegister(request.clone()))
        .expect("register response");

    let response = dispatcher
        .dispatch(RequestEnvelope::GraftDrain(GraftNudgeDrainRequest {
            session_id: request.session_id.clone(),
            limit: GraftBatchLimit::new(8).expect("limit"),
        }))
        .expect("drain response");

    match response {
        ResponseEnvelope::GraftDrain(drain) => {
            assert_eq!(drain.session_id, request.session_id);
            assert!(drain.nudges.is_empty());
            assert_eq!(drain.remaining, 0);
            assert_eq!(drain.dropped_count, 0);
        }
        other => panic!("expected graft drain response, got {other:?}"),
    }
}
