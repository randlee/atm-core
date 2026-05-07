use super::{
    ActiveConnectionRegistry, DaemonShutdownSignals, HOST_RUNTIME_OWNER_LOCK_FILE, SingletonGuard,
    drain_active_connections_for_shutdown, host_runtime_lock_path_from_home,
};
use atm_core::error_codes::AtmErrorCode;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::time::Duration;
use tempfile::TempDir;

#[test]
fn daemon_shutdown_signals_install_is_repeatable() {
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
    second
        .terminate
        .store(false, std::sync::atomic::Ordering::SeqCst);
    second
        .reload
        .store(false, std::sync::atomic::Ordering::SeqCst);
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
        std::thread::sleep(Duration::from_millis(10));
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
        .recv_timeout(Duration::from_millis(250))
        .expect("connection finished");
    drop(client);
    assert!(result.is_ok(), "connection result: {result:?}");
}

#[test]
fn serve_loop_escalates_from_graceful_deadline_to_force_cancel() {
    let tempdir = TempDir::new().expect("tempdir");
    let registry = Arc::new(ActiveConnectionRegistry::default());
    let force_shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let socket_path = tempdir.path().join("daemon-test.sock");
    let listener = UnixListener::bind(&socket_path).expect("bind listener");
    let client = UnixStream::connect(&socket_path).expect("connect client");
    let (mut server, _) = listener.accept().expect("accept server");
    let guard = registry.register(&server).expect("register");
    let (done_tx, done_rx) = mpsc::channel();

    std::thread::spawn(move || {
        let _guard = guard;
        let mut byte = [0u8; 1];
        let result = server.read(&mut byte).map(|_| ());
        done_tx.send(result).expect("send result");
    });

    drain_active_connections_for_shutdown(
        registry.as_ref(),
        force_shutdown.as_ref(),
        Duration::from_millis(50),
        Duration::from_millis(500),
        std::time::Instant::now(),
    )
    .expect("shutdown drain");

    let result = done_rx
        .recv_timeout(Duration::from_millis(250))
        .expect("connection finished");
    drop(client);
    assert!(result.is_ok(), "connection result: {result:?}");
    assert!(
        force_shutdown.load(Ordering::SeqCst),
        "serve loop should enter force-cancel after graceful deadline elapses"
    );
}
