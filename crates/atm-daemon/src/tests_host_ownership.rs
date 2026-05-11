use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::sync::mpsc;
use std::time::Duration;

use atm_core::error_codes::AtmErrorCode;
use serial_test::serial;
use tempfile::TempDir;

use crate::host_ownership::{
    HOST_RUNTIME_OWNER_LOCK_FILE, HostOwnershipAdapter, clear_stale_recovery_signal_for_test,
    install_stale_recovery_signal_for_test,
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
#[serial]
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
    writeln!(&mut file, "{}:token-b", u32::MAX).expect("rewrite owner");
    file.sync_all().expect("resync owner");
    drop(file);
    continue_tx
        .send(())
        .expect("resume stale owner recovery after rewriting the token");

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
