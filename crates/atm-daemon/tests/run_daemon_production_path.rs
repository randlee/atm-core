#![cfg(unix)]
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::time::Duration;

use atm_core::doctor::DoctorQuery;
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, read_bounded_stream};
use atm_core::test_support::EnvGuard;
use serial_test::serial;
use tempfile::TempDir;

struct ShutdownResetGuard;

impl ShutdownResetGuard {
    fn install() -> Self {
        atm_daemon::reset_shutdown_signals_for_test().expect("reset shutdown signals");
        Self
    }
}

impl Drop for ShutdownResetGuard {
    fn drop(&mut self) {
        atm_daemon::reset_shutdown_signals_for_test().expect("reset shutdown signals");
    }
}

#[test]
#[serial]
fn run_daemon_uses_production_socket_path_and_serves_requests() {
    let _shutdown_reset = ShutdownResetGuard::install();
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("workspace");
    let socket_path = tempdir.path().join("runtime").join("daemon.sock");
    std::fs::create_dir_all(&atm_home).expect("atm home");
    let atm_home_value = atm_home.display().to_string();
    let socket_value = socket_path.display().to_string();
    let _env = EnvGuard::set_many([
        ("ATM_HOME", Some(atm_home_value.as_str())),
        ("ATM_CONFIG_HOME", Some(atm_home_value.as_str())),
        ("ATM_DAEMON_SOCKET", Some(socket_value.as_str())),
    ]);

    let helper = std::thread::spawn(move || {
        for _ in 0..200 {
            match UnixStream::connect(&socket_path) {
                Ok(mut stream) => {
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
                    let response_bytes = read_bounded_stream(
                        &mut stream,
                        "read response",
                        "daemon response exceeded frame limit",
                    )
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
                    atm_daemon::request_shutdown_for_test().expect("request shutdown");
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionRefused
                            | std::io::ErrorKind::AddrNotAvailable
                            | std::io::ErrorKind::ConnectionReset
                    ) => {}
                Err(error) => panic!("connect socket: {error}"),
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("daemon socket never became connectable");
    });

    let (result_tx, result_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = result_tx.send(atm_daemon::run_daemon());
    });
    helper.join().expect("helper thread");
    let result = result_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("run_daemon completed");
    assert!(result.is_ok(), "run_daemon result: {result:?}");
}
