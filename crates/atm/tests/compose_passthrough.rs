//! Process-level parity checks for the local compose passthrough.
//!
//! The AN.1 fixture is intentionally used here instead of an ad-hoc template:
//! this protects the public command from drifting away from the template
//! contract that the later decomposed-message sprints consume.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use atm_core::list::ListOutcome;
use serial_test::serial;

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
    run_atm_as(args, "test-sender", "test-team")
}

fn run_atm_as(args: &[&str], identity: &str, team: &str) -> Output {
    let fixture = tempfile::tempdir().expect("temporary ATM environment");
    run_atm_in_home(args, identity, team, fixture.path())
}

fn run_atm_in_home(args: &[&str], identity: &str, team: &str, home: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_atm"))
        .current_dir(workspace_root())
        .args(args)
        .env("HOME", home)
        .env("ATM_HOME", home)
        .env("ATM_CONFIG_HOME", home.join("config"))
        .env("ATM_LOG_DIR", home.join("logs"))
        .env("ATM_TEAMS_DIR", home.join("teams"))
        .env("ATM_IDENTITY", identity)
        .env("ATM_TEAM", team)
        .output()
        .expect("run the built atm binary")
}

fn list_recipient(home: &Path, recipient: &str, team: &str) -> Output {
    run_atm_in_home(
        &["list", "--all", "--team", team, "--as", recipient, "--json"],
        recipient,
        team,
        home,
    )
}

struct OwnedDaemon(Option<Child>);

impl Drop for OwnedDaemon {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn ensure_daemon(home: &Path, recipient: &str, team: &str) -> OwnedDaemon {
    if list_recipient(home, recipient, team).status.success() {
        return OwnedDaemon(None);
    }

    let daemon_name = if cfg!(windows) {
        "atm-daemon.exe"
    } else {
        "atm-daemon"
    };
    let daemon_path = PathBuf::from(env!("CARGO_BIN_EXE_atm")).with_file_name(daemon_name);
    let mut child = Command::new(daemon_path)
        .args(["--peer-wire-security", "plaintext-test"])
        .current_dir(workspace_root())
        .env("HOME", home)
        .env("ATM_HOME", home)
        .env("ATM_CONFIG_HOME", home.join("config"))
        .env("ATM_LOG_DIR", home.join("logs"))
        .env("ATM_TEAMS_DIR", home.join("teams"))
        .env("ATM_IDENTITY", recipient)
        .env("ATM_TEAM", team)
        .env("ATM_DAEMON_READY_STDOUT", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the built Tokio/Axum daemon for the process test");
    let stdout = child.stdout.take().expect("daemon readiness pipe");
    let (ready_sender, ready_receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line.trim() == "ATM_DAEMON_READY" {
                let _ = ready_sender.send(true);
                return;
            }
        }
        let _ = ready_sender.send(false);
    });
    let owned = OwnedDaemon(Some(child));
    let ready = ready_receiver
        .recv_timeout(Duration::from_secs(30))
        .unwrap_or(false);
    let listed = list_recipient(home, recipient, team);
    assert!(
        listed.status.success(),
        "Tokio/Axum daemon ready={ready}, recipient list failed: {}",
        String::from_utf8_lossy(&listed.stderr)
    );
    owned
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
        atm_stderr.contains("template variable resolution failed"),
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
    let send_example = docs
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("atm send teammate@") && line.contains("--template"))
        .expect("team protocol documents a templated send example");
    assert!(send_example.contains(&format!("--template {template}")));
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

fn write_strict_fixture(dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let template = dir.join("strict.md.j2");
    std::fs::write(
        &template,
        "---\nrequired_variables:\n  - name\n  - missing\n---\nhello {{ name }} and {{ missing }}",
    )
    .expect("write template");
    let partial = dir.join("partial.json");
    std::fs::write(&partial, r#"{"name":"x"}"#).expect("write partial vars");
    let full = dir.join("full.json");
    std::fs::write(&full, r#"{"name":"x","missing":"y"}"#).expect("write full vars");
    (template, partial, full)
}

#[test]
fn compose_fails_closed_on_a_variable_with_no_value_and_names_it() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (template, partial, full) = write_strict_fixture(dir.path());
    let template = template.to_str().expect("utf-8");

    let output = run_atm(&[
        "compose",
        "--template",
        template,
        "--vars",
        partial.to_str().expect("utf-8"),
    ]);
    assert!(!output.status.success(), "unbound variable must fail");
    assert!(output.stdout.is_empty(), "failed composition has no body");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_VAL_MISSING_REQUIRED: missing required variable: missing"),
        "must preserve the stable adapter validation code: {stderr}"
    );
    assert!(
        stderr.contains("missing"),
        "must name the variable: {stderr}"
    );
    assert!(
        stderr.contains("strict.md.j2"),
        "must name the template: {stderr}"
    );
    assert_eq!(
        stderr.matches("strict.md.j2").count(),
        1,
        "the composition boundary must add its template context exactly once: {stderr}"
    );

    let output = run_atm(&[
        "compose",
        "--template",
        template,
        "--vars",
        full.to_str().expect("utf-8"),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end(),
        "hello x and y"
    );
}

#[test]
#[serial]
fn read_fails_closed_on_a_stored_decomposed_message_with_a_missing_variable() {
    let unique = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after Unix epoch")
            .as_nanos()
    );
    let identity = format!("stored-render-sender-{unique}");
    let team = format!("stored-render-test-{unique}");
    let dir = tempfile::tempdir().expect("temp dir");
    let home = tempfile::tempdir().expect("temporary ATM environment");
    let recipient_identity = format!("stored-render-reader-{unique}");
    let daemon = ensure_daemon(home.path(), &recipient_identity, &team);
    let registered = run_atm_in_home(
        &["teams", "add-member", &team, &recipient_identity, "--json"],
        &identity,
        &team,
        home.path(),
    );
    assert!(
        registered.status.success(),
        "register recipient: {}",
        String::from_utf8_lossy(&registered.stderr)
    );
    let (template, _partial, full) = write_strict_fixture(dir.path());
    let recipient = format!("{recipient_identity}@{team}");
    let admitted = run_atm_in_home(
        &[
            "send",
            &recipient,
            "--template",
            template.to_str().expect("utf-8"),
            "--vars",
            full.to_str().expect("utf-8"),
            "--json",
        ],
        &identity,
        &team,
        home.path(),
    );
    assert!(
        admitted.status.success(),
        "fully bound send must seed a decomposed message: {}",
        String::from_utf8_lossy(&admitted.stderr)
    );
    let admitted: serde_json::Value =
        serde_json::from_slice(&admitted.stdout).expect("send outcome JSON");
    let message_id = admitted["message_id"]
        .as_str()
        .expect("send outcome has message ID")
        .to_owned();

    drop(daemon);
    atm_runtime_test_support::replace_decomposed_message_vars_for_test(
        atm_core::home::host_mail_db_path().expect("host-scoped mailbox database path"),
        &message_id,
        r#"{"name":"x"}"#,
    )
    .expect("seed stored missing-variable fixture");
    let _daemon = ensure_daemon(home.path(), &recipient_identity, &team);
    let reload_member = format!("stored-render-reload-{unique}");
    let reloaded = run_atm_in_home(
        &["teams", "add-member", &team, &reload_member, "--json"],
        &identity,
        &team,
        home.path(),
    );
    assert!(
        reloaded.status.success(),
        "reload daemon state after seeding fixture: {}",
        String::from_utf8_lossy(&reloaded.stderr)
    );

    let output = run_atm_in_home(
        &["read", "--all", "--message-id", &message_id],
        &recipient_identity,
        &team,
        home.path(),
    );
    atm_runtime_test_support::replace_decomposed_message_vars_for_test(
        atm_core::home::host_mail_db_path().expect("host-scoped mailbox database path"),
        &message_id,
        r#"{"name":"x","missing":"y"}"#,
    )
    .expect("restore stored render fixture");
    assert!(
        !output.status.success(),
        "stored render with an unbound variable must fail; stdout: {}; stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty(), "failed read has no rendered body");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("missing"),
        "must name the stored missing variable: {stderr}"
    );
    assert!(
        stderr.contains("ERR_VAL_MISSING_REQUIRED: missing required variable: missing"),
        "must preserve the stable adapter validation code: {stderr}"
    );
}

#[test]
#[serial]
fn send_render_fails_closed_on_missing_variables() {
    let unique = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after Unix epoch")
            .as_nanos()
    );
    let identity = format!("render-reader-{unique}");
    let team = format!("render-test-{unique}");
    let dir = tempfile::tempdir().expect("temp dir");
    let home = tempfile::tempdir().expect("temporary ATM environment");
    let recipient_identity = format!("recipient-{unique}");
    let _daemon = ensure_daemon(home.path(), &recipient_identity, &team);
    let registered = run_atm_in_home(
        &["teams", "add-member", &team, &recipient_identity, "--json"],
        &identity,
        &team,
        home.path(),
    );
    assert!(
        registered.status.success(),
        "register recipient: {}",
        String::from_utf8_lossy(&registered.stderr)
    );
    let before = list_recipient(home.path(), &recipient_identity, &team);
    assert!(
        before.status.success(),
        "recipient list failed: {}",
        String::from_utf8_lossy(&before.stderr)
    );
    let before: ListOutcome = serde_json::from_slice(&before.stdout).expect("recipient list JSON");
    assert!(before.rows.is_empty(), "recipient mailbox must start empty");
    let (template, partial, full) = write_strict_fixture(dir.path());
    let recipient = format!("{recipient_identity}@{team}");
    let output = run_atm_in_home(
        &[
            "send",
            &recipient,
            "--template",
            template.to_str().expect("utf-8"),
            "--vars",
            partial.to_str().expect("utf-8"),
            "--json",
        ],
        &identity,
        &team,
        home.path(),
    );

    assert!(!output.status.success(), "unbound variable must fail");
    assert!(output.stdout.is_empty(), "failed send has no body");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("missing"),
        "must name the variable: {stderr}"
    );
    assert!(
        stderr.contains("ERR_VAL_MISSING_REQUIRED: missing required variable: missing"),
        "must report the adapter-owned strict-policy failure: {stderr}"
    );
    assert!(
        stderr.contains("strict.md.j2"),
        "must name the template: {stderr}"
    );
    let after = list_recipient(home.path(), &recipient_identity, &team);
    assert!(
        after.status.success(),
        "recipient list failed after rejected send: {}",
        String::from_utf8_lossy(&after.stderr)
    );
    let after: ListOutcome = serde_json::from_slice(&after.stdout).expect("recipient list JSON");
    assert!(
        after.rows.is_empty(),
        "rejected send must not persist a recipient mailbox row"
    );

    let accepted = run_atm_in_home(
        &[
            "send",
            &recipient,
            "--template",
            template.to_str().expect("utf-8"),
            "--vars",
            full.to_str().expect("utf-8"),
            "--json",
        ],
        &identity,
        &team,
        home.path(),
    );
    assert!(
        accepted.status.success(),
        "fully bound send must succeed: {}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let delivered = list_recipient(home.path(), &recipient_identity, &team);
    assert!(
        delivered.status.success(),
        "recipient list failed after accepted send: {}",
        String::from_utf8_lossy(&delivered.stderr)
    );
    let delivered: ListOutcome =
        serde_json::from_slice(&delivered.stdout).expect("recipient list JSON");
    assert_eq!(
        delivered.rows.len(),
        1,
        "accepted send must appear in the same recipient mailbox"
    );
}
