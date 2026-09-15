//! Black-box coverage for the durable HTTPS peer-control CLI surface.
//!
//! These tests invoke the real `atm` binary, but only request Clap help. That
//! exercises command wiring without opening or mutating the host-scoped store.

use std::process::Command;

use serial_test::serial;

fn help_for(arguments: &[&str]) -> String {
    let fixture = tempfile::tempdir().expect("temporary ATM environment");
    let output = Command::new(env!("CARGO_BIN_EXE_atm"))
        .args(arguments)
        .arg("--help")
        .env("ATM_HOME", fixture.path())
        .env("ATM_CONFIG_HOME", fixture.path().join("config"))
        .env("ATM_LOG_DIR", fixture.path().join("logs"))
        .env("ATM_TEAMS_DIR", fixture.path().join("teams"))
        .output()
        .expect("run ATM peer command help");
    assert!(
        output.status.success(),
        "{} failed: {}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("help must be UTF-8")
}

#[test]
fn peer_interface_commands_are_wired_through_the_real_cli() {
    let list = help_for(&["peer", "interface", "list"]);
    let set = help_for(&["peer", "interface", "set"]);
    let remove = help_for(&["peer", "interface", "remove"]);

    assert!(list.contains("--json"));
    assert!(set.contains("--bind"));
    assert!(set.contains("--advertise-host"));
    assert!(remove.contains("--bind"));
}

#[test]
fn peer_certificate_commands_are_wired_through_the_real_cli() {
    let show = help_for(&["peer", "certificate", "show"]);
    let init = help_for(&["peer", "certificate", "init"]);

    assert!(show.contains("--json"));
    assert!(init.contains("--fingerprint"));
    assert!(init.contains("--private-key-ref"));
    assert!(init.contains("--yes"));
}

#[test]
fn peer_trust_commands_are_wired_through_the_real_cli() {
    let list = help_for(&["peer", "trust", "list"]);
    let add = help_for(&["peer", "trust", "add"]);
    let replace = help_for(&["peer", "trust", "replace"]);
    let revoke = help_for(&["peer", "trust", "revoke"]);

    assert!(list.contains("--json"));
    for help in [&add, &replace] {
        assert!(help.contains("--host"));
        assert!(help.contains("--fingerprint"));
        assert!(help.contains("--yes"));
    }
    assert!(revoke.contains("--host"));
    assert!(revoke.contains("--yes"));
}

#[test]
fn peer_mutations_reject_missing_yes_before_touching_durable_configuration() {
    let fixture = tempfile::tempdir().expect("temporary ATM environment");
    let output = Command::new(env!("CARGO_BIN_EXE_atm"))
        .args([
            "peer",
            "trust",
            "add",
            "--host",
            "peer.example",
            "--fingerprint",
            "sha256:test",
        ])
        .env("ATM_HOME", fixture.path())
        .env("ATM_CONFIG_HOME", fixture.path().join("config"))
        .env("ATM_LOG_DIR", fixture.path().join("logs"))
        .env("ATM_TEAMS_DIR", fixture.path().join("teams"))
        .output()
        .expect("run unconfirmed peer mutation");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("requires explicit --yes confirmation")
    );
}

#[test]
#[serial]
fn peer_trust_mutations_succeed_without_daemon_and_persist() {
    let runtime_scope = atm_core::home::current_host_runtime_scope().expect("host runtime scope");
    if runtime_scope.socket.exists() {
        eprintln!("skipping offline trust mutation test beside the ambient daemon");
        return;
    }

    let home = tempfile::tempdir().expect("temporary user home");
    let atm_home = tempfile::tempdir().expect("temporary ATM home");
    let config_home = atm_home.path().join("config");
    let log_dir = atm_home.path().join("logs");
    let teams_dir = atm_home.path().join("teams");
    let host = format!("eq007-offline-{}.example", std::process::id());
    let fingerprint_a = "a".repeat(64);
    let fingerprint_b = "b".repeat(64);

    let run = |arguments: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_atm"))
            .args(arguments)
            .env("HOME", home.path())
            .env("ATM_HOME", atm_home.path())
            .env("ATM_CONFIG_HOME", &config_home)
            .env("ATM_LOG_DIR", &log_dir)
            .env("ATM_TEAMS_DIR", &teams_dir)
            .output()
            .expect("run ATM peer trust command")
    };

    let add = run(&[
        "peer",
        "trust",
        "add",
        "--host",
        &host,
        "--fingerprint",
        &fingerprint_a,
        "--https-port",
        "43101",
        "--yes",
    ]);
    assert!(
        add.status.success(),
        "offline trust add failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );
    assert!(String::from_utf8_lossy(&add.stdout).contains(&format!("added trusted peer {host}")));
    assert!(
        String::from_utf8_lossy(&add.stdout)
            .contains("trusted peer change applies at the next daemon start")
    );

    let listed_after_add = run(&["peer", "trust", "list", "--json"]);
    assert!(listed_after_add.status.success());
    let listed_after_add: serde_json::Value =
        serde_json::from_slice(&listed_after_add.stdout).expect("trust list JSON after add");
    assert_eq!(listed_after_add[0]["host"], host);
    assert_eq!(listed_after_add[0]["fingerprint"], fingerprint_a);

    let replace = run(&[
        "peer",
        "trust",
        "replace",
        "--host",
        &host,
        "--fingerprint",
        &fingerprint_b,
        "--https-port",
        "43101",
        "--yes",
    ]);
    assert!(
        replace.status.success(),
        "offline trust replace failed: {}",
        String::from_utf8_lossy(&replace.stderr)
    );
    assert!(
        String::from_utf8_lossy(&replace.stdout).contains(&format!("replaced trusted peer {host}"))
    );
    assert!(
        String::from_utf8_lossy(&replace.stdout)
            .contains("trusted peer change applies at the next daemon start")
    );

    let listed_after_replace = run(&["peer", "trust", "list", "--json"]);
    assert!(listed_after_replace.status.success());
    let listed_after_replace: serde_json::Value =
        serde_json::from_slice(&listed_after_replace.stdout)
            .expect("trust list JSON after replace");
    assert_eq!(listed_after_replace[0]["host"], host);
    assert_eq!(listed_after_replace[0]["fingerprint"], fingerprint_b);

    let revoke = run(&["peer", "trust", "revoke", "--host", &host, "--yes"]);
    assert!(revoke.status.success(), "clean up test peer row");
}
