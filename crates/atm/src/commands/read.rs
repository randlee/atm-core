use anyhow::{Context, Result};
use atm_core::home;
use atm_core::read::ReadQuery;
use atm_core::types::{AckActivationMode, IsoTimestamp, ReadSelection};
use clap::Args;

use crate::composition::CliComposition;
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
        let composition = CliComposition::bootstrap(observability)?;
        let query = self.build_query(home_dir, current_dir)?;
        let outcome = composition.receive(query)?;
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
            self.from.as_deref(),
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
    use atm_core::types::{AckActivationMode, ReadSelection};
    use tempfile::TempDir;

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

    #[test]
    fn selection_mode_maps_cli_flags_to_expected_bucket() {
        let all = ReadCommand {
            target: None,
            team: None,
            all: true,
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
        let unread = ReadCommand {
            target: None,
            team: None,
            all: false,
            unread_only: true,
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
        let pending = ReadCommand {
            target: None,
            team: None,
            all: false,
            unread_only: false,
            pending_ack_only: true,
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
        let history = ReadCommand {
            target: None,
            team: None,
            all: false,
            unread_only: false,
            pending_ack_only: false,
            history: true,
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

        assert_eq!(all.selection_mode(), ReadSelection::All);
        assert_eq!(unread.selection_mode(), ReadSelection::UnreadOnly);
        assert_eq!(pending.selection_mode(), ReadSelection::PendingAckOnly);
        assert_eq!(
            history.selection_mode(),
            ReadSelection::ActionableWithHistory
        );
    }

    #[test]
    fn build_query_propagates_read_flags_filters_and_timeout() {
        let command = ReadCommand {
            target: Some("recipient-a@test-team".to_string()),
            team: Some("override-team".to_string()),
            all: false,
            unread_only: true,
            pending_ack_only: false,
            history: false,
            since_last_seen: false,
            no_since_last_seen: true,
            no_mark: true,
            no_update_seen: true,
            limit: Some(7),
            since: Some("2026-05-05T12:00:00Z".to_string()),
            from: Some("sender-a".to_string()),
            json: true,
            timeout: Some(12),
            actor: Some("reader-a".to_string()),
        };
        let tempdir = TempDir::new().expect("tempdir");
        let home_dir = tempdir.path().join("home");
        let current_dir = tempdir.path().join("cwd");

        let query = command.build_query(home_dir, current_dir).expect("query");

        assert_eq!(query.selection_mode, ReadSelection::UnreadOnly);
        assert!(!query.seen_state_filter);
        assert!(!query.seen_state_update);
        assert_eq!(query.ack_activation_mode, AckActivationMode::ReadOnly);
        assert_eq!(query.limit, Some(7));
        assert_eq!(
            query.sender_filter.as_ref().map(|value| value.as_str()),
            Some("sender-a")
        );
        assert_eq!(query.timeout_secs, Some(12));
        assert!(query.timestamp_filter.is_some());
        assert_eq!(
            query.actor_override.as_ref().map(|value| value.as_str()),
            Some("reader-a")
        );
        assert_eq!(
            query.target_address.as_ref().map(|value| value.to_string()),
            Some("recipient-a@test-team".to_string())
        );
        assert_eq!(
            query.team_override.as_ref().map(|value| value.as_str()),
            Some("override-team")
        );
    }
}
