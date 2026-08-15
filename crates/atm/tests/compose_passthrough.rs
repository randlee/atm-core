//! Process-level parity checks for the local compose passthrough.
//!
//! The AN.1 fixture is intentionally used here instead of an ad-hoc template:
//! this protects the public command from drifting away from the template
//! contract that the later decomposed-message sprints consume.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("agent-team-mail crate has a workspace root")
        .to_path_buf()
}

fn fixture_template() -> PathBuf {
    workspace_root().join("docs/plans/phase-an/fixtures/task-assignment.xml.j2")
}

fn fixture_vars() -> PathBuf {
    workspace_root().join("docs/plans/phase-an/fixtures/task-vars.json")
}

fn run_sc_compose(args: &[&str]) -> Output {
    Command::new("sc-compose")
        .current_dir(workspace_root())
        .args(args)
        .output()
        .expect("CI installs the pinned sc-compose CLI before running passthrough tests")
}

fn run_atm(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_atm"))
        .current_dir(workspace_root())
        .args(args)
        .output()
        .expect("run the built atm binary")
}

#[test]
fn compose_matches_sc_compose_fixture_stdout_byte_for_byte() {
    let template = fixture_template();
    let vars = fixture_vars();
    let template = template.to_str().expect("fixture path is UTF-8");
    let vars = vars.to_str().expect("fixture path is UTF-8");
    let direct = run_sc_compose(&[
        "render",
        "--root",
        ".",
        "--file",
        template,
        "--var-file",
        vars,
    ]);
    let passthrough = run_atm(&["compose", "--template", template, "--vars", vars]);

    assert!(
        direct.status.success(),
        "direct sc-compose render failed: {}",
        String::from_utf8_lossy(&direct.stderr)
    );
    assert!(
        passthrough.status.success(),
        "atm compose failed: {}",
        String::from_utf8_lossy(&passthrough.stderr)
    );
    assert_eq!(
        passthrough.stdout, direct.stdout,
        "atm compose must preserve the direct sc-compose stdout bytes"
    );
}

#[test]
fn compose_matches_validation_failure_status_and_diagnostic() {
    let template = fixture_template();
    let template = template.to_str().expect("fixture path is UTF-8");
    let direct = run_sc_compose(&["render", "--root", ".", "--file", template]);
    let passthrough = run_atm(&["compose", "--template", template]);

    assert_eq!(
        passthrough.status.code(),
        direct.status.code(),
        "validation failure must retain sc-compose's exit code"
    );
    assert!(
        passthrough.stdout.is_empty(),
        "failed composition has no body"
    );
    let direct_stderr = String::from_utf8_lossy(&direct.stderr);
    let atm_stderr = String::from_utf8_lossy(&passthrough.stderr);
    assert!(
        direct_stderr.contains("ERR_VAL_MISSING_REQUIRED"),
        "direct sc-compose must report the validation diagnostic"
    );
    assert!(
        atm_stderr.contains("template verification render failed"),
        "ATM must map the upstream validation diagnostic to its stable typed error"
    );
    assert!(
        !atm_stderr.contains(template),
        "ATM's public diagnostic must not leak the absolute template path retained in the upstream cause"
    );
}

#[test]
fn team_protocol_worked_example_executes_against_checked_in_fixture() {
    let root = workspace_root();
    let docs = std::fs::read_to_string(root.join("docs/team-protocol.md"))
        .expect("team protocol documentation must be readable");
    let template = "docs/plans/phase-an/fixtures/task-assignment.xml.j2";
    let vars = "docs/plans/phase-an/fixtures/task-vars.json";
    assert!(docs.contains(&format!("atm compose --template {template}")));
    assert!(docs.contains(&format!("atm send teammate@atm-dev --template {template}")));
    assert!(docs.contains(&format!("--vars {vars}")));
    assert!(root.join(template).is_file());
    assert!(root.join(vars).is_file());

    // Execute the preview command verbatim.  The send line is deliberately
    // asserted above rather than fired by a test: it is a mailbox mutation,
    // while compose is the side-effect-free half of the worked example.
    let output = run_atm(&["compose", "--template", template, "--vars", vars]);
    assert!(
        output.status.success(),
        "documented fixture preview failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.stdout.is_empty());
}
