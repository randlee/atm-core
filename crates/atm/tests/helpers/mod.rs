use std::path::Path;
use std::process::Command;

use serde_json::Value;

#[allow(dead_code)]
pub const TEST_TEAM: &str = "test-team";
#[allow(dead_code)]
pub const TEST_SENDER: &str = "sender-a";
#[allow(dead_code)]
pub const TEST_RECIPIENT: &str = "recipient";
#[allow(dead_code)]
pub const TEST_QA: &str = "qa-a";
#[allow(dead_code)]
pub const TEST_LEAD: &str = "team-lead";
#[allow(dead_code)]
pub const TEST_DAEMON: &str = "daemon";
#[allow(dead_code)]
pub const TEST_ORIGIN: &str = "host-a";
#[allow(dead_code)]
pub const TEST_SENDER_ADDRESS: &str = "sender-a@test-team";
#[allow(dead_code)]
pub const TEST_RECIPIENT_ADDRESS: &str = "recipient@test-team";
#[allow(dead_code)]
pub const TEST_LEAD_ADDRESS: &str = "team-lead@test-team";

#[allow(dead_code)]
pub fn qualified(agent: &str) -> String {
    format!("{agent}@{TEST_TEAM}")
}

/// Configure one ATM CLI subprocess for hermetic test execution.
///
/// Tests must not consume ambient ATM_* settings from the host shell. This
/// helper clears the subprocess environment, restores only the minimum
/// platform/runtime variables needed to launch child tools, and then injects
/// a fully test-local ATM home/config/identity/team.
#[allow(dead_code)]
pub fn configure_atm_command<'a>(
    command: &'a mut Command,
    home_dir: &Path,
    identity: Option<&str>,
) -> &'a mut Command {
    command.env_clear();
    for key in [
        "PATH",
        "HOME",
        "USERPROFILE",
        "SYSTEMROOT",
        "TMPDIR",
        "TMP",
        "TEMP",
        "ComSpec",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .env("ATM_HOME", home_dir)
        .env("ATM_CONFIG_HOME", home_dir)
        .env("ATM_TEAM", TEST_TEAM);
    if let Some(identity) = identity {
        command.env("ATM_IDENTITY", identity);
    }
    command
}

#[allow(dead_code)]
pub fn parse_inbox_values(raw: &str) -> Vec<Value> {
    if raw.trim().is_empty() {
        return Vec::new();
    }

    match raw.chars().find(|ch| !ch.is_whitespace()) {
        Some('[') => serde_json::from_str(raw).expect("json array"),
        _ => raw
            .lines()
            .map(|line| serde_json::from_str(line).expect("json line"))
            .collect(),
    }
}
