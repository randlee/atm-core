#![cfg(unix)]

mod support;

use std::fs;
use std::process::Command;

use support::{TEST_DAEMON, TEST_LEAD, TEST_QA, TEST_SENDER, TEST_TEAM};

use atm_core::schema::{AgentMember, TeamConfig};
use serde_json::Value;

#[test]
fn test_doctor_reports_healthy_observability_with_real_adapter() {
    let fixture = Fixture::new(&[TEST_LEAD]);

    let output = fixture.run(&["doctor", "--json"], &[]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["summary"]["status"], "healthy");
    assert_eq!(parsed["findings"][0]["severity"], "info");
    assert_eq!(parsed["findings"][0]["code"], "ATM_OBSERVABILITY_HEALTH_OK");
    assert_eq!(parsed["observability"]["logging_state"], "healthy");
    assert_eq!(parsed["observability"]["query_state"], "healthy");
    assert_eq!(
        parsed["observability"]["active_log_path"],
        fixture.active_log_path().display().to_string()
    );
}

#[test]
fn test_doctor_reports_degraded_observability_with_real_fault_injection() {
    let fixture = Fixture::new(&[TEST_LEAD]);

    let output = fixture.run(
        &["doctor", "--json"],
        &[("ATM_OBSERVABILITY_RETAINED_SINK_FAULT", "degraded")],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["summary"]["status"], "warning");
    assert_eq!(parsed["findings"][0]["severity"], "warning");
    assert_eq!(
        parsed["findings"][0]["code"],
        "ATM_WARNING_OBSERVABILITY_HEALTH_DEGRADED"
    );
    assert_eq!(parsed["observability"]["logging_state"], "degraded");
    assert_eq!(parsed["observability"]["query_state"], "healthy");
}

#[test]
fn test_doctor_reports_unavailable_observability_with_real_fault_injection() {
    let fixture = Fixture::new(&[TEST_LEAD]);

    let output = fixture.run(
        &["doctor", "--json"],
        &[("ATM_OBSERVABILITY_RETAINED_SINK_FAULT", "unavailable")],
    );

    assert!(!output.status.success());
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["summary"]["status"], "error");
    assert_eq!(parsed["findings"][0]["severity"], "error");
    assert_eq!(
        parsed["findings"][0]["code"],
        "ATM_OBSERVABILITY_HEALTH_FAILED"
    );
    assert_eq!(parsed["observability"]["logging_state"], "unavailable");
    assert_eq!(parsed["observability"]["query_state"], "healthy");
}

#[test]
fn test_doctor_reports_obsolete_identity_drift_warning() {
    let fixture = Fixture::new(&[TEST_LEAD]);
    fixture.write_atm_config(&format!("[atm]\nidentity = \"{}\"\n", TEST_SENDER));

    let output = fixture.run(&["doctor", "--json"], &[]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    assert_eq!(parsed["summary"]["status"], "warning");
    let findings = parsed["findings"].as_array().expect("findings array");
    assert!(
        findings
            .iter()
            .any(|finding| finding["code"] == "ATM_WARNING_IDENTITY_DRIFT"),
        "stdout: {}",
        String::from_utf8(output.stdout.clone()).expect("stdout utf8")
    );
}

#[test]
fn test_doctor_reports_missing_baseline_team_member() {
    let fixture = Fixture::new(&[TEST_LEAD, TEST_SENDER]);
    fixture.write_atm_config(&format!(
        "[atm]\ndefault_team = \"{}\"\nteam_members = [\"{}\", \"{}\", \"{}\"]\n",
        TEST_TEAM, TEST_LEAD, TEST_SENDER, TEST_QA
    ));

    let output = fixture.run(&["doctor", "--json"], &[]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    let findings = parsed["findings"].as_array().expect("findings array");
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "ATM_WARNING_BASELINE_MEMBER_MISSING"
                && finding["message"]
                    .as_str()
                    .is_some_and(|message| message.contains(TEST_QA))
        }),
        "stdout: {}",
        String::from_utf8(output.stdout.clone()).expect("stdout utf8")
    );
}

#[test]
fn test_doctor_reports_member_roster_with_baseline_ordering() {
    let fixture = Fixture::new(&[TEST_QA, TEST_SENDER, TEST_LEAD, TEST_DAEMON]);
    fixture.write_atm_config(&format!(
        "[atm]\ndefault_team = \"{}\"\nteam_members = [\"{}\", \"{}\", \"{}\"]\n",
        TEST_TEAM, TEST_LEAD, TEST_SENDER, TEST_QA
    ));

    let output = fixture.run(&["doctor", "--json"], &[]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    let members = parsed["member_roster"]["members"]
        .as_array()
        .expect("member roster array");
    assert_eq!(members[0]["name"], TEST_LEAD);
    assert_eq!(members[1]["name"], TEST_SENDER);
    assert_eq!(members[2]["name"], TEST_QA);
    assert_eq!(members[3]["name"], TEST_DAEMON);
}

#[test]
fn test_doctor_reports_missing_team_directory_finding() {
    let fixture = Fixture::empty();

    let output = fixture.run(&["doctor", "--json"], &[]);

    assert!(!output.status.success());
    let parsed = fixture.stdout_json(&output);
    let findings = parsed["findings"].as_array().expect("findings array");
    assert!(
        findings
            .iter()
            .any(|finding| finding["code"] == "ATM_TEAM_NOT_FOUND"),
        "stdout: {}",
        String::from_utf8(output.stdout.clone()).expect("stdout utf8")
    );
}

#[test]
fn test_doctor_reports_team_config_parse_failure_finding() {
    let fixture = Fixture::empty();
    fixture.write_raw_team_config("{\"members\":");

    let output = fixture.run(&["doctor", "--json"], &[]);

    assert!(!output.status.success());
    let parsed = fixture.stdout_json(&output);
    let findings = parsed["findings"].as_array().expect("findings array");
    assert!(
        findings
            .iter()
            .any(|finding| finding["code"] == "ATM_CONFIG_TEAM_PARSE_FAILED"),
        "stdout: {}",
        String::from_utf8(output.stdout.clone()).expect("stdout utf8")
    );
}

#[test]
fn test_doctor_reports_missing_inboxes_directory_finding() {
    let fixture = Fixture::new(&[TEST_LEAD]);
    fs::remove_dir_all(fixture.team_dir().join("inboxes")).expect("remove inboxes dir");

    let output = fixture.run(&["doctor", "--json"], &[]);

    assert!(!output.status.success());
    let parsed = fixture.stdout_json(&output);
    let findings = parsed["findings"].as_array().expect("findings array");
    assert!(
        findings
            .iter()
            .any(|finding| finding["code"] == "ATM_MAILBOX_WRITE_FAILED"),
        "stdout: {}",
        String::from_utf8(output.stdout.clone()).expect("stdout utf8")
    );
}

#[test]
fn test_doctor_reports_stale_restore_marker_warning() {
    let fixture = Fixture::new(&[TEST_SENDER, TEST_LEAD]);
    let backup_path = fixture.tempdir.path().join("backup");
    fs::write(
        fixture.team_dir().join(".restore-in-progress"),
        format!(r#"{{"backup_path":"{}"}}"#, backup_path.display()),
    )
    .expect("restore marker");

    let output = fixture.run(&["doctor", "--json"], &[]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    let findings = parsed["findings"].as_array().expect("findings array");
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "ATM_WARNING_RESTORE_IN_PROGRESS" && finding["severity"] == "warning"
        }),
        "stdout: {}",
        String::from_utf8(output.stdout.clone()).expect("stdout utf8")
    );
}

#[test]
fn test_doctor_reports_stale_mailbox_lock_across_team_inboxes() {
    let fixture = Fixture::new(&[TEST_SENDER, TEST_LEAD]);
    let stale_lock = fixture
        .tempdir
        .path()
        .join(".claude")
        .join("teams")
        .join("ops")
        .join("inboxes")
        .join("worker.json.lock");
    fs::create_dir_all(stale_lock.parent().expect("lock parent")).expect("inboxes dir");
    fs::write(&stale_lock, u32::MAX.to_string()).expect("stale lock");

    let output = fixture.run(&["doctor", "--json"], &[]);

    assert!(
        output.status.success(),
        "stderr: {}",
        fixture.stderr(&output)
    );
    let parsed = fixture.stdout_json(&output);
    let findings = parsed["findings"].as_array().expect("findings array");
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "ATM_WARNING_STALE_MAILBOX_LOCK"
                && finding["message"]
                    .as_str()
                    .is_some_and(|message| message.contains(&stale_lock.display().to_string()))
        }),
        "stdout: {}",
        String::from_utf8(output.stdout.clone()).expect("stdout utf8")
    );
}

struct Fixture {
    tempdir: tempfile::TempDir,
}

impl Fixture {
    fn empty() -> Self {
        Self {
            tempdir: tempfile::tempdir().expect("tempdir"),
        }
    }

    fn new(members: &[&str]) -> Self {
        let fixture = Self::empty();
        fixture.write_team_config(members);
        fixture.warm_daemon();
        fixture
    }

    fn run(&self, args: &[&str], extra_env: &[(&str, &str)]) -> std::process::Output {
        let mut first = Command::new(env!("CARGO_BIN_EXE_atm"));
        support::configure_atm_command(&mut first, self.tempdir.path(), Some(TEST_LEAD))
            .args(args)
            .current_dir(self.tempdir.path());
        for (key, value) in extra_env {
            first.env(key, value);
        }
        let output = first.output().expect("run atm");
        if !support::is_daemon_start_transient(&output) {
            return output;
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
        let mut retry = Command::new(env!("CARGO_BIN_EXE_atm"));
        support::configure_atm_command(&mut retry, self.tempdir.path(), Some(TEST_LEAD))
            .args(args)
            .current_dir(self.tempdir.path());
        for (key, value) in extra_env {
            retry.env(key, value);
        }
        retry
            .output()
            .unwrap_or_else(|error| panic!("retry atm {:?} failed: {error}", args))
    }

    fn warm_daemon(&self) {
        let output = self.run(&["doctor", "--json"], &[]);
        assert!(output.status.success(), "stderr: {}", self.stderr(&output));
    }

    fn write_team_config(&self, members: &[&str]) {
        let team_dir = self.team_dir();
        fs::create_dir_all(&team_dir).expect("team dir");
        fs::create_dir_all(team_dir.join("inboxes")).expect("inboxes dir");
        let config = TeamConfig {
            members: members
                .iter()
                .map(|member| AgentMember::with_name((*member).parse().expect("agent")))
                .collect(),
            ..Default::default()
        };
        fs::write(
            team_dir.join("config.json"),
            serde_json::to_vec(&config).expect("team config"),
        )
        .expect("write team config");
    }

    fn write_atm_config(&self, raw: &str) {
        fs::write(self.tempdir.path().join(".atm.toml"), raw).expect("write .atm.toml");
    }

    fn write_raw_team_config(&self, raw: &str) {
        let team_dir = self.team_dir();
        fs::create_dir_all(&team_dir).expect("team dir");
        fs::write(team_dir.join("config.json"), raw).expect("write raw team config");
    }

    fn stdout_json(&self, output: &std::process::Output) -> Value {
        serde_json::from_slice(&output.stdout).expect("valid doctor json")
    }

    fn stderr(&self, output: &std::process::Output) -> String {
        String::from_utf8(output.stderr.clone()).expect("stderr utf8")
    }

    fn team_dir(&self) -> std::path::PathBuf {
        self.tempdir
            .path()
            .join(".claude")
            .join("teams")
            .join(TEST_TEAM)
    }

    fn active_log_path(&self) -> std::path::PathBuf {
        self.tempdir
            .path()
            .join(".local")
            .join("share")
            .join("logs")
            .join("atm.log.jsonl")
    }
}
