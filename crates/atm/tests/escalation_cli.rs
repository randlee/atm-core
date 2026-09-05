//! Process-level coverage for the Phase AX escalation recipient CLI.

use std::path::Path;
use std::process::{Command, Output};

fn run_atm(home: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_atm"))
        .args(arguments)
        .env("ATM_HOME", home)
        .env("ATM_LOG_DIR", home.join("logs"))
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

#[test]
fn escalation_cli_round_trips_daemon_and_team_recipients() {
    let fixture = tempfile::tempdir().expect("temporary ATM home");

    let daemon_add = run_atm(fixture.path(), &["escalation", "add", "ops@atm-dev"]);
    assert!(daemon_add.status.success());
    let team_add = run_atm(
        fixture.path(),
        &["escalation", "add", "team-ops@atm-dev", "--team", "team-a"],
    );
    assert!(team_add.status.success());

    let daemon_list = run_atm(fixture.path(), &["escalation", "list"]);
    assert_eq!(
        String::from_utf8_lossy(&daemon_list.stdout).trim(),
        "ops@atm-dev"
    );
    let team_list = run_atm(fixture.path(), &["escalation", "list", "--team", "team-a"]);
    assert_eq!(
        String::from_utf8_lossy(&team_list.stdout).trim(),
        "team-ops@atm-dev"
    );

    let team_remove = run_atm(
        fixture.path(),
        &[
            "escalation",
            "remove",
            "team-ops@atm-dev",
            "--team",
            "team-a",
        ],
    );
    assert!(team_remove.status.success());
    let team_after_remove = run_atm(fixture.path(), &["escalation", "list", "--team", "team-a"]);
    assert!(team_after_remove.stdout.is_empty());
}
