use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::sync::mpsc;
use std::time::Duration;

use atm_core::error_codes::AtmErrorCode;
use tempfile::TempDir;

use crate::host_ownership::{
    HOST_RUNTIME_OWNER_LOCK_FILE, HostOwnershipAdapter, clear_stale_recovery_signal_for_test,
    install_stale_recovery_signal_for_test, recorded_owner_identity_at_path_for_test,
    recorded_owner_identity_for_guard_for_test,
};

struct StaleRecoverySignalGuard;

impl StaleRecoverySignalGuard {
    fn install(observed_tx: mpsc::SyncSender<()>, continue_rx: mpsc::Receiver<()>) -> Self {
        install_stale_recovery_signal_for_test(observed_tx, continue_rx);
        Self
    }
}

impl Drop for StaleRecoverySignalGuard {
    fn drop(&mut self) {
        clear_stale_recovery_signal_for_test();
    }
}

fn join_with_timeout<T: Send + 'static>(
    join: std::thread::JoinHandle<T>,
    timeout: Duration,
    context: &str,
) -> T {
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("test-bounded-join".to_string())
        .spawn(move || {
            let _ = result_tx.send(join.join());
        })
        .expect("spawn bounded join helper");
    match result_rx.recv_timeout(timeout) {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => panic!("{context}: worker panicked"),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("{context}: join did not complete within {timeout:?}")
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("{context}: join helper disconnected before returning")
        }
    }
}

fn write_stale_owner_record(_lock_path: &std::path::Path, file: &mut std::fs::File, token: &str) {
    writeln!(file, "{}:{token}", u32::MAX).expect("write owner");
    file.sync_all().expect("sync owner");
    #[cfg(windows)]
    {
        let shadow_path = _lock_path.with_file_name(format!(
            "{}.meta",
            _lock_path
                .file_name()
                .expect("lock file name")
                .to_string_lossy()
        ));
        std::fs::write(&shadow_path, format!("{}:{token}\n", u32::MAX))
            .expect("write owner shadow");
    }
}

#[test]
fn daemon_host_runtime_lock_path_follows_the_explicit_home_root() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("workspace").join(".atm-home");
    let path =
        atm_core::home::host_runtime_lock_path_from_home(&atm_home, HOST_RUNTIME_OWNER_LOCK_FILE);

    assert_eq!(
        path,
        atm_home.join(".atm").join("daemon").join("owner.lock")
    );
}

#[test]
fn singleton_guard_is_host_wide_across_distinct_atm_home_roots() {
    let tempdir = TempDir::new().expect("tempdir");
    let home_a = tempdir.path().join("workspace-a").join("atm-home");
    let home_b = tempdir.path().join("workspace-b").join("atm-home");
    std::fs::create_dir_all(&home_a).expect("home a");
    std::fs::create_dir_all(&home_b).expect("home b");
    assert_ne!(
        home_a, home_b,
        "fixture requires genuinely distinct ATM_HOME roots"
    );

    // Production scope is OS-account based, never caller-home based.  The
    // isolated test host therefore maps both distinct workspaces to one owner
    // path without touching the developer's real singleton lock.
    let host_owner_lock = tempdir
        .path()
        .join("os-user-runtime")
        .join(HOST_RUNTIME_OWNER_LOCK_FILE);
    let first = HostOwnershipAdapter::acquire_at(host_owner_lock.clone()).expect("first singleton");
    let error = HostOwnershipAdapter::acquire_at(host_owner_lock)
        .expect_err("second singleton across a distinct ATM_HOME");

    assert_eq!(error.code(), AtmErrorCode::DaemonServingStateRejected);
    drop(first);
}

#[test]
#[serial_test::serial(env)]
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
    write_stale_owner_record(&lock_path, &mut file, "deadbeef");

    let error = HostOwnershipAdapter::acquire_at(lock_path).expect_err("stale");
    assert_eq!(error.code(), AtmErrorCode::DaemonStaleOwnerRecoveryFailed);
}

#[test]
#[serial_test::serial(env)]
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
    write_stale_owner_record(&lock_path, &mut file, "deadbeef");
    drop(file);

    let guard =
        HostOwnershipAdapter::acquire_at(lock_path).expect("stale owner recovery should succeed");
    drop(guard);
}

#[test]
#[serial_test::serial(env)]
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
    write_stale_owner_record(&lock_path, &mut file, "token-a");

    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (continue_tx, continue_rx) = mpsc::sync_channel(0);
    let _signal_guard = StaleRecoverySignalGuard::install(ready_tx, continue_rx);
    let lock_path_for_thread = lock_path.clone();
    let join = std::thread::spawn(move || HostOwnershipAdapter::acquire_at(lock_path_for_thread));
    ready_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("stale recovery hook did not fire within 5s");
    file.set_len(0).expect("clear record");
    file.seek(SeekFrom::Start(0)).expect("rewind");
    write_stale_owner_record(&lock_path, &mut file, "token-b");
    drop(file);
    continue_tx
        .send(())
        .expect("resume stale owner recovery after rewriting the token");

    let error = join_with_timeout(join, Duration::from_secs(15), "stale owner recovery join")
        .expect_err("token mismatch");
    assert_eq!(error.code(), AtmErrorCode::DaemonStaleOwnerRecoveryFailed);
}

#[test]
fn host_ownership_record_uses_pid_and_token_while_held_and_clears_on_release() {
    let tempdir = TempDir::new().expect("tempdir");
    let lock_path = atm_core::home::host_runtime_lock_path_from_home(
        tempdir.path(),
        HOST_RUNTIME_OWNER_LOCK_FILE,
    );
    let guard = HostOwnershipAdapter::acquire_at(lock_path.clone()).expect("acquire");

    let (pid, token) = recorded_owner_identity_for_guard_for_test(&guard)
        .expect("read owner record while held")
        .expect("pid:token");
    assert_eq!(pid, std::process::id());
    assert!(!token.is_empty(), "token should not be empty");

    drop(guard);

    assert_eq!(
        recorded_owner_identity_at_path_for_test(&lock_path).expect("read cleared record"),
        None,
        "record should be cleared on drop"
    );
}
