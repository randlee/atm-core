use std::fs;
use std::process::Command;

use atm_core::schema::{AgentMember, TeamConfig};
use serde_json::Value;

#[test]
fn test_doctor_reports_healthy_observability() {
    let fixture = Fixture::new(&["arch-ctm"]);

    let output = fixture.run(
        &["doctor", "--json"],
        &[
            ("ATM_TEST_OBSERVABILITY_HEALTH", "healthy"),
            ("ATM_TEST_OBSERVABILITY_QUERY_STATE", "healthy"),
        ],
    );

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
    assert!(
        parsed["observability"]["active_log_path"]
            .as_str()
            .is_some()
    );
}

#[test]
fn test_doctor_reports_degraded_observability() {
    let fixture = Fixture::new(&["arch-ctm"]);

    let output = fixture.run(
        &["doctor", "--json"],
        &[
            ("ATM_TEST_OBSERVABILITY_HEALTH", "degraded"),
            ("ATM_TEST_OBSERVABILITY_QUERY_STATE", "degraded"),
            ("ATM_TEST_OBSERVABILITY_DETAIL", "query backlog"),
        ],
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
    assert!(
        parsed["findings"][0]["message"]
            .as_str()
            .expect("message")
            .contains("query backlog")
    );
}

#[test]
fn test_doctor_reports_unavailable_observability_as_error() {
    let fixture = Fixture::new(&["arch-ctm"]);

    let output = fixture.run(
        &["doctor", "--json"],
        &[
            ("ATM_TEST_OBSERVABILITY_HEALTH", "unavailable"),
            ("ATM_TEST_OBSERVABILITY_QUERY_STATE", "unavailable"),
            ("ATM_TEST_OBSERVABILITY_DETAIL", "logger unavailable"),
        ],
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
    assert_eq!(parsed["observability"]["query_state"], "unavailable");
}

struct Fixture {
    tempdir: tempfile::TempDir,
}

impl Fixture {
    fn new(members: &[&str]) -> Self {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let fixture = Self { tempdir };
        fixture.write_team_config(members);
        fixture
    }

    fn run(&self, args: &[&str], extra_env: &[(&str, &str)]) -> std::process::Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_atm"));
        command
            .args(args)
            .env("ATM_HOME", self.tempdir.path())
            .env("ATM_IDENTITY", "arch-ctm")
            .env("ATM_TEAM", "atm-dev")
            .current_dir(self.tempdir.path());
        for (key, value) in extra_env {
            command.env(key, value);
        }
        command.output().expect("run atm")
    }

    fn write_team_config(&self, members: &[&str]) {
        let team_dir = self.team_dir();
        fs::create_dir_all(&team_dir).expect("team dir");
        let config = TeamConfig {
            members: members
                .iter()
                .map(|member| AgentMember {
                    name: (*member).to_string(),
                    ..Default::default()
                })
                .collect(),
        };
        fs::write(
            team_dir.join("config.json"),
            serde_json::to_vec(&config).expect("team config"),
        )
        .expect("write team config");
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
            .join("atm-dev")
    }
}
