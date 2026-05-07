#![cfg(unix)]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

use atm_core::doctor::DoctorQuery;
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
use atm_core::test_support::EnvGuard;
use tempfile::TempDir;

#[test]
fn run_daemon_uses_production_socket_path_and_serves_requests() {
    let tempdir = TempDir::new().expect("tempdir");
    let user_home = tempdir.path().join("user-home");
    let atm_home = tempdir.path().join("workspace");
    let socket_path = tempdir.path().join("runtime").join("daemon.sock");
    std::fs::create_dir_all(&user_home).expect("user home");
    std::fs::create_dir_all(&atm_home).expect("atm home");
    let home_value = user_home.display().to_string();
    let atm_home_value = atm_home.display().to_string();
    let socket_value = socket_path.display().to_string();
    let _env = EnvGuard::set_many([
        ("HOME", Some(home_value.as_str())),
        ("ATM_HOME", Some(atm_home_value.as_str())),
        ("ATM_DAEMON_SOCKET", Some(socket_value.as_str())),
    ]);

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
        atm_daemon::request_shutdown_for_test().expect("request shutdown");
    });

    let result = atm_daemon::run_daemon();
    helper.join().expect("helper thread");
    assert!(result.is_ok(), "run_daemon result: {result:?}");
}
