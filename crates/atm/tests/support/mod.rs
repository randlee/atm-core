#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;
#[cfg(test)]
use std::process::Output;
#[cfg(test)]
use std::time::{Duration as StdDuration, Instant};

#[allow(unused_imports)]
pub use atm_core::roles::ROLE_TEAM_LEAD;
#[allow(unused_imports)]
pub use atm_core::test_support::{
    TEST_DAEMON, TEST_LEAD, TEST_LEAD_ADDRESS, TEST_ORIGIN, TEST_QA, TEST_QA_AGENT, TEST_RECIPIENT,
    TEST_RECIPIENT_ADDRESS, TEST_SENDER, TEST_SENDER_ADDRESS, TEST_TEAM,
};
#[cfg(test)]
use atm_core::schema::{AgentMember, LegacyMessageId, MessageEnvelope, TeamConfig, hydrate_legacy_fields_from_metadata};
#[cfg(test)]
use atm_core::types::{AgentName, TeamName};
#[cfg(test)]
use chrono::{DateTime, Utc};
use serde_json::json;
#[cfg(test)]
use serde_json::Value;
use tempfile::TempDir;

#[derive(Debug)]
pub struct TestEnv {
    pub tempdir: TempDir,
    pub env_map: BTreeMap<String, String>,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone)]
pub struct TestEnvBuilder {
    team: String,
    members: Vec<String>,
    cwd_name: String,
}

impl TestEnvBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn team(mut self, team: impl Into<String>) -> Self {
        self.team = team.into();
        self
    }

    pub fn members<I, S>(mut self, members: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.members = members.into_iter().map(Into::into).collect();
        self
    }

    pub fn cwd_name(mut self, cwd_name: impl Into<String>) -> Self {
        self.cwd_name = cwd_name.into();
        self
    }

    pub fn build(self) -> io::Result<TestEnv> {
        let tempdir = tempfile::tempdir()?;
        let atm_home = tempdir.path().join("atm-home");
        let atm_config_home = tempdir.path().join("config-home");
        let atm_teams_dir = atm_config_home.join(".claude").join("teams");
        let team_dir = atm_teams_dir.join(&self.team);
        let inboxes_dir = team_dir.join("inboxes");
        let workflow_dir = team_dir.join(".atm-state").join("workflow");
        let db_dir = atm_home.join("db");
        let cwd = tempdir.path().join(&self.cwd_name);

        fs::create_dir_all(&atm_home)?;
        fs::create_dir_all(&inboxes_dir)?;
        fs::create_dir_all(&workflow_dir)?;
        fs::create_dir_all(&db_dir)?;
        fs::create_dir_all(&cwd)?;

        let config_path = team_dir.join("config.json");
        let members = self
            .members
            .into_iter()
            .map(|name| json!({ "name": name }))
            .collect::<Vec<_>>();
        fs::write(
            config_path,
            serde_json::to_vec_pretty(&json!({ "members": members }))?,
        )?;

        let env_map = BTreeMap::from([
            (
                "ATM_HOME".to_string(),
                atm_home.to_string_lossy().into_owned(),
            ),
            (
                "ATM_CONFIG_HOME".to_string(),
                atm_config_home.to_string_lossy().into_owned(),
            ),
            (
                "ATM_TEAMS_DIR".to_string(),
                atm_teams_dir.to_string_lossy().into_owned(),
            ),
        ]);

        Ok(TestEnv {
            tempdir,
            env_map,
            cwd,
        })
    }
}

/// Default fixtures use `TEST_LEAD` instead of the reserved `ROLE_TEAM_LEAD`
/// string so generic tests do not silently depend on production role naming.
/// Tests that must exercise `team-lead` semantics should opt in explicitly by
/// using `ROLE_TEAM_LEAD`.
impl Default for TestEnvBuilder {
    fn default() -> Self {
        Self {
            team: TEST_TEAM.to_string(),
            members: vec![
                TEST_SENDER.to_string(),
                TEST_RECIPIENT.to_string(),
                TEST_LEAD.to_string(),
            ],
            cwd_name: "cwd".to_string(),
        }
    }
}

pub fn qualified(agent: &str) -> String {
    format!("{agent}@{TEST_TEAM}")
}

pub fn configure_atm_command<'a>(
    command: &'a mut Command,
    home_dir: &std::path::Path,
    identity: Option<&str>,
) -> &'a mut Command {
    let daemon_bin = ensure_test_daemon_launcher(home_dir);
    command.env_clear();
    for key in [
        "PATH",
        "CARGO",
        "CARGO_HOME",
        "HOME",
        "RUSTUP_HOME",
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
        .env("ATM_TEAMS_DIR", home_dir.join(".claude").join("teams"))
        .env("ATM_TEAM", TEST_TEAM)
        .env("ATM_DAEMON_BIN", &daemon_bin);
    if let Some(identity) = identity {
        command.env("ATM_IDENTITY", identity);
    }
    command
}

#[cfg(test)]
pub fn is_daemon_start_transient(output: &Output) -> bool {
    let stderr = String::from_utf8_lossy(&output.stderr);
    stderr.contains("failed to read daemon request frame")
        || stderr.contains("failed to read daemon response frame")
        || stderr.contains("daemon socket was not published")
        || stderr.contains("failed to connect to daemon socket")
        || stderr.contains("failed to connect to daemon socket after auto-start")
        || stderr.contains("failed to write daemon request frame")
        || stderr.contains("failed to finalize daemon request frame")
}

fn ensure_test_daemon_launcher(home_dir: &std::path::Path) -> PathBuf {
    #[allow(unused_variables)]
    let hermetic_daemon = option_env!("CARGO_BIN_EXE_atm-daemon").map(PathBuf::from);
    if let Some(path) = hermetic_daemon.as_ref().filter(|path| path.exists()) {
        return path.clone();
    }

    let sibling = PathBuf::from(env!("CARGO_BIN_EXE_atm"))
        .with_file_name(format!("atm-daemon{}", std::env::consts::EXE_SUFFIX));
    if sibling.exists() {
        return sibling;
    }

    let workspace_binary = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("target")
        .join("debug")
        .join(format!("atm-daemon{}", std::env::consts::EXE_SUFFIX));
    if workspace_binary.exists() {
        return workspace_binary;
    }

    let _ = home_dir;
    panic!(
        "expected hermetic test daemon binary at one of: {:?}, {}, {}",
        hermetic_daemon,
        sibling.display(),
        workspace_binary.display()
    );
}

#[cfg(test)]
fn parse_inbox_values(raw: &str) -> Vec<Value> {
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

#[cfg(test)]
#[derive(Debug)]
pub struct CliFixture {
    pub tempdir: TempDir,
}

#[cfg(test)]
impl CliFixture {
    pub fn new_with_recipient(recipient: &str) -> Self {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let fixture = Self { tempdir };
        fixture.write_team_config(recipient);
        fixture.warm_daemon();
        fixture
    }

    pub fn new_with_members(members: &[&str]) -> Self {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let fixture = Self { tempdir };
        fixture.write_team_config_members(members);
        fixture.warm_daemon();
        fixture
    }

    pub fn run(&self, args: &[&str]) -> std::process::Output {
        self.run_with_env(args, &[])
    }

    pub fn warm_daemon(&self) {
        let deadline = Instant::now() + StdDuration::from_secs(2);
        loop {
            let output = self.run(&["read", "--all", "--no-mark", "--json"]);
            if output.status.success() {
                return;
            }
            assert!(
                is_daemon_start_transient(&output),
                "stderr: {}",
                self.stderr(&output)
            );
            if Instant::now() >= deadline {
                panic!("daemon warmup exhausted retries: {}", self.stderr(&output));
            }
            std::thread::sleep(StdDuration::from_millis(50));
        }
    }

    pub fn run_without_identity(&self, args: &[&str]) -> std::process::Output {
        let deadline = Instant::now() + StdDuration::from_secs(2);
        let mut attempt = 0usize;
        loop {
            attempt += 1;
            let mut command = Command::new(env!("CARGO_BIN_EXE_atm"));
            let output = configure_atm_command(&mut command, self.tempdir.path(), None)
                .args(args)
                .current_dir(self.tempdir.path())
                .output()
                .unwrap_or_else(|error| {
                    panic!(
                        "atm without identity {:?} failed on attempt {attempt}: {error}",
                        args
                    )
                });
            if output.status.success()
                || !is_daemon_start_transient(&output)
                || Instant::now() >= deadline
            {
                return output;
            }
            std::thread::sleep(StdDuration::from_millis(50));
        }
    }

    pub fn run_with_env(
        &self,
        args: &[&str],
        extra_env: &[(&str, &str)],
    ) -> std::process::Output {
        let deadline = Instant::now() + StdDuration::from_secs(2);
        let mut attempt = 0usize;
        loop {
            attempt += 1;
            let mut command = Command::new(env!("CARGO_BIN_EXE_atm"));
            configure_atm_command(&mut command, self.tempdir.path(), Some(TEST_SENDER))
                .args(args)
                .current_dir(self.tempdir.path());
            for (key, value) in extra_env {
                command.env(key, value);
            }
            let output = command.output().unwrap_or_else(|error| {
                panic!("atm {:?} failed on attempt {attempt}: {error}", args)
            });
            if output.status.success()
                || !is_daemon_start_transient(&output)
                || Instant::now() >= deadline
            {
                return output;
            }
            std::thread::sleep(StdDuration::from_millis(50));
        }
    }

    pub fn write_team_config(&self, recipient: &str) {
        self.write_team_config_for_team(TEST_TEAM, recipient);
    }

    pub fn write_team_config_for_team(&self, team: &str, recipient: &str) {
        let team_dir = self.team_dir_for(team);
        fs::create_dir_all(&team_dir).expect("team dir");
        let config = TeamConfig {
            members: vec![AgentMember::with_name(recipient.parse().expect("agent"))],
            ..Default::default()
        };
        fs::write(
            team_dir.join("config.json"),
            serde_json::to_vec(&config).expect("team config"),
        )
        .expect("write team config");
    }

    pub fn write_team_config_members(&self, members: &[&str]) {
        let team_dir = self.team_dir();
        fs::create_dir_all(&team_dir).expect("team dir");
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

    pub fn write_raw_team_config(&self, raw: &str) {
        let team_dir = self.team_dir();
        fs::create_dir_all(&team_dir).expect("team dir");
        fs::write(team_dir.join("config.json"), raw).expect("write raw team config");
    }

    pub fn write_atm_config(&self, raw: &str) {
        fs::write(self.tempdir.path().join(".atm.toml"), raw).expect("write .atm.toml");
    }

    pub fn inbox_path(&self, recipient: &str) -> std::path::PathBuf {
        self.inbox_path_in_team(TEST_TEAM, recipient)
    }

    pub fn inbox_path_in_team(&self, team: &str, recipient: &str) -> std::path::PathBuf {
        self.team_dir_for(team)
            .join("inboxes")
            .join(format!("{recipient}.json"))
    }

    pub fn write_inbox(&self, recipient: &str, messages: &[MessageEnvelope]) {
        let inbox_path = self.inbox_path(recipient);
        if let Some(parent) = inbox_path.parent() {
            fs::create_dir_all(parent).expect("inbox dir");
        }
        let values: Vec<Value> = messages
            .iter()
            .map(|message| serde_json::to_value(message).expect("json value"))
            .collect();
        fs::write(
            inbox_path,
            serde_json::to_string_pretty(&values).expect("json array"),
        )
        .expect("write inbox");
    }

    pub fn inbox_contents(&self, recipient: &str) -> Vec<MessageEnvelope> {
        self.inbox_contents_in_team(TEST_TEAM, recipient)
    }

    pub fn inbox_json_lines(&self, recipient: &str) -> Vec<Value> {
        self.inbox_json_lines_in_team(TEST_TEAM, recipient)
    }

    pub fn inbox_contents_in_team(&self, team: &str, recipient: &str) -> Vec<MessageEnvelope> {
        let inbox_path = self.inbox_path_in_team(team, recipient);
        let raw = fs::read_to_string(&inbox_path).expect("inbox contents");
        parse_inbox_values(&raw)
            .into_iter()
            .map(|mut value| {
                hydrate_legacy_fields_from_metadata(&mut value);
                serde_json::from_value(value).expect("message envelope")
            })
            .collect()
    }

    pub fn inbox_json_lines_in_team(&self, team: &str, recipient: &str) -> Vec<Value> {
        let inbox_path = self.inbox_path_in_team(team, recipient);
        let raw = fs::read_to_string(&inbox_path).expect("inbox contents");
        parse_inbox_values(&raw)
    }

    pub fn write_workflow_state(&self, agent: &str, value: Value) {
        let path = self
            .team_dir()
            .join(".atm-state")
            .join("workflow")
            .join(format!("{agent}.json"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("workflow dir");
        }
        fs::write(path, serde_json::to_vec(&value).expect("workflow json"))
            .expect("write workflow");
    }

    pub fn workflow_state_contents(&self, agent: &str) -> Value {
        self.workflow_state_contents_in_team(TEST_TEAM, agent)
    }

    pub fn workflow_state_contents_in_team(&self, team: &str, agent: &str) -> Value {
        let raw = fs::read_to_string(
            self.team_dir_for(team)
                .join(".atm-state")
                .join("workflow")
                .join(format!("{agent}.json")),
        )
        .expect("workflow state contents");
        serde_json::from_str(&raw).expect("workflow json")
    }

    pub fn write_origin_inbox(&self, agent: &str, origin: &str, messages: &[MessageEnvelope]) {
        let inbox_path = self.origin_inbox_path(agent, origin);
        if let Some(parent) = inbox_path.parent() {
            fs::create_dir_all(parent).expect("origin inbox dir");
        }
        let values: Vec<Value> = messages
            .iter()
            .map(|message| serde_json::to_value(message).expect("json value"))
            .collect();
        fs::write(
            inbox_path,
            serde_json::to_string_pretty(&values).expect("json array"),
        )
        .expect("write origin inbox");
    }

    pub fn origin_inbox_path(&self, agent: &str, origin: &str) -> std::path::PathBuf {
        self.team_dir()
            .join("inboxes")
            .join(format!("{agent}.{origin}.json"))
    }

    pub fn origin_inbox_contents(&self, agent: &str, origin: &str) -> Vec<MessageEnvelope> {
        let raw = fs::read_to_string(self.origin_inbox_path(agent, origin))
            .expect("origin inbox contents");
        parse_inbox_values(&raw)
            .into_iter()
            .map(|value| serde_json::from_value(value).expect("message envelope"))
            .collect()
    }

    pub fn team_dir(&self) -> std::path::PathBuf {
        self.team_dir_for(TEST_TEAM)
    }

    pub fn team_dir_for(&self, team: &str) -> std::path::PathBuf {
        self.tempdir.path().join(".claude").join("teams").join(team)
    }

    pub fn install_hook_fixture(&self, mode: &str) -> (PathBuf, PathBuf) {
        let fixture_binary = PathBuf::from(env!("CARGO_BIN_EXE_atm_post_send_hook_fixture"));
        let hook_dir = self.tempdir.path().join("bin");
        fs::create_dir_all(&hook_dir).expect("hook dir");
        let hook_path = hook_dir.join(fixture_binary.file_name().expect("hook binary filename"));
        fs::copy(&fixture_binary, &hook_path).expect("copy hook fixture");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = fs::metadata(&hook_path)
                .expect("hook metadata")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&hook_path, permissions).expect("hook permissions");
        }
        let payload_path = self.tempdir.path().join(format!("{mode}-payload.json"));
        (
            PathBuf::from("bin").join(hook_path.file_name().expect("copied hook binary filename")),
            payload_path,
        )
    }

    pub fn install_executable_script(&self, relative_path: &str, body: &str) -> PathBuf {
        let path = self.tempdir.path().join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("script dir");
        }
        fs::write(&path, body).expect("write script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = fs::metadata(&path).expect("script metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&path, permissions).expect("script permissions");
        }
        path
    }

    pub fn stdout(&self, output: &std::process::Output) -> String {
        String::from_utf8(output.stdout.clone()).expect("stdout utf8")
    }

    pub fn stdout_json(&self, output: &std::process::Output) -> serde_json::Value {
        serde_json::from_slice(&output.stdout).expect("valid json")
    }

    pub fn stderr(&self, output: &std::process::Output) -> String {
        String::from_utf8(output.stderr.clone()).expect("stderr utf8")
    }

    pub fn message(
        &self,
        from: &str,
        text: &str,
        read: bool,
        pending_ack_at: Option<DateTime<Utc>>,
        acknowledged_at: Option<DateTime<Utc>>,
        timestamp: DateTime<Utc>,
    ) -> MessageEnvelope {
        MessageEnvelope {
            from: from.parse::<AgentName>().expect("agent"),
            text: text.to_string(),
            timestamp: timestamp.into(),
            read,
            source_team: Some(TEST_TEAM.parse::<TeamName>().expect("team")),
            summary: None,
            message_id: Some(LegacyMessageId::new()),
            pending_ack_at: pending_ack_at.map(Into::into),
            acknowledged_at: acknowledged_at.map(Into::into),
            acknowledges_message_id: None,
            task_id: None,
            extra: serde_json::Map::new(),
        }
    }
}
