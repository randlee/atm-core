use anyhow::Result;
use atm_core::home;
use atm_core::team_admin::{self, MembersQuery};
use clap::Args;

use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
/// List the current member roster for one ATM team.
pub struct MembersCommand {
    #[arg(long)]
    team: Option<String>,

    #[arg(long)]
    json: bool,
}

impl MembersCommand {
    /// Execute the `atm members` command.
    pub fn run(self, _observability: &CliObservability) -> Result<()> {
        let home_dir = home::atm_home()?;
        let current_dir = std::env::current_dir()?;
        let outcome = team_admin::list_members(MembersQuery {
            home_dir,
            current_dir,
            team_override: self.team,
        })?;
        output::print_members_result(&outcome, self.json)
    }
}
