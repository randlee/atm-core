use anyhow::Result;
use atm_core::home;
use atm_core::read::{MAX_TIMEOUT_SECS, ReadQuery};
use atm_core::types::{AckActivationMode, ReadSelection};
use clap::Args;

use crate::commands::util::parse_timestamp;
use crate::composition::CliComposition;
use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
/// Read one ATM mailbox message and optionally update read state.
pub struct ReadCommand {
    target: Option<String>,

    #[arg(long)]
    team: Option<String>,

    #[arg(long, conflicts_with_all = ["unread", "unread_only", "pending_ack", "pending_ack_only"])]
    all: bool,

    #[arg(long, conflicts_with_all = ["pending_ack", "pending_ack_only", "all"])]
    unread: bool,

    #[arg(long = "unread-only", conflicts_with_all = ["pending_ack", "pending_ack_only", "all"])]
    unread_only: bool,

    #[arg(long = "pending-ack", conflicts_with_all = ["unread", "unread_only", "all"])]
    pending_ack: bool,

    #[arg(long = "pending-ack-only", conflicts_with_all = ["unread", "unread_only", "all"])]
    pending_ack_only: bool,

    #[arg(long, conflicts_with_all = ["message_id", "task", "contains", "from", "since"])]
    history: bool,

    #[arg(long = "message-id", conflicts_with_all = ["task", "contains", "from", "since"])]
    message_id: Option<String>,

    #[arg(long)]
    task: Option<String>,

    #[arg(long)]
    contains: Option<String>,

    #[arg(long)]
    since_last_seen: bool,

    #[arg(long = "no-since-last-seen", default_value_t = false)]
    no_since_last_seen: bool,

    #[arg(long)]
    no_mark: bool,

    #[arg(long)]
    no_update_seen: bool,

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
    pub fn run(self, observability: &CliObservability) -> Result<()> {
        let warnings = self.deprecation_warnings();
        let current_dir = std::env::current_dir()?;
        let home_dir = home::atm_home()?;
        let json = self.json;
        let composition = CliComposition::bootstrap("read", observability)?;
        let query = self.build_query(home_dir, current_dir)?;
        let outcome = composition.receive(query)?;
        output::print_read_result(&outcome, json)?;
        for warning in warnings {
            eprintln!("{warning}");
        }
        Ok(())
    }

    fn build_query(
        &self,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    ) -> Result<ReadQuery> {
        if let Some(timeout_secs) = self.timeout
            && timeout_secs > MAX_TIMEOUT_SECS
        {
            return Err(atm_core::error::AtmError::validation(format!(
                "timeout exceeds the {} second maximum",
                MAX_TIMEOUT_SECS
            ))
            .with_recovery("Use a timeout no greater than one hour before retrying `atm read`.")
            .into());
        }
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
            !self.no_since_last_seen && selection_mode != ReadSelection::All,
            !self.no_update_seen,
            if self.no_mark {
                AckActivationMode::ReadOnly
            } else {
                AckActivationMode::PromoteDisplayedUnread
            },
            self.message_id.as_deref(),
            self.from.as_deref(),
            timestamp_filter,
            self.task.as_deref(),
            self.contains.as_deref(),
            self.timeout,
        )
        .map_err(Into::into)
    }

    fn selection_mode(&self) -> ReadSelection {
        if self.all || self.history {
            ReadSelection::All
        } else if self.unread || self.unread_only {
            ReadSelection::Unread
        } else if self.pending_ack || self.pending_ack_only {
            ReadSelection::PendingAck
        } else {
            ReadSelection::Actionable
        }
    }

    fn deprecation_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        if self.unread_only {
            warnings.push(
                "warning: `atm read --unread-only` is deprecated; use `atm read --unread`."
                    .to_string(),
            );
        }
        if self.pending_ack_only {
            warnings.push(
                "warning: `atm read --pending-ack-only` is deprecated; use `atm read --pending-ack`."
                    .to_string(),
            );
        }
        if self.history {
            warnings.push(
                "warning: `atm read --history` is deprecated; use `atm read --all`.".to_string(),
            );
        }
        if self.since_last_seen {
            warnings.push(
                "note: `atm read --since-last-seen` remains supported as an explicit restatement of the default seen-state behavior."
                    .to_string(),
            );
        }
        warnings
    }
}

#[cfg(test)]
mod tests {
    use atm_core::types::{AckActivationMode, ReadSelection};

    use super::ReadCommand;

    #[test]
    fn selection_mode_maps_cli_flags_to_expected_bucket() {
        let mut command = base_command();
        command.all = true;
        assert_eq!(command.selection_mode(), ReadSelection::All);

        let mut command = base_command();
        command.unread = true;
        assert_eq!(command.selection_mode(), ReadSelection::Unread);

        let mut command = base_command();
        command.pending_ack = true;
        assert_eq!(command.selection_mode(), ReadSelection::PendingAck);

        let command = base_command();
        assert_eq!(command.selection_mode(), ReadSelection::Actionable);
    }

    #[test]
    fn build_query_propagates_filters_and_exact_message_id() {
        let mut command = base_command();
        command.target = Some("recipient-a@test-team".to_string());
        command.team = Some("override-team".to_string());
        command.message_id = Some("550e8400-e29b-41d4-a716-446655440000".to_string());
        command.no_since_last_seen = true;
        command.no_mark = true;
        command.no_update_seen = true;
        command.timeout = Some(9);

        let query = command.build_query(".".into(), ".".into()).expect("query");

        assert_eq!(query.selection_mode(), ReadSelection::Actionable);
        assert_eq!(query.ack_activation_mode(), AckActivationMode::ReadOnly);
        assert!(!query.seen_state_filter());
        assert!(!query.seen_state_update());
        assert_eq!(query.timeout_secs(), Some(9));
        assert!(query.message_id_filter().is_some());
    }

    #[test]
    fn deprecation_warnings_cover_legacy_aliases() {
        let mut command = base_command();
        command.unread_only = true;
        command.pending_ack_only = true;
        command.history = true;
        command.since_last_seen = true;

        let warnings = command.deprecation_warnings();
        assert_eq!(warnings.len(), 4);
    }

    fn base_command() -> ReadCommand {
        ReadCommand {
            target: None,
            team: None,
            all: false,
            unread: false,
            unread_only: false,
            pending_ack: false,
            pending_ack_only: false,
            history: false,
            message_id: None,
            task: None,
            contains: None,
            since_last_seen: false,
            no_since_last_seen: false,
            no_mark: false,
            no_update_seen: false,
            since: None,
            from: None,
            json: false,
            timeout: None,
            actor: None,
        }
    }
}
