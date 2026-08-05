#[cfg(not(windows))]
use super::local_ipc_transport::{PeerResendServeHooks, RuntimeServeHooks};
#[cfg(windows)]
use super::local_tcp_transport::{PeerResendServeHooks, RuntimeServeHooks};
use super::runtime_health::{
    DaemonRequestDispatcher, MAX_STATUS_CACHE_ENTRIES, RuntimeStatusCache,
};
use super::{
    LocalIpcServerTransportAdapter,
    composition::build_production_runtime,
    lifecycle_control::LifecycleControlSourceAdapter,
    non_claude_outbound_runtime::DaemonNonClaudeOutbound,
    test_support::{DoctorOnlyDispatcher, LifecycleFlagResetGuard},
};
use crate::test_support::{
    configure_test_local_ipc_timeouts, connect_daemon_local_ipc_until_ready,
    write_test_local_ipc_request,
};
use atm_core::ApiRouter;
use atm_core::doctor::DoctorQuery;
use atm_core::doctor::DoctorStatus;
use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::observability::AtmObservabilityHealthState;
use atm_core::protocol::{
    HeartbeatActivity, RequestEnvelope, ResponseEnvelope, RuntimeLivenessState, RuntimeMemberState,
    RuntimeReadinessState, TeamMemberHeartbeatRequest,
};
use atm_core::schema::{AgentMember, TeamConfig};
use atm_core::send::{SendMessageSource, SendRequest, send_mail_with_runtime};
use atm_core::test_support::EnvGuard;
use atm_core::test_support::ROLE_TEAM_LEAD;
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use atm_runtime_test_support::{
    SQLITE_RUNTIME_PATH_ENV, install_sqlite_retained_runtime_factory, open_sqlite_boundary,
};
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::mpsc;
use std::time::Duration;
use tempfile::TempDir;

pub(crate) const TEST_TEAM: &str = "test-team";
fn test_team() -> &'static TeamName {
    static TEST_TEAM_NAME: OnceLock<TeamName> = OnceLock::new();
    TEST_TEAM_NAME.get_or_init(|| TEST_TEAM.parse().expect("team"))
}

pub(crate) fn install_retained_runtime_factory() {
    install_sqlite_retained_runtime_factory();
}

struct ShutdownFinalizerDrainGuard;

impl Drop for ShutdownFinalizerDrainGuard {
    fn drop(&mut self) {
        DaemonRequestDispatcher::drain_shutdown_finalizer_threads_for_test();
    }
}

#[cfg(not(windows))]
mod local_ipc_depth;
mod peer_message_array;
mod resend_cache;
mod runtime_root;

#[test]
#[serial_test::serial(env)]
fn local_ipc_runtime_round_trips_doctor_requests_on_shared_transport() {
    install_retained_runtime_factory();
    // TempDir uniqueness is process-local; #[serial] keeps this same-host transport smoke test
    // from racing other lifecycle-control and singleton-sensitive daemon tests.
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");
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
    let dispatcher: Arc<dyn ApiRouter + Send + Sync> = Arc::new(DoctorOnlyDispatcher);
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
                            "doctor round-trip test failed to observe the daemon ready signal",
                        )
                    })
                },
                peer_resends: PeerResendServeHooks::disabled(),
            },
        );
        serve_result_tx.send(result).expect("send serve result");
    });

    let mut stream = connect_daemon_local_ipc_until_ready(&socket_path, ready_rx);
    configure_test_local_ipc_timeouts(&stream);
    let request = RequestEnvelope::Doctor(DoctorQuery {
        home_dir: tempdir.path().join("home"),
        current_dir: tempdir.path().join("cwd"),
        team_override: None,
        ..DoctorQuery::default()
    });
    write_test_local_ipc_request(&mut stream, &request).expect("write doctor request");
    let response =
        atm_core::api::read_http_response(&mut stream, &request).expect("read doctor response");
    match response {
        ResponseEnvelope::Doctor(report) => {
            assert_eq!(report.summary.status, DoctorStatus::Healthy);
        }
        other => panic!("unexpected response: {other:?}"),
    }

    // The loopback TCP record is published by whichever transport bound the
    // runtime. On Windows the local_tcp_transport ignores the caller's socket
    // path and writes the record under the host runtime root, so we resolve it
    // through the same production lookup a client would use. On Unix the
    // local_ipc_transport writes the record next to the endpoint socket
    // (`endpoint_path.parent()`), so we read it from `socket_path`'s parent.
    // Resolving the record from its real write location keeps this full
    // round-trip assertion running on every platform.
    #[cfg(windows)]
    let record_path = atm_daemon_client::resolve_daemon_local_ipc_endpoint()
        .expect("resolve local loopback TCP endpoint record")
        .as_ref()
        .to_path_buf();
    #[cfg(not(windows))]
    let record_path = socket_path
        .parent()
        .expect("socket path has a runtime directory")
        .join(atm_core::local_http::LOCAL_HTTP_RECORD_FILENAME);

    let record: atm_core::local_http::LocalHttpEndpointRecord = serde_json::from_slice(
        &std::fs::read(&record_path).expect("read local loopback TCP endpoint record"),
    )
    .expect("parse local loopback TCP endpoint record");
    let capability = record.capability().expect("active local capability");
    let capability_header = capability.to_base64url();
    let endpoint = record.ipv4_loopback.expect("IPv4 loopback endpoint");
    let mut tcp_stream = std::net::TcpStream::connect(endpoint).expect("connect loopback TCP");
    atm_core::api::write_http_request_with_headers(
        &mut tcp_stream,
        &request,
        &[(
            atm_core::local_http::LOCAL_CAPABILITY_HEADER,
            capability_header.as_str(),
        )],
    )
    .expect("write loopback TCP doctor request");
    let tcp_response = atm_core::api::read_http_response(&mut tcp_stream, &request)
        .expect("read loopback TCP doctor response");
    assert!(matches!(tcp_response, ResponseEnvelope::Doctor(_)));

    lifecycle.set_terminate_for_test(true);
    serve_result_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("recv serve result")
        .expect("serve runtime result");
    join.join().expect("join serve thread");
}

#[test]
#[serial_test::serial(env)]
fn runtime_composition_start_writes_retained_log_and_reports_healthy_observability() {
    install_retained_runtime_factory();
    let _drain_guard = ShutdownFinalizerDrainGuard;
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");
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
        ("ATM_LOG_DIR", None),
        ("ATM_DAEMON_SOCKET", None),
        ("HOME", Some(tempdir.path().to_str().expect("utf8 home"))),
        ("USERPROFILE", None),
    ]);
    let lifecycle = LifecycleControlSourceAdapter::install().expect("install lifecycle");
    let _reset = LifecycleFlagResetGuard::install(lifecycle.clone());
    let observability = std::sync::Arc::new(
        crate::test_observability::TestDaemonObservability::new(
            atm_core::home::host_log_dir_from_home(&atm_home),
        )
        .expect("test observability"),
    );
    let socket_path = tempdir.path().join("daemon.sock");
    let runtime = crate::composition::RuntimeComposition::new_with_runtime_db_path(
        crate::AtmHomeDir::from_path_for_test(atm_home.clone()),
        db_path.clone(),
        observability.clone(),
    )
    .expect("compose test runtime");
    let (result_tx, result_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let runtime_socket_path = socket_path.clone();

    let join = std::thread::spawn(move || {
        let result = runtime.start_with_socket_path_for_test(runtime_socket_path, Some(ready_tx));
        result_tx.send(result).expect("send runtime result");
    });

    let mut stream = connect_daemon_local_ipc_until_ready(&socket_path, ready_rx);
    configure_test_local_ipc_timeouts(&stream);
    let request = RequestEnvelope::Doctor(DoctorQuery {
        home_dir: atm_home.clone(),
        current_dir: atm_home.clone(),
        team_override: None,
        ..DoctorQuery::default()
    });
    write_test_local_ipc_request(&mut stream, &request).expect("write doctor request");
    let response =
        atm_core::api::read_http_response(&mut stream, &request).expect("read doctor response");
    match response {
        ResponseEnvelope::Doctor(report) => {
            assert_eq!(
                report.observability.logging_state,
                AtmObservabilityHealthState::Healthy
            );
        }
        other => panic!("expected doctor response, got {other:?}"),
    }

    observability
        .wait_for_message_contains("daemon start requested", Duration::from_secs(10))
        .expect("startup event should be recorded without busy-spin polling");

    lifecycle.set_terminate_for_test(true);
    result_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("recv runtime result")
        .expect("runtime result");
    join.join().expect("join runtime thread");
    DaemonRequestDispatcher::drain_shutdown_finalizer_threads_for_test();
}

#[allow(
    deprecated,
    reason = "legacy roster fixture remains outside AK.2 scope"
)]
fn install_test_roster(db_path: &std::path::Path, members: &[&str]) {
    let assembly = open_sqlite_boundary(db_path).expect("sqlite boundary");
    let roster = members
        .iter()
        .map(|name| {
            atm_core::boundary::roster_member_record_from_claude_code_member(
                test_team().clone(),
                AgentMember::with_name((*name).parse().expect("member")),
            )
        })
        .collect::<Vec<_>>();
    assembly
        .roster_store_arc()
        .replace_roster(test_team(), &roster)
        .expect("replace roster");
}

pub(crate) fn write_team_config(home_dir: &std::path::Path, members: &[&str]) {
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

fn write_workspace_config(workspace_dir: &std::path::Path) {
    std::fs::write(workspace_dir.join(".atm.toml"), "[atm]\n").expect("workspace config");
}

#[test]
#[serial_test::serial(env)]
fn production_runtime_only_logs_notifications_after_successful_post_send_emission() {
    let tempdir = TempDir::new().expect("tempdir");
    let workspace_dir = tempdir.path().join("workspace");
    let atm_home = tempdir.path().join("atm-home");
    let db_path = tempdir.path().join("mail.db");
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    write_workspace_config(&workspace_dir);
    install_test_roster(&db_path, &[ROLE_TEAM_LEAD, "qa-a"]);
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD, "qa-a"]);

    let _env = EnvGuard::set_many([
        ("ATM_HOME", Some(atm_home.to_str().expect("utf8 atm home"))),
        (
            "ATM_CONFIG_HOME",
            Some(tempdir.path().to_str().expect("utf8 config home")),
        ),
        ("HOME", Some(tempdir.path().to_str().expect("utf8 home"))),
        ("USERPROFILE", None),
    ]);
    let notification_path = atm_core::home::host_runtime_dir()
        .expect("host runtime dir")
        .join("notifications.jsonl");
    let notifications_before = std::fs::read(&notification_path).unwrap_or_default();
    let assembly = open_sqlite_boundary(&db_path).expect("sqlite boundary");
    let runtime = build_production_runtime(&assembly, Arc::new(DaemonNonClaudeOutbound::new()));

    let request = SendRequest::new(
        atm_home.clone(),
        workspace_dir.clone(),
        ROLE_TEAM_LEAD.parse().expect("caller"),
        "qa-a@test-team",
        TEST_TEAM.parse().expect("team"),
        SendMessageSource::Inline("boundary install proof".to_string()),
        None,
        false,
        None,
        false,
    )
    .expect("send request");
    let observability = atm_core::observability::NullObservability;

    send_mail_with_runtime(request, &observability, &runtime).expect("send mail");

    assert_eq!(
        std::fs::read(&notification_path).unwrap_or_default(),
        notifications_before,
        "notification log should only be appended after a successful post-send emission"
    );
}

#[test]
#[serial_test::serial(env)]
fn heartbeat_updates_status_cache_and_doctor_projection() {
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");
    let _env = EnvGuard::set_many([(
        SQLITE_RUNTIME_PATH_ENV,
        Some(db_path.to_str().expect("utf8 sqlite db path")),
    )]);

    install_test_roster(&db_path, &[ROLE_TEAM_LEAD, "qa-a"]);
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD, "qa-a"]);

    let status_cache = RuntimeStatusCache::new();
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), status_cache.clone(), db_path);
    let team = test_team().clone();
    let member: AgentName = ROLE_TEAM_LEAD.parse().expect("member");

    let response = dispatcher
        .dispatch(RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: team.clone(),
            member: member.clone(),
            pid: std::process::id(),
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::ActiveToolUse,
        }))
        .expect("heartbeat response");
    match response {
        ResponseEnvelope::Heartbeat(response) => {
            assert_eq!(response.team, team);
            assert_eq!(response.member, member);
            assert_eq!(
                response.state,
                atm_core::protocol::RuntimeMemberState::Active
            );
        }
        other => panic!("expected heartbeat response, got {other:?}"),
    }

    let snapshot = status_cache.snapshot();
    assert_eq!(snapshot.liveness, RuntimeLivenessState::Running);
    assert_eq!(snapshot.readiness, RuntimeReadinessState::Ready);
    assert_eq!(snapshot.member_counts.active_members, 1);

    let doctor = dispatcher
        .dispatch(RequestEnvelope::Doctor(atm_core::doctor::DoctorQuery {
            home_dir: atm_home.clone(),
            current_dir: atm_home.clone(),
            team_override: Some(team.clone()),
            ..atm_core::doctor::DoctorQuery::default()
        }))
        .expect("doctor response");
    match doctor {
        ResponseEnvelope::Doctor(report) => {
            let runtime_status = report.runtime_status.expect("runtime status");
            assert_eq!(runtime_status.member_counts.active_members, 1);
            assert_eq!(runtime_status.member_counts.unknown_members, 1);
        }
        other => panic!("expected doctor response, got {other:?}"),
    }
}

#[test]
fn dispatcher_hydrates_unknown_members_from_team_roster_on_startup() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");

    install_test_roster(&db_path, &[ROLE_TEAM_LEAD, "qa-a"]);
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD, "qa-a"]);

    let status_cache = RuntimeStatusCache::new();
    let _dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home, status_cache.clone(), db_path);

    let snapshot = status_cache.snapshot();
    assert_eq!(snapshot.readiness, RuntimeReadinessState::Ready);
    assert_eq!(snapshot.member_counts.active_members, 0);
    assert_eq!(snapshot.member_counts.idle_members, 0);
    assert_eq!(snapshot.member_counts.offline_members, 0);
    assert_eq!(snapshot.member_counts.unknown_members, 2);
}

#[test]
fn reload_runtime_view_applies_updated_team_config_and_preserves_live_state() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");

    install_test_roster(&db_path, &[ROLE_TEAM_LEAD]);
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD]);

    let status_cache = RuntimeStatusCache::new();
    let dispatcher = DaemonRequestDispatcher::new_for_test(
        atm_home.clone(),
        status_cache.clone(),
        db_path.clone(),
    );
    let team = test_team().clone();
    let leader: AgentName = ROLE_TEAM_LEAD.parse().expect("member");
    let qa: AgentName = "qa-a".parse().expect("member");

    dispatcher
        .dispatch(RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: team.clone(),
            member: leader.clone(),
            pid: std::process::id(),
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::ActiveToolUse,
        }))
        .expect("initial heartbeat");

    install_test_roster(&db_path, &[ROLE_TEAM_LEAD, "qa-a"]);
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD, "qa-a"]);

    dispatcher
        .reload_runtime_view()
        .expect("runtime view reload should succeed");

    assert_eq!(
        status_cache.member_state_for_test(&team, &leader),
        Some(RuntimeMemberState::Active)
    );
    assert_eq!(
        status_cache.member_state_for_test(&team, &qa),
        Some(RuntimeMemberState::Unknown)
    );
}

#[test]
fn reload_runtime_view_ignores_invalid_config_and_preserves_last_known_good_state() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");

    install_test_roster(&db_path, &[ROLE_TEAM_LEAD]);
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD]);

    let status_cache = RuntimeStatusCache::new();
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), status_cache.clone(), db_path);
    let team = test_team().clone();
    let leader: AgentName = ROLE_TEAM_LEAD.parse().expect("member");

    dispatcher
        .dispatch(RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: team.clone(),
            member: leader.clone(),
            pid: std::process::id(),
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::ActiveToolUse,
        }))
        .expect("initial heartbeat");

    let config_path = atm_home
        .join(".claude")
        .join("teams")
        .join(TEST_TEAM)
        .join("config.json");
    std::fs::write(&config_path, br#"{"members":["some-member",}"#).expect("invalid config");

    dispatcher
        .reload_runtime_view()
        .expect("invalid config should be ignored once roster truth is in sqlite");
    assert_eq!(
        status_cache.member_state_for_test(&team, &leader),
        Some(RuntimeMemberState::Active)
    );
}

#[test]
fn heartbeat_rejects_live_pid_conflict() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");

    install_test_roster(&db_path, &[ROLE_TEAM_LEAD]);

    let status_cache = RuntimeStatusCache::new();
    let dispatcher = DaemonRequestDispatcher::new_for_test(atm_home, status_cache.clone(), db_path);
    let team = test_team().clone();
    let member: AgentName = ROLE_TEAM_LEAD.parse().expect("member");

    dispatcher
        .dispatch(RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: team.clone(),
            member: member.clone(),
            pid: std::process::id(),
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::ActiveToolUse,
        }))
        .expect("initial heartbeat");

    let error = dispatcher
        .dispatch(RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: team.clone(),
            member: member.clone(),
            pid: std::process::id().saturating_add(1),
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::Idle,
        }))
        .expect_err("live pid conflict");

    assert_eq!(error.code(), AtmErrorCode::IdentityConflict);
    assert!(
        error
            .message()
            .starts_with("ATM_IDENTITY_CONFLICT: stop and report to user immediately")
    );
    assert_eq!(
        status_cache.member_state_for_test(&team, &member),
        Some(RuntimeMemberState::IdentityConflict)
    );
    let snapshot = status_cache.snapshot();
    assert_eq!(snapshot.readiness, RuntimeReadinessState::Degraded);
    assert!(
        snapshot
            .detail
            .as_ref()
            .is_some_and(|detail: &String| detail.contains("identity conflict"))
    );
}

#[test]
fn heartbeat_accepts_pid_takeover_when_previous_pid_is_dead() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");

    install_test_roster(&db_path, &[ROLE_TEAM_LEAD]);

    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home, RuntimeStatusCache::new(), db_path);
    let team = test_team().clone();
    let member: AgentName = ROLE_TEAM_LEAD.parse().expect("member");

    dispatcher
        .dispatch(RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: team.clone(),
            member: member.clone(),
            pid: u32::MAX,
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::Idle,
        }))
        .expect("initial dead-pid heartbeat");

    let response = dispatcher
        .dispatch(RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team,
            member,
            pid: std::process::id(),
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::ActiveToolUse,
        }))
        .expect("takeover heartbeat");

    match response {
        ResponseEnvelope::Heartbeat(response) => {
            assert!(response.pid_changed);
            assert_eq!(response.pid, std::process::id());
            assert_eq!(
                response.state,
                atm_core::protocol::RuntimeMemberState::Active
            );
        }
        other => panic!("expected heartbeat response, got {other:?}"),
    }
}

#[test]
fn heartbeat_evicts_oldest_member_and_projects_missing_roster_entries_as_unknown() {
    use chrono::{Duration as ChronoDuration, Utc};

    let status_cache = RuntimeStatusCache::new();
    let team = test_team().clone();
    let member: AgentName = "qa-a".parse().expect("member");
    let base = Utc::now();
    status_cache.insert_member_for_test(
        team.clone(),
        member.clone(),
        Some(u32::MAX),
        RuntimeMemberState::Idle,
        Some(IsoTimestamp::from_datetime(base)),
    );

    for index in 0..4095 {
        let member_name: AgentName = format!("member-{index}").parse().expect("member");
        status_cache.insert_member_for_test(
            team.clone(),
            member_name,
            Some(index as u32 + 2),
            RuntimeMemberState::Idle,
            Some(IsoTimestamp::from_datetime(
                base + ChronoDuration::seconds(index as i64 + 1),
            )),
        );
    }

    let response = status_cache.record_heartbeat_for_test(
        &TeamMemberHeartbeatRequest {
            team: team.clone(),
            member: "trigger-member".parse().expect("member"),
            pid: std::process::id(),
            observed_at: IsoTimestamp::from_datetime(base + ChronoDuration::hours(2)),
            activity: HeartbeatActivity::ActiveToolUse,
        },
        false,
    );
    assert_eq!(response.state, RuntimeMemberState::Active);

    assert_eq!(status_cache.member_count_for_test(), 4096);
    assert_eq!(status_cache.member_state_for_test(&team, &member), None);
    let scoped_snapshot = status_cache.snapshot_for_members_for_test([
        (
            team.clone(),
            "trigger-member".parse().expect("trigger member"),
        ),
        (team.clone(), member.clone()),
    ]);
    assert_eq!(scoped_snapshot.member_counts.active_members, 1);
    assert_eq!(scoped_snapshot.member_counts.unknown_members, 1);
}

#[test]
fn heartbeat_retries_identity_conflict_after_old_pid_dies() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");

    install_test_roster(&db_path, &[ROLE_TEAM_LEAD]);

    let status_cache = RuntimeStatusCache::new();
    let dispatcher = DaemonRequestDispatcher::new_for_test(atm_home, status_cache.clone(), db_path);
    let team = test_team().clone();
    let member: AgentName = ROLE_TEAM_LEAD.parse().expect("member");

    status_cache.insert_member_for_test(
        team.clone(),
        member.clone(),
        Some(u32::MAX),
        RuntimeMemberState::IdentityConflict,
        None,
    );

    let response = dispatcher
        .dispatch(RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: team.clone(),
            member: member.clone(),
            pid: std::process::id(),
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::ActiveToolUse,
        }))
        .expect("retry heartbeat");

    match response {
        ResponseEnvelope::Heartbeat(response) => {
            assert!(response.pid_changed);
            assert_eq!(response.state, RuntimeMemberState::Active);
        }
        other => panic!("expected heartbeat response, got {other:?}"),
    }
}

#[test]
fn identity_conflict_insert_evicts_oldest_conflict_when_cache_is_full() {
    use chrono::{Duration as ChronoDuration, Utc};

    let status_cache = RuntimeStatusCache::new();
    let team = test_team().clone();
    let oldest_member: AgentName = "qa-oldest".parse().expect("member");
    let base = Utc::now();

    for index in 0..MAX_STATUS_CACHE_ENTRIES {
        let member_name: AgentName = if index == 0 {
            oldest_member.clone()
        } else {
            format!("conflict-{index}").parse().expect("member")
        };
        status_cache.insert_member_for_test(
            team.clone(),
            member_name,
            Some(index as u32 + 1),
            RuntimeMemberState::IdentityConflict,
            Some(IsoTimestamp::from_datetime(
                base + ChronoDuration::seconds(index as i64),
            )),
        );
    }

    let request = TeamMemberHeartbeatRequest {
        team: team.clone(),
        member: "qa-trigger".parse().expect("member"),
        pid: std::process::id(),
        observed_at: IsoTimestamp::from_datetime(base + ChronoDuration::hours(1)),
        activity: HeartbeatActivity::ActiveToolUse,
    };
    status_cache.record_identity_conflict_for_test(&request, u32::MAX);

    assert_eq!(
        status_cache.member_count_for_test(),
        MAX_STATUS_CACHE_ENTRIES
    );
    assert_eq!(
        status_cache.member_state_for_test(&team, &oldest_member),
        None
    );
    assert_eq!(
        status_cache.member_state_for_test(&team, &request.member),
        Some(RuntimeMemberState::IdentityConflict)
    );
}

#[test]
#[serial_test::serial(env)]
fn doctor_projects_degraded_runtime_when_member_identity_conflicts_exist() {
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");
    let _env = EnvGuard::set_many([(
        SQLITE_RUNTIME_PATH_ENV,
        Some(db_path.to_str().expect("utf8 sqlite db path")),
    )]);

    install_test_roster(&db_path, &[ROLE_TEAM_LEAD]);
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD]);

    let status_cache = RuntimeStatusCache::new();
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), status_cache.clone(), db_path);
    status_cache.insert_member_for_test(
        test_team().clone(),
        ROLE_TEAM_LEAD.parse().expect("member"),
        Some(std::process::id()),
        RuntimeMemberState::IdentityConflict,
        Some(IsoTimestamp::now()),
    );

    let doctor = dispatcher
        .dispatch(RequestEnvelope::Doctor(atm_core::doctor::DoctorQuery {
            home_dir: atm_home.clone(),
            current_dir: atm_home,
            team_override: Some(test_team().clone()),
            ..atm_core::doctor::DoctorQuery::default()
        }))
        .expect("doctor response");

    match doctor {
        ResponseEnvelope::Doctor(report) => {
            assert_eq!(report.summary.status, DoctorStatus::Warning);
            let runtime_status = report.runtime_status.expect("runtime status");
            assert_eq!(runtime_status.readiness, RuntimeReadinessState::Degraded);
            let finding = report
                .findings
                .iter()
                .find(|finding| {
                    finding.code
                        == atm_core::error_codes::AtmErrorCode::WarningSendAlertStateDegraded
                })
                .expect("runtime finding");
            assert!(finding.message.contains("owner_pid="));
            assert!(finding.message.contains("unknown=1"));
        }
        other => panic!("expected doctor response, got {other:?}"),
    }
}

#[test]
#[serial_test::serial(env)]
fn doctor_projects_unavailable_runtime_when_all_members_are_offline() {
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");
    let _env = EnvGuard::set_many([(
        SQLITE_RUNTIME_PATH_ENV,
        Some(db_path.to_str().expect("utf8 sqlite db path")),
    )]);

    install_test_roster(&db_path, &[ROLE_TEAM_LEAD]);
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD]);

    let status_cache = RuntimeStatusCache::new();
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), status_cache.clone(), db_path);

    let doctor = dispatcher
        .dispatch(RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: test_team().clone(),
            member: ROLE_TEAM_LEAD.parse().expect("member"),
            pid: std::process::id(),
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::SessionEnded,
        }))
        .and_then(|_| {
            dispatcher.dispatch(RequestEnvelope::Doctor(atm_core::doctor::DoctorQuery {
                home_dir: atm_home.clone(),
                current_dir: atm_home,
                team_override: Some(test_team().clone()),
                ..atm_core::doctor::DoctorQuery::default()
            }))
        })
        .expect("doctor response");

    match doctor {
        ResponseEnvelope::Doctor(report) => {
            assert_eq!(report.summary.status, DoctorStatus::Error);
            let runtime_status = report.runtime_status.expect("runtime status");
            assert_eq!(runtime_status.liveness, RuntimeLivenessState::Running);
            assert_eq!(runtime_status.readiness, RuntimeReadinessState::Unavailable);
            assert_eq!(runtime_status.member_counts.offline_members, 1);
            let finding = report
                .findings
                .iter()
                .find(|finding| {
                    finding.code == atm_core::error_codes::AtmErrorCode::DaemonUnavailable
                })
                .expect("runtime finding");
            assert!(finding.message.contains("owner_pid="));
            assert!(finding.message.contains("degraded_ingest=false"));
        }
        other => panic!("expected doctor response, got {other:?}"),
    }
}

#[test]
#[serial_test::serial(env)]
fn doctor_client_context_reflects_caller_over_daemon_launch_environment() {
    install_retained_runtime_factory();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");
    // The daemon process env stands in for the frozen launch-time identity of
    // the long-lived singleton. It must not leak into the caller-facing
    // `client_context` (issue #548).
    let _env = EnvGuard::set_many([
        (
            SQLITE_RUNTIME_PATH_ENV,
            Some(db_path.to_str().expect("utf8 sqlite db path")),
        ),
        ("ATM_TEAM", Some("daemon-launch-team")),
        ("ATM_IDENTITY", Some("daemon-launch-identity")),
        ("ATM_ENVIRONMENT", Some("daemon-launch-environment")),
    ]);

    install_test_roster(&db_path, &[ROLE_TEAM_LEAD]);
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD]);

    let status_cache = RuntimeStatusCache::new();
    let dispatcher = DaemonRequestDispatcher::new_for_test(atm_home.clone(), status_cache, db_path);

    let doctor = dispatcher
        .dispatch(RequestEnvelope::Doctor(DoctorQuery {
            home_dir: atm_home.clone(),
            current_dir: atm_home,
            team_override: None,
            caller_team: Some(test_team().clone()),
            caller_identity: Some(
                atm_core::test_support::TEST_SENDER
                    .parse()
                    .expect("caller identity"),
            ),
        }))
        .expect("doctor response");

    match doctor {
        ResponseEnvelope::Doctor(report) => {
            // client_context tracks the caller threaded through the request.
            assert_eq!(
                report.client_context.team.as_ref().map(TeamName::as_str),
                Some(TEST_TEAM)
            );
            assert_eq!(
                report
                    .client_context
                    .identity
                    .as_ref()
                    .map(AgentName::as_str),
                Some(atm_core::test_support::TEST_SENDER)
            );
            // daemon_context intentionally omits caller-derived fields even
            // when hostile values are present in the daemon process env.
            let daemon_context = report.daemon_context.expect("daemon context");
            assert!(daemon_context.team.is_none());
            assert!(daemon_context.identity.is_none());
        }
        other => panic!("expected doctor response, got {other:?}"),
    }
}
