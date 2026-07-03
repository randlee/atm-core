use anyhow::Result;
use atm_core::home;
use atm_core::list::ListQuery;
use clap::Args;

use crate::commands::caller_context::{
    CallerContextOverrides, CallerIdentityOverride, CallerTeamOverride, resolve_cli_caller_context,
};
use crate::commands::util::parse_timestamp;
use crate::composition::CliComposition;
use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
/// List one ATM mailbox surface as bounded metadata rows.
pub struct ListCommand {
    target: Option<String>,

    #[arg(long)]
    team: Option<String>,

    #[arg(long, conflicts_with_all = ["unread", "pending_ack"])]
    all: bool,

    #[arg(long, conflicts_with = "pending_ack")]
    unread: bool,

    #[arg(long = "pending-ack", conflicts_with = "unread")]
    pending_ack: bool,

    #[arg(long)]
    limit: Option<usize>,

    #[arg(long)]
    since: Option<String>,

    #[arg(long)]
    from: Option<String>,

    #[arg(long)]
    task: Option<String>,

    #[arg(long)]
    contains: Option<String>,

    #[arg(long)]
    json: bool,

    #[arg(long = "as")]
    actor: Option<String>,
}

impl ListCommand {
    pub fn run(self, observability: &CliObservability) -> Result<()> {
        let current_dir = std::env::current_dir()?;
        let home_dir = home::atm_home()?;
        let json = self.json;
        let query = self.build_query(home_dir, current_dir)?;
        let composition = CliComposition::bootstrap("list", observability)?;
        let outcome = composition.list(query)?;
        output::print_list_result(&outcome, json)
    }

    fn build_query(
        self,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    ) -> Result<ListQuery> {
        let caller_context = resolve_cli_caller_context(CallerContextOverrides {
            identity_override: self.actor.as_deref().map(CallerIdentityOverride),
            team_override: self.team.as_deref().map(CallerTeamOverride),
        })?;
        let selection_mode = self.selection_mode();
        let timestamp_filter = self.since.as_deref().map(parse_timestamp).transpose()?;
        ListQuery::new(
            home_dir,
            current_dir,
            caller_context.caller_identity,
            self.target.as_deref(),
            caller_context.caller_team,
            selection_mode,
            selection_mode != atm_core::types::ReadSelection::All,
            self.limit,
            self.from.as_deref(),
            timestamp_filter,
            self.task.as_deref(),
            self.contains.as_deref(),
        )
        .map_err(Into::into)
    }

    fn selection_mode(&self) -> atm_core::types::ReadSelection {
        if self.all {
            atm_core::types::ReadSelection::All
        } else if self.unread {
            atm_core::types::ReadSelection::Unread
        } else if self.pending_ack {
            atm_core::types::ReadSelection::PendingAck
        } else {
            atm_core::types::ReadSelection::Actionable
        }
    }
}

#[cfg(test)]
mod tests {
    use atm_core::types::ReadSelection;

    use super::ListCommand;

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
    fn build_query_preserves_limit_and_filters() {
        let mut command = base_command();
        command.target = Some("recipient-a@test-team".to_string());
        command.team = Some("override-team".to_string());
        command.limit = Some(12);
        command.task = Some("TASK-22".to_string());
        command.contains = Some("needle".to_string());

        let query = command.build_query(".".into(), ".".into()).expect("query");

        assert_eq!(query.selection_mode, ReadSelection::Actionable);
        assert_eq!(query.limit, Some(12));
        assert_eq!(
            query.task_filter.as_ref().map(|value| value.as_str()),
            Some("TASK-22")
        );
        assert_eq!(query.contains_filter.as_deref(), Some("needle"));
    }

    fn base_command() -> ListCommand {
        ListCommand {
            target: None,
            team: None,
            all: false,
            unread: false,
            pending_ack: false,
            limit: None,
            since: None,
            from: None,
            task: None,
            contains: None,
            json: false,
            actor: None,
        }
    }
}
