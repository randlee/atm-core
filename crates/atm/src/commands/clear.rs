use std::time::Duration;

use anyhow::{Context, Result};
use atm_core::address::AgentAddress;
use atm_core::clear::ClearQuery;
use atm_core::home;
use clap::Args;

use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
/// Clear read or acknowledged messages from a mailbox.
pub struct ClearCommand {
    target: Option<String>,

    #[arg(long = "as")]
    actor_override: Option<String>,

    #[arg(long)]
    team: Option<String>,

    #[arg(long = "older-than", value_name = "DURATION")]
    older_than: Option<String>,

    #[arg(long)]
    idle_only: bool,

    #[arg(long)]
    dry_run: bool,

    #[arg(long)]
    json: bool,
}

impl ClearCommand {
    /// Execute the `atm clear` command.
    pub fn run(self, observability: &CliObservability) -> Result<()> {
        let current_dir = std::env::current_dir()?;
        let home_dir = home::atm_home()?;
        let dry_run = self.dry_run;
        let json = self.json;
        let outcome = self.execute_with_requester(home_dir, current_dir, |query| {
            atm_daemon::request_clear_with_autostart(query)
        })?;
        output::print_clear_result(&outcome, dry_run, json)?;
        let _ = observability;
        Ok(())
    }

    fn execute_with_requester<F>(
        self,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
        requester: F,
    ) -> Result<atm_core::clear::ClearOutcome>
    where
        F: FnOnce(ClearQuery) -> Result<atm_core::clear::ClearOutcome, atm_core::error::AtmError>,
    {
        let query = self.build_query(home_dir, current_dir)?;
        requester(query).map_err(Into::into)
    }

    fn build_query(
        self,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    ) -> Result<ClearQuery> {
        let older_than = self.older_than.as_deref().map(parse_duration).transpose()?;
        let target_address = self
            .target
            .as_deref()
            .map(str::parse::<AgentAddress>)
            .transpose()?;

        Ok(ClearQuery {
            home_dir,
            current_dir,
            actor_override: self.actor_override.map(|value| value.parse()).transpose()?,
            target_address,
            team_override: self.team.map(|value| value.parse()).transpose()?,
            older_than,
            idle_only: self.idle_only,
            dry_run: self.dry_run,
        })
    }
}

fn parse_duration(raw: &str) -> Result<Duration> {
    let value = raw.trim();
    let Some((unit_index, unit_char)) = value.char_indices().last() else {
        anyhow::bail!("invalid duration: {value}");
    };
    let amount = &value[..unit_index];
    if amount.is_empty() {
        anyhow::bail!("invalid duration: {value}");
    }

    let amount = amount
        .parse::<u64>()
        .with_context(|| format!("invalid duration: {value}"))?;

    let secs = match unit_char {
        's' => amount,
        'm' => amount
            .checked_mul(60)
            .ok_or_else(|| anyhow::anyhow!("duration overflow: {value}"))?,
        'h' => amount
            .checked_mul(60)
            .and_then(|value| value.checked_mul(60))
            .ok_or_else(|| anyhow::anyhow!("duration overflow: {value}"))?,
        'd' => amount
            .checked_mul(60)
            .and_then(|value| value.checked_mul(60))
            .and_then(|value| value.checked_mul(24))
            .ok_or_else(|| anyhow::anyhow!("duration overflow: {value}"))?,
        _ => anyhow::bail!("invalid duration unit in {value}; use s, m, h, or d"),
    };

    Ok(Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::ClearCommand;
    use std::fs;
    use std::sync::Arc;

    use atm_core::dispatcher::{DaemonRequest, RequestKind, RequestPayload};
    use atm_core::schema::{AgentMember, LegacyMessageId, MessageEnvelope, TeamConfig};
    use atm_core::write_messages;
    use atm_daemon::{CoreDispatcher, TestSocketClient};
    use tempfile::TempDir;

    const TEST_TEAM: &str = "test-team";
    const TEST_SENDER: &str = "sender-a";
    const ROLE_TEAM_LEAD: &str = "team-lead";

    #[test]
    fn build_query_rejects_invalid_target_before_core() {
        let command = ClearCommand {
            target: Some("../evil".to_string()),
            actor_override: None,
            team: None,
            older_than: None,
            idle_only: false,
            dry_run: false,
            json: false,
        };

        let error = command
            .build_query(".".into(), ".".into())
            .expect_err("invalid target");

        assert!(error.to_string().contains("agent name"));
    }

    #[test]
    fn execute_with_requester_supports_in_process_dispatch() {
        let fixture = Fixture::new();
        let command = ClearCommand {
            target: None,
            actor_override: Some(TEST_SENDER.to_string()),
            team: Some(TEST_TEAM.to_string()),
            older_than: None,
            idle_only: false,
            dry_run: false,
            json: true,
        };

        let outcome = command
            .execute_with_requester(fixture.home_dir(), fixture.current_dir(), |query| {
                let dispatcher = CoreDispatcher::new(
                    fixture.home_dir(),
                    Arc::new(atm_core::observability::NullObservability),
                );
                let client = TestSocketClient::new(&dispatcher);
                let response = client.request(DaemonRequest {
                    team_name: TEST_TEAM.parse().expect("team"),
                    agent_name: TEST_SENDER.parse().expect("agent"),
                    payload: RequestPayload::Clear(
                        serde_json::to_value(query).expect("clear query json"),
                    ),
                })?;
                assert_eq!(response.kind, RequestKind::Clear);
                serde_json::from_str(&response.payload_json).map_err(|error| {
                    atm_core::error::AtmError::daemon_protocol(
                        "failed to decode test clear response",
                    )
                    .with_source(error)
                })
            })
            .expect("clear outcome");

        assert_eq!(outcome.team.as_str(), TEST_TEAM);
        assert_eq!(outcome.agent.as_str(), TEST_SENDER);
        assert_eq!(outcome.removed_total, 1);
    }

    struct Fixture {
        tempdir: TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let tempdir = TempDir::new().expect("tempdir");
            let fixture = Self { tempdir };
            fixture.write_team_config();
            fixture.write_inbox();
            fixture
        }

        fn home_dir(&self) -> std::path::PathBuf {
            self.tempdir.path().to_path_buf()
        }

        fn current_dir(&self) -> std::path::PathBuf {
            self.tempdir.path().to_path_buf()
        }

        fn team_dir(&self) -> std::path::PathBuf {
            self.tempdir
                .path()
                .join(".claude")
                .join("teams")
                .join(TEST_TEAM)
        }

        fn write_team_config(&self) {
            fs::create_dir_all(self.team_dir()).expect("team dir");
            let config = TeamConfig {
                members: vec![AgentMember::with_name(TEST_SENDER.parse().expect("agent"))],
                ..Default::default()
            };
            fs::write(
                self.team_dir().join("config.json"),
                serde_json::to_vec(&config).expect("config json"),
            )
            .expect("write config");
            fs::write(
                self.tempdir.path().join(".atm.toml"),
                format!("[atm]\ndefault_team = \"{TEST_TEAM}\"\n"),
            )
            .expect("write .atm.toml");
        }

        fn write_inbox(&self) {
            let inbox_path = self
                .team_dir()
                .join("inboxes")
                .join(format!("{TEST_SENDER}.json"));
            if let Some(parent) = inbox_path.parent() {
                fs::create_dir_all(parent).expect("inbox dir");
            }
            write_messages(
                &inbox_path,
                &[MessageEnvelope {
                    from: ROLE_TEAM_LEAD.parse().expect("lead"),
                    text: "already read".to_string(),
                    timestamp: chrono::Utc::now().into(),
                    read: true,
                    source_team: Some(TEST_TEAM.parse().expect("team")),
                    summary: Some("already read".to_string()),
                    message_id: Some(LegacyMessageId::new()),
                    pending_ack_at: None,
                    acknowledged_at: None,
                    acknowledges_message_id: None,
                    task_id: None,
                    extra: Default::default(),
                }],
            )
            .expect("write inbox");
        }
    }
}
