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
        let json = self.json;
        let outcome = team_admin::list_members(self.build_query(home_dir, current_dir)?)?;
        output::print_members_result(&outcome, json)
    }

    fn build_query(
        self,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    ) -> Result<MembersQuery> {
        Ok(MembersQuery {
            home_dir,
            current_dir,
            team_override: self.team.map(|value| value.parse()).transpose()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::MembersCommand;

    #[test]
    fn build_query_preserves_team_override() {
        let command = MembersCommand {
            team: Some("test-team".to_string()),
            json: true,
        };

        let query = command
            .build_query("/tmp/home".into(), "/tmp/cwd".into())
            .expect("query");

        assert_eq!(
            query.team_override.as_ref().map(|value| value.as_str()),
            Some("test-team")
        );
        assert_eq!(query.home_dir, std::path::PathBuf::from("/tmp/home"));
        assert_eq!(query.current_dir, std::path::PathBuf::from("/tmp/cwd"));
    }

    #[test]
    fn build_query_rejects_invalid_team_override() {
        let command = MembersCommand {
            team: Some("../evil".to_string()),
            json: false,
        };

        let error = command
            .build_query("/tmp/home".into(), "/tmp/cwd".into())
            .expect_err("invalid team");

        assert!(error.to_string().contains("team name"));
    }
}
