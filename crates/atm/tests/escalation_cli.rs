//! Process-level coverage for the Phase AX escalation recipient CLI.

use std::path::Path;
use std::process::{Command, Output};

fn run_atm(home: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_atm"))
        .args(arguments)
        .env("ATM_HOME", home)
        .env("ATM_CONFIG_HOME", home.join("config"))
        .env("ATM_LOG_DIR", home.join("logs"))
        .env("ATM_TEAMS_DIR", home.join("teams"))
        .output()
        .expect("run escalation CLI")
}

#[test]
fn invalid_escalation_address_uses_validation_exit_code() {
    let fixture = tempfile::tempdir().expect("temporary ATM home");
    let output = run_atm(fixture.path(), &["escalation", "add", "not an address"]);

    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid escalation recipient"));
}
