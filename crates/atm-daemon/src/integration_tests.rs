//! Cross-transport runtime-observation integration regressions.

use crate::runtime_health::dispatch::TrustedActivityObservation;
use crate::runtime_health::{DaemonRequestDispatcher, RuntimeStatusCache};
use atm_core::ack::AckRequest;
use atm_core::api::UntrustedSmokeProvenance;
use atm_core::protocol::{
    HeartbeatActivity, RequestEnvelope, ResponseEnvelope, SendResponseEnvelope,
    TeamMemberHeartbeatRequest,
};
use atm_core::read::ReadQuery;
use atm_core::send::{SendMessageSource, SendRequest};
use atm_core::test_support::{ROLE_TEAM_LEAD, TEST_QA};
use atm_core::types::{AgentName, IsoTimestamp, ReadSelection, SessionId, TeamName};
use atm_core::{ApiRequest, ApiRouter, AuthenticatedIngress, RequestDeadline};

// The constructor for this capability is private to the authenticated router;
// retaining the type requirement here makes the boundary compile-time checked.
fn requires_trusted_observation(_: TrustedActivityObservation) {}

#[test]
fn trusted_activity_observation_capability_gate_is_compile_time_enforced() {
    let _ = requires_trusted_observation;
}

#[test]
fn heartbeat_session_id_round_trip() {
    let tempdir = tempfile::TempDir::new().expect("tempdir");
    let home = tempdir.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let db_path = tempdir.path().join("runtime.db");
    crate::tests::install_test_roster(&db_path, &[ROLE_TEAM_LEAD]);
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(home, RuntimeStatusCache::new(), db_path);
    let team: TeamName = crate::tests::TEST_TEAM.parse().expect("team");
    let member: AgentName = ROLE_TEAM_LEAD.parse().expect("member");
    let session = SessionId::new("three-step").expect("session");
    for incoming in [Some(session.clone()), None, Some(session.clone())] {
        let response = dispatcher
            .dispatch(RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
                team: team.clone(),
                member: member.clone(),
                pid: 1,
                observed_at: IsoTimestamp::now(),
                activity: HeartbeatActivity::ActiveToolUse,
                session_id: incoming,
            }))
            .expect("heartbeat");
        let ResponseEnvelope::Heartbeat(response) = response else {
            panic!("heartbeat response")
        };
        assert_eq!(response.session_id, Some(session.clone()));
    }
}

#[test]
fn send_read_ack_reflects_each_caller_session_in_runtime_cache() {
    let tempdir = tempfile::TempDir::new().expect("tempdir");
    let home = tempdir.path().join("home");
    let workspace = tempdir.path().join("workspace");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&workspace).expect("workspace");
    let db_path = tempdir.path().join("runtime.db");
    crate::tests::install_test_roster(&db_path, &[ROLE_TEAM_LEAD, TEST_QA]);
    let cache = RuntimeStatusCache::new();
    let dispatcher = DaemonRequestDispatcher::new_for_test(home.clone(), cache.clone(), db_path);
    let team: TeamName = crate::tests::TEST_TEAM.parse().expect("team");
    let lead: AgentName = ROLE_TEAM_LEAD.parse().expect("lead");
    let qa: AgentName = TEST_QA.parse().expect("qa");

    let send_session = SessionId::new("send-session").expect("session");
    let sent = dispatcher
        .dispatch(RequestEnvelope::Write(Box::new(
            SendRequest::new(
                home.clone(),
                workspace.clone(),
                lead.clone(),
                &format!("{TEST_QA}@{}", crate::tests::TEST_TEAM),
                team.clone(),
                SendMessageSource::Inline("please acknowledge".to_string()),
                None,
                true,
                None,
                false,
            )
            .expect("send request")
            .with_activity_observation(Some(
                atm_core::caller_context::ActivityObservation {
                    team: team.clone(),
                    member: lead.clone(),
                    session_id: Some(send_session.clone()),
                    pid: Some(1),
                },
            )),
        )))
        .expect("send response");
    let ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) = sent else {
        panic!("send response")
    };
    assert_eq!(cache.cached_session_id(&team, &lead), Some(send_session));

    let read_session = SessionId::new("read-session").expect("session");
    let read = ReadQuery::new(
        home.clone(),
        workspace.clone(),
        qa.clone(),
        None,
        team.clone(),
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
    .expect("read query")
    .with_activity_observation(Some(atm_core::caller_context::ActivityObservation {
        team: team.clone(),
        member: qa.clone(),
        session_id: Some(read_session.clone()),
        pid: Some(2),
    }));
    assert!(matches!(
        dispatcher
            .dispatch(RequestEnvelope::Receive(read))
            .expect("read response"),
        ResponseEnvelope::Receive(_)
    ));
    assert_eq!(cache.cached_session_id(&team, &qa), Some(read_session));

    let ack_session = SessionId::new("ack-session").expect("session");
    let mut ack = AckRequest {
        home_dir: home,
        current_dir: workspace,
        caller_identity: qa.clone(),
        caller_chat_id: None,
        caller_team: team.clone(),
        activity_observation: None,
        message_id: outcome.message_id,
        reply_body: "acknowledged".to_string(),
    };
    ack.activity_observation = Some(atm_core::caller_context::ActivityObservation {
        team: team.clone(),
        member: qa.clone(),
        session_id: Some(ack_session.clone()),
        pid: Some(2),
    });
    assert!(matches!(
        dispatcher
            .dispatch(RequestEnvelope::Write(Box::new(ack.into_write_request())))
            .expect("ack response"),
        ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(_))
    ));
    assert_eq!(cache.cached_session_id(&team, &qa), Some(ack_session));
}

#[test]
fn failed_dispatch_does_not_partially_mutate_runtime_cache() {
    let tempdir = tempfile::TempDir::new().expect("tempdir");
    let home = tempdir.path().join("home");
    let workspace = tempdir.path().join("workspace");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&workspace).expect("workspace");
    let db_path = tempdir.path().join("runtime.db");
    crate::tests::install_test_roster(&db_path, &[ROLE_TEAM_LEAD]);
    let cache = RuntimeStatusCache::new();
    let dispatcher = DaemonRequestDispatcher::new_for_test(home.clone(), cache.clone(), db_path);
    let team: TeamName = crate::tests::TEST_TEAM.parse().expect("team");
    let member: AgentName = ROLE_TEAM_LEAD.parse().expect("member");
    let request = SendRequest::new(
        home,
        workspace,
        member.clone(),
        &format!("missing@{}", crate::tests::TEST_TEAM),
        team.clone(),
        SendMessageSource::Inline("will fail".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("request")
    .with_activity_observation(Some(atm_core::caller_context::ActivityObservation {
        team: team.clone(),
        member: member.clone(),
        session_id: Some(SessionId::new("must-not-stick").expect("session")),
        pid: Some(1),
    }));
    assert!(
        dispatcher
            .dispatch(RequestEnvelope::Write(Box::new(request)))
            .is_err()
    );
    assert_eq!(cache.cached_session_id(&team, &member), None);
}

#[test]
fn forged_peer_ingress_does_not_mutate_runtime_cache() {
    let tempdir = tempfile::TempDir::new().expect("tempdir");
    let home = tempdir.path().join("home");
    let workspace = tempdir.path().join("workspace");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&workspace).expect("workspace");
    let db_path = tempdir.path().join("runtime.db");
    crate::tests::install_test_roster(&db_path, &[ROLE_TEAM_LEAD, TEST_QA]);
    let cache = RuntimeStatusCache::new();
    let dispatcher = DaemonRequestDispatcher::new_for_test(home.clone(), cache.clone(), db_path);
    let team: TeamName = crate::tests::TEST_TEAM.parse().expect("team");
    let member: AgentName = ROLE_TEAM_LEAD.parse().expect("member");
    let request = SendRequest::new(
        home,
        workspace,
        member.clone(),
        &format!("{TEST_QA}@{}", crate::tests::TEST_TEAM),
        team.clone(),
        SendMessageSource::Inline("peer".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("request")
    .with_activity_observation(Some(atm_core::caller_context::ActivityObservation {
        team: team.clone(),
        member: member.clone(),
        session_id: Some(SessionId::new("forged").expect("session")),
        pid: Some(1),
    }));
    assert!(
        dispatcher
            .route(
                ApiRequest::new(RequestEnvelope::Write(Box::new(request))),
                AuthenticatedIngress::Peer,
                RequestDeadline::after(std::time::Duration::from_secs(1)),
            )
            .is_err()
    );
    assert_eq!(cache.cached_session_id(&team, &member), None);
}

#[test]
fn deadline_before_dispatch_does_not_record_observation() {
    let tempdir = tempfile::TempDir::new().expect("tempdir");
    let home = tempdir.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let db_path = tempdir.path().join("runtime.db");
    crate::tests::install_test_roster(&db_path, &[ROLE_TEAM_LEAD]);
    let cache = RuntimeStatusCache::new();
    let dispatcher = DaemonRequestDispatcher::new_for_test(home, cache.clone(), db_path);
    let team: TeamName = crate::tests::TEST_TEAM.parse().expect("team");
    let member: AgentName = ROLE_TEAM_LEAD.parse().expect("member");
    let request = TeamMemberHeartbeatRequest {
        team: team.clone(),
        member: member.clone(),
        pid: 1,
        observed_at: IsoTimestamp::now(),
        activity: HeartbeatActivity::ActiveToolUse,
        session_id: Some(SessionId::new("late").expect("session")),
    };
    assert!(
        dispatcher
            .route(
                ApiRequest::new(RequestEnvelope::Heartbeat(request)),
                AuthenticatedIngress::Local,
                RequestDeadline::after(std::time::Duration::ZERO)
            )
            .is_err()
    );
    assert_eq!(cache.cached_session_id(&team, &member), None);
}

#[test]
fn non_local_heartbeat_ingress_is_rejected_without_cache_mutation() {
    let tempdir = tempfile::TempDir::new().expect("tempdir");
    let home = tempdir.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let db_path = tempdir.path().join("runtime.db");
    crate::tests::install_test_roster(&db_path, &[ROLE_TEAM_LEAD]);
    let cache = RuntimeStatusCache::new();
    let dispatcher = DaemonRequestDispatcher::new_for_test(home, cache.clone(), db_path);
    let team: TeamName = crate::tests::TEST_TEAM.parse().expect("team");
    let member: AgentName = ROLE_TEAM_LEAD.parse().expect("member");
    let session = SessionId::new("forged-heartbeat").expect("session");
    for ingress in [
        AuthenticatedIngress::Peer,
        AuthenticatedIngress::UntrustedSmoke(UntrustedSmokeProvenance::new(
            "smoke-peer.invalid".parse().expect("host"),
        )),
        AuthenticatedIngress::AnonymousSmoke,
    ] {
        let request = TeamMemberHeartbeatRequest {
            team: team.clone(),
            member: member.clone(),
            pid: 7,
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::ActiveToolUse,
            session_id: Some(session.clone()),
        };
        assert!(
            dispatcher
                .route(
                    ApiRequest::new(RequestEnvelope::Heartbeat(request)),
                    ingress,
                    RequestDeadline::after(std::time::Duration::from_secs(1)),
                )
                .is_err()
        );
    }
    assert_eq!(cache.cached_session_id(&team, &member), None);
}

#[test]
fn deadline_inside_dispatch_records_observation() {
    let tempdir = tempfile::TempDir::new().expect("tempdir");
    let home = tempdir.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let db_path = tempdir.path().join("runtime.db");
    crate::tests::install_test_roster(&db_path, &[ROLE_TEAM_LEAD]);
    let cache = RuntimeStatusCache::new();
    let dispatcher = DaemonRequestDispatcher::new_for_test(home, cache.clone(), db_path);
    let team: TeamName = crate::tests::TEST_TEAM.parse().expect("team");
    let member: AgentName = ROLE_TEAM_LEAD.parse().expect("member");
    let session = SessionId::new("on-time").expect("session");
    let request = TeamMemberHeartbeatRequest {
        team: team.clone(),
        member: member.clone(),
        pid: 1,
        observed_at: IsoTimestamp::now(),
        activity: HeartbeatActivity::ActiveToolUse,
        session_id: Some(session.clone()),
    };
    dispatcher
        .route(
            ApiRequest::new(RequestEnvelope::Heartbeat(request)),
            AuthenticatedIngress::Local,
            RequestDeadline::after(std::time::Duration::from_secs(1)),
        )
        .expect("on-time request");
    assert_eq!(cache.cached_session_id(&team, &member), Some(session));
}
