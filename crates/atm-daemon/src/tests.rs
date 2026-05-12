use super::runtime_health::{
    DaemonRequestDispatcher, MAX_STATUS_CACHE_ENTRIES, RuntimeStatusCache,
};
use super::{
    LocalIpcServerTransportAdapter,
    lifecycle_control::LifecycleControlSourceAdapter,
    local_ipc_transport::RuntimeServeHooks,
    test_support::{DoctorOnlyDispatcher, LifecycleFlagResetGuard},
};
use atm_core::boundary::RequestDispatcher;
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
use atm_core::test_support::EnvGuard;
use atm_core::test_support::ROLE_TEAM_LEAD;
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use atm_rusqlite::assemble_boundary;
#[cfg(windows)]
use interprocess::local_socket::Stream as LocalSocketStream;
use interprocess::local_socket::traits::Stream as _;
use serial_test::serial;
use std::io::Write;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::mpsc;
use std::time::Duration;
use tempfile::TempDir;

use crate::test_support::connect_daemon_local_ipc_until_ready;

const TEST_TEAM: &str = "test-team";

fn test_team() -> &'static TeamName {
    static TEST_TEAM_NAME: OnceLock<TeamName> = OnceLock::new();
    TEST_TEAM_NAME.get_or_init(|| TEST_TEAM.parse().expect("team"))
}

struct ShutdownFinalizerDrainGuard;

impl Drop for ShutdownFinalizerDrainGuard {
    fn drop(&mut self) {
        DaemonRequestDispatcher::drain_shutdown_finalizer_threads_for_test();
    }
}

#[test]
#[serial]
fn local_ipc_runtime_round_trips_doctor_requests_on_shared_transport() {
    // TempDir uniqueness is process-local; #[serial] keeps this same-host transport smoke test
    // from racing other lifecycle-control and singleton-sensitive daemon tests.
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let _env = EnvGuard::set_many([
        ("ATM_HOME", Some(atm_home.to_str().expect("utf8 atm home"))),
        ("HOME", Some(tempdir.path().to_str().expect("utf8 home"))),
        ("USERPROFILE", None),
    ]);
    let socket_path = tempdir.path().join("daemon.sock");
    let server_transport = LocalIpcServerTransportAdapter::new();
    let runtime = server_transport
        .prepare_runtime_at_socket_path(socket_path.clone())
        .expect("prepare runtime");
    let mut runtime = runtime;
    let endpoint_guard = runtime.take_endpoint_guard().expect("take endpoint guard");
    let (lifecycle, _reset) = {
        let lifecycle = LifecycleControlSourceAdapter::install().expect("install lifecycle");
        let reset = LifecycleFlagResetGuard::install(lifecycle.clone());
        (lifecycle, reset)
    };
    let dispatcher: Arc<dyn RequestDispatcher + Send + Sync> = Arc::new(DoctorOnlyDispatcher);
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
                finalize_shutdown: || {},
                publish_ready: move || {
                    ready_tx.send(()).map_err(|_| {
                        AtmError::daemon_unavailable(
                            "doctor round-trip test failed to observe the daemon ready signal",
                        )
                        .with_recovery(
                            "Rerun the same-host daemon test after restoring the bounded ready-signal handshake.",
                        )
                    })
                },
            },
        );
        serve_result_tx.send(result).expect("send serve result");
    });

    let mut stream = connect_daemon_local_ipc_until_ready(&socket_path, ready_rx);
    stream
        .set_send_timeout(Some(Duration::from_secs(5)))
        .expect("set send timeout");
    stream
        .set_recv_timeout(Some(Duration::from_secs(5)))
        .expect("set recv timeout");
    let request = RequestEnvelope::Doctor(DoctorQuery {
        home_dir: tempdir.path().join("home"),
        current_dir: tempdir.path().join("cwd"),
        team_override: None,
    });
    let request_id = atm_core::protocol::next_request_id();
    let frame = atm_core::protocol::request_to_frame_payload(request_id, request).expect("frame");
    atm_core::protocol::write_frame(&mut stream, &frame, "write doctor frame").expect("write");
    stream.flush().expect("flush");
    let response_frame =
        atm_core::protocol::read_frame(&mut stream, "read doctor frame", "doctor frame too large")
            .expect("read frame")
            .expect("response frame");
    let (response_id, response) =
        atm_core::protocol::response_from_frame_payload(response_frame).expect("decode response");
    assert_eq!(response_id, request_id);
    match response {
        ResponseEnvelope::Doctor(report) => {
            assert_eq!(report.summary.status, DoctorStatus::Healthy);
        }
        other => panic!("unexpected response: {other:?}"),
    }

    lifecycle.set_terminate_for_test(true);
    serve_result_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("recv serve result")
        .expect("serve runtime result");
    join.join().expect("join serve thread");
}

#[test]
#[serial]
fn compose_runtime_start_writes_retained_log_and_reports_healthy_observability() {
    let _drain_guard = ShutdownFinalizerDrainGuard;
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let _env = EnvGuard::set_many([
        ("ATM_HOME", Some(atm_home.to_str().expect("utf8 atm home"))),
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
    let socket_path = atm_core::protocol::daemon_socket_path().expect("daemon socket path");
    let runtime =
        crate::composition::compose_runtime(observability.clone()).expect("compose runtime");
    let (result_tx, result_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let runtime_socket_path = socket_path.clone();

    let join = std::thread::spawn(move || {
        let result = runtime.start_with_socket_path_for_test(runtime_socket_path, Some(ready_tx));
        result_tx.send(result).expect("send runtime result");
    });

    let mut stream = connect_daemon_local_ipc_until_ready(&socket_path, ready_rx);
    stream
        .set_send_timeout(Some(Duration::from_secs(5)))
        .expect("set send timeout");
    stream
        .set_recv_timeout(Some(Duration::from_secs(5)))
        .expect("set recv timeout");
    let request = RequestEnvelope::Doctor(DoctorQuery {
        home_dir: atm_home.clone(),
        current_dir: atm_home.clone(),
        team_override: None,
    });
    let request_id = atm_core::protocol::next_request_id();
    let frame = atm_core::protocol::request_to_frame_payload(request_id, request).expect("frame");
    atm_core::protocol::write_frame(&mut stream, &frame, "write doctor frame").expect("write");
    stream.flush().expect("flush");
    let response_frame =
        atm_core::protocol::read_frame(&mut stream, "read doctor frame", "doctor frame too large")
            .expect("read frame")
            .expect("response frame");
    let (response_id, response) =
        atm_core::protocol::response_from_frame_payload(response_frame).expect("decode response");
    assert_eq!(response_id, request_id);
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

#[cfg(windows)]
#[test]
#[serial]
fn windows_local_ipc_runtime_terminate_finishes_within_deadline() {
    let tempdir = TempDir::new().expect("tempdir");
    let socket_path = tempdir.path().join("daemon.sock");
    let server_transport = LocalIpcServerTransportAdapter::new();
    let runtime = server_transport
        .prepare_runtime_at_socket_path(socket_path)
        .expect("prepare runtime");
    let mut runtime = runtime;
    let endpoint_guard = runtime.take_endpoint_guard().expect("take endpoint guard");
    let (lifecycle, _reset) = {
        let lifecycle = LifecycleControlSourceAdapter::install().expect("install lifecycle");
        let reset = LifecycleFlagResetGuard::install(lifecycle.clone());
        (lifecycle, reset)
    };
    let dispatcher: Arc<dyn RequestDispatcher + Send + Sync> = Arc::new(DoctorOnlyDispatcher);
    let (serve_result_tx, serve_result_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::sync_channel(0);

    let join = std::thread::spawn(move || {
        let result = runtime.serve_with_runtime_hooks(
            dispatcher,
            RuntimeServeHooks {
                endpoint_guard,
                graceful_drain_deadline: Duration::from_millis(500),
                force_cancel_deadline: Duration::from_secs(2),
                begin_shutdown: || Ok(()),
                reload_runtime_view: || Ok(()),
                finalize_shutdown: || {},
                publish_ready: move || {
                    ready_tx.send(()).ok();
                    Ok(())
                },
            },
        );
        serve_result_tx.send(result).expect("send serve result");
    });

    let local_ipc_name =
        atm_core::protocol::daemon_local_ipc_name_from_path(&tempdir.path().join("daemon.sock"))
            .expect("ipc name");
    ready_rx.recv().expect("daemon ready");
    lifecycle.set_terminate_for_test(true);

    serve_result_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("recv serve result")
        .expect("serve runtime result");
    join.join().expect("join serve thread");
    assert!(
        LocalSocketStream::connect(local_ipc_name).is_err(),
        "windows same-host runtime should reject new local IPC connections after shutdown",
    );
}

fn install_test_roster(db_path: &std::path::Path, members: &[&str]) {
    let assembly = assemble_boundary(db_path).expect("sqlite boundary");
    assembly
        .roster_store()
        .replace_roster(atm_core::boundary::RosterStoreReplaceRosterRequest {
            team: test_team().clone(),
            members: members
                .iter()
                .map(|name| {
                    atm_core::boundary::RosterMemberRecord::from_claude_code_member(
                        test_team().clone(),
                        AgentMember::with_name((*name).parse().expect("member")),
                    )
                })
                .collect(),
            source: Some(
                atm_core::boundary::ReplaySource::new("daemon-heartbeat-test")
                    .expect("replay source"),
            ),
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

#[test]
fn heartbeat_updates_status_cache_and_doctor_projection() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");

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

    let snapshot = status_cache.snapshot().expect("snapshot");
    assert_eq!(snapshot.liveness, RuntimeLivenessState::Running);
    assert_eq!(snapshot.readiness, RuntimeReadinessState::Ready);
    assert_eq!(snapshot.member_counts.active_members, 1);

    let doctor = dispatcher
        .dispatch(RequestEnvelope::Doctor(atm_core::doctor::DoctorQuery {
            home_dir: atm_home.clone(),
            current_dir: atm_home.clone(),
            team_override: Some(team.clone()),
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

    let snapshot = status_cache.snapshot().expect("snapshot");
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
        status_cache
            .member_state_for_test(&team, &leader)
            .expect("leader state"),
        Some(RuntimeMemberState::Active)
    );
    assert_eq!(
        status_cache
            .member_state_for_test(&team, &qa)
            .expect("qa state"),
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
    std::fs::write(&config_path, br#"{"members":["team-lead",}"#).expect("invalid config");

    dispatcher
        .reload_runtime_view()
        .expect("invalid config should be ignored once roster truth is in sqlite");
    assert_eq!(
        status_cache
            .member_state_for_test(&team, &leader)
            .expect("leader state"),
        Some(RuntimeMemberState::Active)
    );
}

#[test]
#[serial]
fn finalize_shutdown_drains_test_tracked_finalizer_threads() {
    let _drain_guard = ShutdownFinalizerDrainGuard;
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");

    install_test_roster(&db_path, &[ROLE_TEAM_LEAD]);
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home, RuntimeStatusCache::new(), db_path);

    dispatcher.finalize_shutdown();
    DaemonRequestDispatcher::drain_shutdown_finalizer_threads_for_test();
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

    assert_eq!(error.code, AtmErrorCode::IdentityConflict);
    assert_eq!(
        error.message,
        "ATM_IDENTITY_CONFLICT: stop and report to user immediately"
    );
    assert_eq!(
        status_cache
            .member_state_for_test(&team, &member)
            .expect("member state"),
        Some(RuntimeMemberState::IdentityConflict)
    );
    let snapshot = status_cache.snapshot().expect("snapshot");
    assert_eq!(snapshot.readiness, RuntimeReadinessState::Degraded);
    assert!(
        snapshot
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("identity conflict"))
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
    status_cache
        .insert_member_for_test(
            team.clone(),
            member.clone(),
            Some(u32::MAX),
            RuntimeMemberState::Idle,
            Some(IsoTimestamp::from_datetime(base)),
        )
        .expect("seed evicted member");

    for index in 0..4095 {
        let member_name: AgentName = format!("member-{index}").parse().expect("member");
        status_cache
            .insert_member_for_test(
                team.clone(),
                member_name,
                Some(index as u32 + 2),
                RuntimeMemberState::Idle,
                Some(IsoTimestamp::from_datetime(
                    base + ChronoDuration::seconds(index as i64 + 1),
                )),
            )
            .expect("insert member");
    }

    let response = status_cache
        .record_heartbeat_for_test(
            &TeamMemberHeartbeatRequest {
                team: team.clone(),
                member: "trigger-member".parse().expect("member"),
                pid: std::process::id(),
                observed_at: IsoTimestamp::from_datetime(base + ChronoDuration::hours(2)),
                activity: HeartbeatActivity::ActiveToolUse,
            },
            false,
        )
        .expect("heartbeat");
    assert_eq!(response.state, RuntimeMemberState::Active);

    assert_eq!(
        status_cache.member_count_for_test().expect("member count"),
        4096
    );
    assert_eq!(
        status_cache
            .member_state_for_test(&team, &member)
            .expect("member state"),
        None
    );
    let scoped_snapshot = status_cache
        .snapshot_for_members_for_test([
            (
                team.clone(),
                "trigger-member".parse().expect("trigger member"),
            ),
            (team.clone(), member.clone()),
        ])
        .expect("scoped snapshot");
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

    status_cache
        .insert_member_for_test(
            team.clone(),
            member.clone(),
            Some(u32::MAX),
            RuntimeMemberState::IdentityConflict,
            None,
        )
        .expect("seed stale conflict");

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
        status_cache
            .insert_member_for_test(
                team.clone(),
                member_name,
                Some(index as u32 + 1),
                RuntimeMemberState::IdentityConflict,
                Some(IsoTimestamp::from_datetime(
                    base + ChronoDuration::seconds(index as i64),
                )),
            )
            .expect("seed conflict member");
    }

    let request = TeamMemberHeartbeatRequest {
        team: team.clone(),
        member: "qa-trigger".parse().expect("member"),
        pid: std::process::id(),
        observed_at: IsoTimestamp::from_datetime(base + ChronoDuration::hours(1)),
        activity: HeartbeatActivity::ActiveToolUse,
    };
    status_cache
        .record_identity_conflict_for_test(&request, u32::MAX)
        .expect("record conflict");

    assert_eq!(
        status_cache.member_count_for_test().expect("member count"),
        MAX_STATUS_CACHE_ENTRIES
    );
    assert_eq!(
        status_cache
            .member_state_for_test(&team, &oldest_member)
            .expect("oldest member state"),
        None
    );
    assert_eq!(
        status_cache
            .member_state_for_test(&team, &request.member)
            .expect("trigger member state"),
        Some(RuntimeMemberState::IdentityConflict)
    );
}

#[test]
fn doctor_projects_degraded_runtime_when_sqlite_is_unavailable() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");

    install_test_roster(&db_path, &[ROLE_TEAM_LEAD]);
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD]);

    let status_cache = RuntimeStatusCache::new();
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), status_cache.clone(), db_path);
    status_cache.mark_sqlite_unavailable();

    let doctor = dispatcher
        .dispatch(RequestEnvelope::Doctor(atm_core::doctor::DoctorQuery {
            home_dir: atm_home.clone(),
            current_dir: atm_home,
            team_override: Some(test_team().clone()),
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
                        == atm_core::error_codes::AtmErrorCode::WarningObservabilityHealthDegraded
                })
                .expect("runtime finding");
            assert!(finding.message.contains("sqlite_ready=false"));
        }
        other => panic!("expected doctor response, got {other:?}"),
    }
}

#[test]
fn doctor_projects_unavailable_runtime_when_all_members_are_offline() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");

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
            assert!(finding.message.contains("sqlite_ready=true"));
        }
        other => panic!("expected doctor response, got {other:?}"),
    }
}
