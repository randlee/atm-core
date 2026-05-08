#![cfg(unix)]
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};

use atm_core::doctor::DoctorQuery;
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, read_bounded_stream};
use atm_core::test_support::EnvGuard;
use serial_test::serial;
use tempfile::TempDir;

#[test]
#[serial(env)]
fn run_daemon_uses_production_socket_path_and_serves_requests() {
    let tempdir = TempDir::new().expect("tempdir");
    let atm_home = tempdir.path().join("workspace");
    let user_home = tempdir.path().join("user-home");
    let socket_path = tempdir.path().join("runtime").join("daemon.sock");
    std::fs::create_dir_all(&atm_home).expect("atm home");
    std::fs::create_dir_all(&user_home).expect("user home");
    let atm_home_value = atm_home.display().to_string();
    let user_home_value = user_home.display().to_string();
    let socket_value = socket_path.display().to_string();
    let _env = EnvGuard::set_many([
        ("ATM_HOME", Some(atm_home_value.as_str())),
        ("ATM_CONFIG_HOME", Some(atm_home_value.as_str())),
        ("ATM_DAEMON_SOCKET", Some(socket_value.as_str())),
        ("HOME", Some(user_home_value.as_str())),
    ]);
    let daemon_bin = std::env::var("CARGO_BIN_EXE_atm-daemon").expect("atm-daemon binary path");

    let mut child = Command::new(daemon_bin)
        .env("ATM_HOME", &atm_home_value)
        .env("ATM_CONFIG_HOME", &atm_home_value)
        .env("ATM_DAEMON_SOCKET", &socket_value)
        .env("ATM_DAEMON_READY_STDOUT", "1")
        .env("HOME", &user_home_value)
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn atm-daemon");
    let stdout = child.stdout.take().expect("child stdout");
    let mut stdout = BufReader::new(stdout);
    let mut ready_line = String::new();
    stdout.read_line(&mut ready_line).expect("read ready line");
    assert!(
        ready_line.contains("ATM_DAEMON_READY"),
        "daemon child did not emit ready signal: {ready_line:?}"
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

    child.kill().expect("terminate daemon child");
    let status = child.wait().expect("wait for daemon child");
    assert!(
        !status.success() || status.code().is_none(),
        "daemon child should exit once terminated: {status}"
    );
}
