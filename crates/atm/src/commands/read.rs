use anyhow::{Context, Result};
use atm_core::home;
use atm_core::read::{self, ReadQuery};
use atm_core::types::{AckActivationMode, IsoTimestamp, ReadSelection};
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
        let query = self.build_query(home_dir, current_dir)?;
        let outcome = read::read_mail(query, observability)?;
        output::print_read_result(&outcome, json)
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
            self.from,
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
    use super::ReadCommand;

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
}
