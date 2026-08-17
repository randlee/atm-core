//! Black-box coverage for the durable HTTPS peer-control CLI surface.
//!
//! These tests invoke the real `atm` binary, but only request Clap help. That
//! exercises command wiring without opening or mutating the host-scoped store.

use std::process::Command;

fn help_for(arguments: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_atm"))
        .args(arguments)
        .arg("--help")
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
    let tempdir = tempfile::TempDir::new().expect("temporary log directory");
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
        .env("ATM_LOG_DIR", tempdir.path())
        .output()
        .expect("run unconfirmed peer mutation");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("requires explicit --yes confirmation")
    );
}
