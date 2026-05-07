use super::runtime_health::{DaemonRequestDispatcher, RuntimeStatusCache};
use super::{
    ActiveConnectionRegistry, DaemonShutdownSignals, HOST_RUNTIME_OWNER_LOCK_FILE, SingletonGuard,
    host_runtime_lock_path_from_home, reset_shutdown_signals_for_test,
};
use atm_core::boundary::RequestDispatcher;
use atm_core::doctor::DoctorStatus;
use atm_core::error_codes::AtmErrorCode;
use atm_core::protocol::{
    HeartbeatActivity, RequestEnvelope, ResponseEnvelope, RuntimeLivenessState, RuntimeMemberState,
    RuntimeReadinessState, TeamMemberHeartbeatRequest,
};
use atm_core::schema::{AgentMember, TeamConfig};
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use atm_rusqlite::assemble_boundary;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, mpsc};
use std::time::Duration;
use tempfile::TempDir;

struct SignalResetGuard {
    _private: (),
}

impl SignalResetGuard {
    fn install() -> Self {
        reset_shutdown_signals_for_test().expect("reset signals");
        Self { _private: () }
    }
}

impl Drop for SignalResetGuard {
    fn drop(&mut self) {
        reset_shutdown_signals_for_test().expect("reset signals");
    }
}

#[test]
fn daemon_shutdown_signals_install_is_repeatable() {
    let _reset = SignalResetGuard::install();
    let first = DaemonShutdownSignals::install().expect("first install");
    first
        .terminate
        .store(true, std::sync::atomic::Ordering::SeqCst);
    first
        .reload
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let second = DaemonShutdownSignals::install().expect("second install");

    assert!(second.terminate.load(std::sync::atomic::Ordering::SeqCst));
    assert!(second.reload.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn daemon_host_runtime_lock_path_ignores_atm_home() {
    let tempdir = TempDir::new().expect("tempdir");
    let user_home = tempdir.path().join("user-home");
    let atm_home = tempdir.path().join("workspace").join(".atm-home");
    let runtime_dir = atm_core::home::host_runtime_dir_from_home(&user_home);
    let path = host_runtime_lock_path_from_home(&runtime_dir, HOST_RUNTIME_OWNER_LOCK_FILE);

    assert_eq!(
        path,
        user_home.join(".atm").join("daemon").join("owner.lock")
    );
    assert!(
        !path.starts_with(&atm_home),
        "daemon singleton lock must remain OS-home scoped"
    );
}

#[test]
fn singleton_guard_is_host_wide_across_different_socket_paths() {
    let tempdir = TempDir::new().expect("tempdir");
    let runtime_dir = atm_core::home::host_runtime_dir_from_home(tempdir.path());

    let first_socket = tempdir.path().join("one.sock");
    let second_socket = tempdir.path().join("other").join("two.sock");
    let first = SingletonGuard::acquire_at(
        &first_socket,
        host_runtime_lock_path_from_home(&runtime_dir, HOST_RUNTIME_OWNER_LOCK_FILE),
    )
    .expect("first singleton");
    let error = SingletonGuard::acquire_at(
        &second_socket,
        host_runtime_lock_path_from_home(&runtime_dir, HOST_RUNTIME_OWNER_LOCK_FILE),
    )
    .expect_err("second singleton");

    assert_eq!(error.code, AtmErrorCode::DaemonServingStateRejected);
    drop(first);
}

#[test]
fn singleton_guard_reports_stale_owner_record_failure() {
    let tempdir = TempDir::new().expect("tempdir");
    let runtime_dir = atm_core::home::host_runtime_dir_from_home(tempdir.path());
    let lock_path = host_runtime_lock_path_from_home(&runtime_dir, HOST_RUNTIME_OWNER_LOCK_FILE);
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .expect("open lock file");
    fs2::FileExt::try_lock_exclusive(&file).expect("lock file");
    writeln!(&mut file, "999999").expect("write owner");
    file.sync_all().expect("sync owner");

    let error =
        SingletonGuard::acquire_at(&tempdir.path().join("atm.sock"), lock_path).expect_err("stale");
    assert_eq!(error.code, AtmErrorCode::DaemonStaleOwnerRecoveryFailed);
}

#[test]
fn singleton_guard_recovers_stale_owner_once_lock_is_released() {
    let tempdir = TempDir::new().expect("tempdir");
    let runtime_dir = atm_core::home::host_runtime_dir_from_home(tempdir.path());
    let lock_path = host_runtime_lock_path_from_home(&runtime_dir, HOST_RUNTIME_OWNER_LOCK_FILE);
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .expect("open lock file");
    fs2::FileExt::try_lock_exclusive(&file).expect("lock file");
    writeln!(&mut file, "999999").expect("write owner");
    file.sync_all().expect("sync owner");

    let (release_tx, release_rx) = mpsc::channel();
    std::thread::spawn(move || {
        release_rx.recv().expect("release signal");
        drop(file);
    });

    let release_tx_clone = release_tx.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        release_tx_clone.send(()).expect("release lock");
    });

    let guard = SingletonGuard::acquire_at(&tempdir.path().join("atm.sock"), lock_path)
        .expect("stale owner recovery should succeed");
    drop(guard);
}

#[test]
fn blocked_connection_is_interrupted_on_force_cancel() {
    let tempdir = TempDir::new().expect("tempdir");
    let registry = Arc::new(ActiveConnectionRegistry::default());
    let socket_path = tempdir.path().join("daemon-test.sock");
    let listener = UnixListener::bind(&socket_path).expect("bind listener");
    let client = UnixStream::connect(&socket_path).expect("connect client");
    let (mut server, _) = listener.accept().expect("accept server");
    let _guard = registry.register(&server).expect("register");
    let (done_tx, done_rx) = mpsc::channel();

    std::thread::spawn(move || {
        let mut byte = [0u8; 1];
        let result = server.read(&mut byte).map(|_| ());
        done_tx.send(result).expect("send result");
    });

    registry.interrupt_all().expect("interrupt all");
    let result = done_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("connection finished");
    drop(client);
    assert!(result.is_ok(), "connection result: {result:?}");
}

fn install_test_roster(db_path: &std::path::Path, members: &[&str]) {
    let assembly = assemble_boundary(db_path).expect("sqlite boundary");
    assembly
        .roster_store()
        .replace_roster(atm_core::boundary::RosterStoreReplaceRosterRequest {
            team: "test-team".parse().expect("team"),
            roster: TeamConfig {
                members: members
                    .iter()
                    .map(|name| AgentMember::with_name((*name).parse().expect("member")))
                    .collect(),
                ..Default::default()
            },
            source: Some("daemon-heartbeat-test".to_string()),
        })
        .expect("replace roster");
}

fn write_team_config(home_dir: &std::path::Path, members: &[&str]) {
    let team_dir = home_dir.join(".claude").join("teams").join("test-team");
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

    install_test_roster(&db_path, &["team-lead", "qa-a"]);
    write_team_config(&atm_home, &["team-lead", "qa-a"]);

    let status_cache = RuntimeStatusCache::new();
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), status_cache.clone(), db_path);
    let team: TeamName = "test-team".parse().expect("team");
    let member: AgentName = "team-lead".parse().expect("member");

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

    install_test_roster(&db_path, &["team-lead", "qa-a"]);
    write_team_config(&atm_home, &["team-lead", "qa-a"]);

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
fn heartbeat_rejects_live_pid_conflict() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");

    install_test_roster(&db_path, &["team-lead"]);

    let status_cache = RuntimeStatusCache::new();
    let dispatcher = DaemonRequestDispatcher::new_for_test(atm_home, status_cache.clone(), db_path);
    let team: TeamName = "test-team".parse().expect("team");
    let member: AgentName = "team-lead".parse().expect("member");

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

    install_test_roster(&db_path, &["team-lead"]);

    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home, RuntimeStatusCache::new(), db_path);
    let team: TeamName = "test-team".parse().expect("team");
    let member: AgentName = "team-lead".parse().expect("member");

    dispatcher
        .dispatch(RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: team.clone(),
            member: member.clone(),
            pid: 999_999,
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
fn doctor_projects_degraded_runtime_when_sqlite_is_unavailable() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");

    install_test_roster(&db_path, &["team-lead"]);
    write_team_config(&atm_home, &["team-lead"]);

    let status_cache = RuntimeStatusCache::new();
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), status_cache.clone(), db_path);
    status_cache.mark_sqlite_unavailable();

    let doctor = dispatcher
        .dispatch(RequestEnvelope::Doctor(atm_core::doctor::DoctorQuery {
            home_dir: atm_home.clone(),
            current_dir: atm_home,
            team_override: Some("test-team".parse().expect("team")),
        }))
        .expect("doctor response");

    match doctor {
        ResponseEnvelope::Doctor(report) => {
            assert_eq!(report.summary.status, DoctorStatus::Warning);
            let runtime_status = report.runtime_status.expect("runtime status");
            assert_eq!(runtime_status.readiness, RuntimeReadinessState::Degraded);
        }
        other => panic!("expected doctor response, got {other:?}"),
    }
}
