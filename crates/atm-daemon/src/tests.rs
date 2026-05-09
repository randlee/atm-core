use super::runtime_health::{DaemonRequestDispatcher, RuntimeStatusCache};
use super::{
    host_ownership::{
        HOST_RUNTIME_OWNER_LOCK_FILE, HostOwnershipAdapter, clear_stale_recovery_barrier_for_test,
        install_stale_recovery_barrier_for_test,
    },
    lifecycle_control::LifecycleControlSourceAdapter,
};
use atm_core::boundary::RequestDispatcher;
use atm_core::doctor::DoctorStatus;
use atm_core::error_codes::AtmErrorCode;
use atm_core::protocol::{
    HeartbeatActivity, RequestEnvelope, ResponseEnvelope, RuntimeLivenessState, RuntimeMemberState,
    RuntimeReadinessState, TeamMemberHeartbeatRequest,
};
use atm_core::schema::{AgentMember, TeamConfig};
use atm_core::test_support::ROLE_TEAM_LEAD;
use atm_core::types::{AgentName, IsoTimestamp, TeamName};
use atm_rusqlite::assemble_boundary;
use serial_test::serial;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::sync::{Arc, Barrier};
use tempfile::TempDir;

struct LifecycleFlagResetGuard<'a> {
    lifecycle: &'a LifecycleControlSourceAdapter,
}

impl<'a> LifecycleFlagResetGuard<'a> {
    fn install(lifecycle: &'a LifecycleControlSourceAdapter) -> Self {
        lifecycle.set_terminate_for_test(false);
        lifecycle.set_reload_for_test(false);
        Self { lifecycle }
    }
}

impl Drop for LifecycleFlagResetGuard<'_> {
    fn drop(&mut self) {
        self.lifecycle.set_terminate_for_test(false);
        self.lifecycle.set_reload_for_test(false);
    }
}

#[test]
fn daemon_shutdown_signals_for_test_are_isolated() {
    let first = LifecycleControlSourceAdapter::new_for_test();
    first.set_terminate_for_test(true);
    first.set_reload_for_test(true);
    let second = LifecycleControlSourceAdapter::new_for_test();

    assert!(!second.terminate_requested());
    assert!(!second.reload_requested_for_test());
}

#[test]
#[serial]
fn daemon_shutdown_signal_install_reuses_shared_flags() {
    let first = LifecycleControlSourceAdapter::install().expect("install first");
    let _reset = LifecycleFlagResetGuard::install(&first);

    let second = LifecycleControlSourceAdapter::install().expect("install second");
    first.set_reload_for_test(true);
    second.set_terminate_for_test(true);

    assert!(second.reload_requested_for_test());
    assert!(first.terminate_requested());
}

#[test]
fn daemon_host_runtime_lock_path_ignores_atm_home() {
    let tempdir = TempDir::new().expect("tempdir");
    let user_home = tempdir.path().join("user-home");
    let atm_home = tempdir.path().join("workspace").join(".atm-home");
    let path =
        atm_core::home::host_runtime_lock_path_from_home(&user_home, HOST_RUNTIME_OWNER_LOCK_FILE);

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
    let first_socket = tempdir.path().join("one.sock");
    let second_socket = tempdir.path().join("other").join("two.sock");
    let first = HostOwnershipAdapter::acquire_at(atm_core::home::host_runtime_lock_path_from_home(
        tempdir.path(),
        HOST_RUNTIME_OWNER_LOCK_FILE,
    ))
    .expect("first singleton");
    let _ = first_socket;
    let _ = second_socket;
    let error = HostOwnershipAdapter::acquire_at(atm_core::home::host_runtime_lock_path_from_home(
        tempdir.path(),
        HOST_RUNTIME_OWNER_LOCK_FILE,
    ))
    .expect_err("second singleton");

    assert_eq!(error.code, AtmErrorCode::DaemonServingStateRejected);
    drop(first);
}

#[test]
fn singleton_guard_reports_stale_owner_record_failure() {
    let tempdir = TempDir::new().expect("tempdir");
    let lock_path = atm_core::home::host_runtime_lock_path_from_home(
        tempdir.path(),
        HOST_RUNTIME_OWNER_LOCK_FILE,
    );
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
    writeln!(&mut file, "{}:deadbeef", u32::MAX).expect("write owner");
    file.sync_all().expect("sync owner");

    let error = HostOwnershipAdapter::acquire_at(lock_path).expect_err("stale");
    assert_eq!(error.code, AtmErrorCode::DaemonStaleOwnerRecoveryFailed);
}

#[test]
#[serial]
fn singleton_guard_recovers_stale_owner_once_lock_is_released() {
    let tempdir = TempDir::new().expect("tempdir");
    let lock_path = atm_core::home::host_runtime_lock_path_from_home(
        tempdir.path(),
        HOST_RUNTIME_OWNER_LOCK_FILE,
    );
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
    writeln!(&mut file, "{}:deadbeef", u32::MAX).expect("write owner");
    file.sync_all().expect("sync owner");
    drop(file);

    let guard =
        HostOwnershipAdapter::acquire_at(lock_path).expect("stale owner recovery should succeed");
    drop(guard);
}

#[test]
#[serial]
fn singleton_guard_rejects_stale_recovery_when_owner_token_changes() {
    let tempdir = TempDir::new().expect("tempdir");
    let lock_path = atm_core::home::host_runtime_lock_path_from_home(
        tempdir.path(),
        HOST_RUNTIME_OWNER_LOCK_FILE,
    );
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
    writeln!(&mut file, "{}:token-a", u32::MAX).expect("write owner");
    file.sync_all().expect("sync owner");

    let barrier = Arc::new(Barrier::new(2));
    install_stale_recovery_barrier_for_test(Arc::clone(&barrier));
    let lock_path_for_thread = lock_path.clone();
    let join = std::thread::spawn(move || HostOwnershipAdapter::acquire_at(lock_path_for_thread));
    barrier.wait();
    file.set_len(0).expect("clear record");
    file.seek(SeekFrom::Start(0)).expect("rewind");
    writeln!(&mut file, "{}:token-b", u32::MAX).expect("rewrite owner");
    file.sync_all().expect("resync owner");
    drop(file);
    clear_stale_recovery_barrier_for_test();

    let error = join.join().expect("join").expect_err("token mismatch");
    assert_eq!(error.code, AtmErrorCode::DaemonStaleOwnerRecoveryFailed);
}

#[test]
fn host_ownership_record_uses_pid_and_token_while_held_and_clears_on_release() {
    let tempdir = TempDir::new().expect("tempdir");
    let lock_path = atm_core::home::host_runtime_lock_path_from_home(
        tempdir.path(),
        HOST_RUNTIME_OWNER_LOCK_FILE,
    );
    let guard = HostOwnershipAdapter::acquire_at(lock_path.clone()).expect("acquire");

    let record = std::fs::read_to_string(&lock_path).expect("read record");
    let trimmed = record.trim();
    // The singleton tests intentionally read the same owner.lock metadata that
    // ADR-002 documents for the launch.lock -> owner.lock handoff.
    let (pid, token) = trimmed.split_once(':').expect("pid:token");
    assert_eq!(pid, std::process::id().to_string());
    assert!(!token.is_empty(), "token should not be empty");

    drop(guard);

    let cleared = std::fs::read_to_string(&lock_path).expect("read cleared record");
    assert!(
        cleared.trim().is_empty(),
        "record should be cleared on drop"
    );
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

    install_test_roster(&db_path, &[ROLE_TEAM_LEAD, "qa-a"]);
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD, "qa-a"]);

    let status_cache = RuntimeStatusCache::new();
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), status_cache.clone(), db_path);
    let team: TeamName = "test-team".parse().expect("team");
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
    let team: TeamName = "test-team".parse().expect("team");
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
fn reload_runtime_view_rejects_invalid_config_and_preserves_last_known_good_state() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");

    install_test_roster(&db_path, &[ROLE_TEAM_LEAD]);
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD]);

    let status_cache = RuntimeStatusCache::new();
    let dispatcher =
        DaemonRequestDispatcher::new_for_test(atm_home.clone(), status_cache.clone(), db_path);
    let team: TeamName = "test-team".parse().expect("team");
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
        .join("test-team")
        .join("config.json");
    std::fs::write(&config_path, br#"{"members":["team-lead",}"#).expect("invalid config");

    let error = dispatcher
        .reload_runtime_view()
        .expect_err("invalid config should be rejected");

    assert!(error.is_config(), "expected config error, got {error:?}");
    assert_eq!(
        status_cache
            .member_state_for_test(&team, &leader)
            .expect("leader state"),
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
    let team: TeamName = "test-team".parse().expect("team");
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
    let team: TeamName = "test-team".parse().expect("team");
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
fn heartbeat_demotes_evicted_member_to_explicit_unknown() {
    use chrono::{Duration as ChronoDuration, Utc};

    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("atm-home");
    std::fs::create_dir_all(&atm_home).expect("atm home dir");
    let db_path = tempdir.path().join("mail.db");

    install_test_roster(&db_path, &[ROLE_TEAM_LEAD, "qa-a"]);
    write_team_config(&atm_home, &[ROLE_TEAM_LEAD, "qa-a"]);

    let status_cache = RuntimeStatusCache::new();
    let team: TeamName = "test-team".parse().expect("team");
    let dispatcher = DaemonRequestDispatcher::new_for_test(atm_home, status_cache.clone(), db_path);
    let member: AgentName = "evicted".parse().expect("member");
    let base = Utc::now();
    status_cache
        .hydrate_member_for_test(team.clone(), member.clone(), Some(u32::MAX))
        .expect("hydrate member");

    for index in 0..=4096 {
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

    let response = dispatcher
        .dispatch(RequestEnvelope::Heartbeat(TeamMemberHeartbeatRequest {
            team: team.clone(),
            member: ROLE_TEAM_LEAD.parse().expect("member"),
            pid: std::process::id(),
            observed_at: IsoTimestamp::from_datetime(base + ChronoDuration::hours(2)),
            activity: HeartbeatActivity::ActiveToolUse,
        }))
        .expect("heartbeat");

    match response {
        ResponseEnvelope::Heartbeat(response) => {
            assert_eq!(response.state, RuntimeMemberState::Active);
        }
        other => panic!("expected heartbeat response, got {other:?}"),
    }

    assert_eq!(
        status_cache
            .member_state_for_test(&team, &member)
            .expect("member state"),
        Some(RuntimeMemberState::Unknown)
    );
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
    let team: TeamName = "test-team".parse().expect("team");
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
            team: "test-team".parse().expect("team"),
            member: ROLE_TEAM_LEAD.parse().expect("member"),
            pid: std::process::id(),
            observed_at: IsoTimestamp::now(),
            activity: HeartbeatActivity::SessionEnded,
        }))
        .and_then(|_| {
            dispatcher.dispatch(RequestEnvelope::Doctor(atm_core::doctor::DoctorQuery {
                home_dir: atm_home.clone(),
                current_dir: atm_home,
                team_override: Some("test-team".parse().expect("team")),
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
        }
        other => panic!("expected doctor response, got {other:?}"),
    }
}
