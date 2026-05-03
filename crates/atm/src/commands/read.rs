use anyhow::{Context, Result};
use atm_core::home;
use atm_core::read::{ReadOutcome, ReadQuery};
use atm_core::types::{AckActivationMode, AgentName, IsoTimestamp, ReadSelection};
use clap::Args;

use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
/// Read one ATM mailbox surface and optionally update read state.
pub struct ReadCommand {
    target: Option<String>,

    #[arg(long)]
    team: Option<String>,

    #[arg(long, conflicts_with_all = ["unread_only", "pending_ack_only", "history"])]
    all: bool,

    #[arg(long, conflicts_with_all = ["pending_ack_only", "history", "all"])]
    unread_only: bool,

    #[arg(long, conflicts_with_all = ["unread_only", "history", "all"])]
    pending_ack_only: bool,

    #[arg(long, conflicts_with_all = ["unread_only", "pending_ack_only", "all"])]
    history: bool,

    #[arg(long)]
    since_last_seen: bool,

    #[arg(long = "no-since-last-seen", default_value_t = false)]
    no_since_last_seen: bool,

    #[arg(long)]
    no_mark: bool,

    #[arg(long)]
    no_update_seen: bool,

    #[arg(long)]
    limit: Option<usize>,

    #[arg(long)]
    since: Option<String>,

    #[arg(long)]
    from: Option<String>,

    #[arg(long)]
    json: bool,

    #[arg(long)]
    timeout: Option<u64>,

    #[arg(long = "as")]
    actor: Option<String>,
}

impl ReadCommand {
    /// Execute the `atm read` command.
    pub fn run(self, observability: &CliObservability) -> Result<()> {
        let current_dir = std::env::current_dir()?;
        let home_dir = home::atm_home()?;
        let json = self.json;
        let outcome = self.execute_with_requester(home_dir, current_dir, |query| {
            atm_daemon::request_read_with_autostart(query)
        })?;
        output::print_read_result(&outcome, json)?;
        let _ = observability;
        Ok(())
    }

    fn execute_with_requester<F>(
        self,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
        requester: F,
    ) -> Result<ReadOutcome>
    where
        F: FnOnce(ReadQuery) -> Result<ReadOutcome, atm_core::error::AtmError>,
    {
        let query = self.build_query(home_dir, current_dir)?;
        requester(query).map_err(Into::into)
    }

    fn build_query(
        self,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    ) -> Result<ReadQuery> {
        // --since-last-seen is the default; explicitly setting it has the same effect.
        let _ = self.since_last_seen;
        let selection_mode = self.selection_mode();
        let timestamp_filter = self.since.as_deref().map(parse_timestamp).transpose()?;
        let sender_filter = self
            .from
            .as_deref()
            .map(str::parse::<AgentName>)
            .transpose()?;
        ReadQuery::new(
            home_dir,
            current_dir,
            self.actor.as_deref(),
            self.target.as_deref(),
            self.team.as_deref(),
            selection_mode,
            !self.no_since_last_seen,
            !self.no_update_seen,
            if self.no_mark {
                AckActivationMode::ReadOnly
            } else {
                AckActivationMode::PromoteDisplayedUnread
            },
            self.limit,
            sender_filter,
            timestamp_filter,
            self.timeout,
        )
        .map_err(Into::into)
    }

    fn selection_mode(&self) -> ReadSelection {
        if self.all {
            ReadSelection::All
        } else if self.unread_only {
            ReadSelection::UnreadOnly
        } else if self.pending_ack_only {
            ReadSelection::PendingAckOnly
        } else if self.history {
            ReadSelection::ActionableWithHistory
        } else {
            ReadSelection::Actionable
        }
    }
}

fn parse_timestamp(value: &str) -> Result<IsoTimestamp> {
    chrono::DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("invalid ISO 8601 timestamp: {value}"))
        .map(|timestamp| timestamp.with_timezone(&chrono::Utc).into())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use super::ReadCommand;
    use atm_core::dispatcher::{DaemonRequest, RequestKind, RequestPayload};
    use atm_core::schema::{AgentMember, MessageEnvelope, TeamConfig};
    use atm_core::types::{AgentName, TeamName};
    use atm_core::write_messages;
    use atm_daemon::{CoreDispatcher, TestSocketClient};
    use tempfile::TempDir;

    const TEST_TEAM: &str = "test-team";
    const TEST_SENDER: &str = "sender-a";
    const ROLE_TEAM_LEAD: &str = "team-lead";

    #[test]
    fn build_query_rejects_invalid_target_before_core() {
        let command = ReadCommand {
            target: Some("../evil".to_string()),
            team: None,
            all: false,
            unread_only: false,
            pending_ack_only: false,
            history: false,
            since_last_seen: false,
            no_since_last_seen: false,
            no_mark: false,
            no_update_seen: false,
            limit: None,
            since: None,
            from: None,
            json: false,
            timeout: None,
            actor: None,
        };

        let error = command
            .build_query(".".into(), ".".into())
            .expect_err("invalid target");

        assert!(error.to_string().contains("agent name"));
    }

    #[test]
    fn execute_with_requester_supports_in_process_dispatch() {
        let fixture = Fixture::new();
        let command = ReadCommand {
            target: None,
            team: Some(TEST_TEAM.to_string()),
            all: false,
            unread_only: false,
            pending_ack_only: false,
            history: false,
            since_last_seen: false,
            no_since_last_seen: false,
            no_mark: true,
            no_update_seen: false,
            limit: None,
            since: None,
            from: None,
            json: true,
            timeout: None,
            actor: Some(TEST_SENDER.to_string()),
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
                    payload: RequestPayload::Read(
                        serde_json::to_value(query).expect("read query json"),
                    ),
                })?;
                assert_eq!(response.kind, RequestKind::Read);
                serde_json::from_str(&response.payload_json).map_err(|error| {
                    atm_core::error::AtmError::daemon_protocol(
                        "failed to decode test read response",
                    )
                    .with_source(error)
                })
            })
            .expect("read outcome");

        assert_eq!(outcome.team.as_str(), TEST_TEAM);
        assert_eq!(outcome.agent.as_str(), TEST_SENDER);
        assert_eq!(outcome.count, 1);
        assert_eq!(outcome.messages[0].envelope.text, "hello");
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
                members: vec![AgentMember::with_name(
                    TEST_SENDER.parse::<AgentName>().expect("agent"),
                )],
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
                    text: "hello".to_string(),
                    timestamp: chrono::Utc::now().into(),
                    read: false,
                    source_team: Some(TEST_TEAM.parse::<TeamName>().expect("team")),
                    summary: Some("hello".to_string()),
                    message_id: Some(atm_core::schema::LegacyMessageId::new()),
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
