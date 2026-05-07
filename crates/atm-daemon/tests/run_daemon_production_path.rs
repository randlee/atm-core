#![cfg(unix)]
use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::{Mutex, OnceLock};

use atm_core::doctor::DoctorQuery;
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
use signal_hook::consts::signal::SIGTERM;
use signal_hook::low_level::raise;
use tempfile::TempDir;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvGuard {
    key: &'static str,
    original: Option<OsString>,
}

impl EnvGuard {
    fn set<V: AsRef<std::ffi::OsStr>>(key: &'static str, value: V) -> Self {
        let original = std::env::var_os(key);
        set_env_var(key, value);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match self.original.take() {
            Some(value) => set_env_var(self.key, value),
            None => remove_env_var(self.key),
        }
    }
}

fn set_env_var<K: AsRef<std::ffi::OsStr>, V: AsRef<std::ffi::OsStr>>(key: K, value: V) {
    // SAFETY: the integration test holds a process-wide mutex before mutating
    // the environment, so mutation is serialized within this test process.
    unsafe { std::env::set_var(key, value) }
}

fn remove_env_var<K: AsRef<std::ffi::OsStr>>(key: K) {
    // SAFETY: the integration test holds a process-wide mutex before mutating
    // the environment, so mutation is serialized within this test process.
    unsafe { std::env::remove_var(key) }
}

#[test]
fn run_daemon_uses_production_socket_path_and_serves_requests() {
    let _guard = env_lock().lock().expect("env lock");
    let tempdir = TempDir::new().expect("tempdir");
    let runtime_home = tempdir.path().join("runtime-home");
    let atm_home = tempdir.path().join("workspace");
    let socket_path = tempdir.path().join("runtime").join("daemon.sock");
    std::fs::create_dir_all(&runtime_home).expect("runtime home");
    std::fs::create_dir_all(&atm_home).expect("atm home");
    // R.13 singleton ownership is still OS-home scoped, so keep the host
    // runtime root isolated from the developer machine even though ATM_HOME
    // remains the canonical mailbox/runtime root under test.
    let _runtime_home = EnvGuard::set("HOME", &runtime_home);
    let _atm_home = EnvGuard::set("ATM_HOME", &atm_home);
    let _config_home = EnvGuard::set("ATM_CONFIG_HOME", &atm_home);
    let _socket = EnvGuard::set("ATM_DAEMON_SOCKET", &socket_path);

    let helper = std::thread::spawn(move || {
        for _ in 0..200 {
            if socket_path.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            socket_path.exists(),
            "daemon socket should exist after start"
        );

        let mut stream = UnixStream::connect(&socket_path).expect("connect socket");
        let request = RequestEnvelope::Doctor(DoctorQuery {
            home_dir: atm_home.clone(),
            current_dir: atm_home,
            team_override: None,
        });
        let bytes = serde_json::to_vec(&request).expect("request json");
        stream.write_all(&bytes).expect("write request");
        stream
            .shutdown(std::net::Shutdown::Write)
            .expect("shutdown write");
        let mut response_bytes = Vec::new();
        stream
            .read_to_end(&mut response_bytes)
            .expect("read response");
        let response: ResponseEnvelope =
            serde_json::from_slice(&response_bytes).expect("response json");
        assert!(
            matches!(
                response,
                ResponseEnvelope::Doctor(_) | ResponseEnvelope::Error(_)
            ),
            "daemon production path should serve a request before shutdown"
        );
        raise(SIGTERM).expect("raise SIGTERM");
    });

    let result = atm_daemon::run_daemon();
    helper.join().expect("helper thread");
    assert!(result.is_ok(), "run_daemon result: {result:?}");
}
