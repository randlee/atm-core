use std::path::PathBuf;

use anyhow::Result;
use atm_core::home;
use atm_core::team_admin::{self, AddMemberRequest, BackupRequest, RestoreRequest, RestoreResult};
use atm_core::types::{AgentName, TeamName};
use clap::{Args, Subcommand};

use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
/// List teams or run one team-administration subcommand.
pub struct TeamsCommand {
    #[command(subcommand)]
    command: Option<TeamsSubcommand>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Subcommand)]
enum TeamsSubcommand {
    AddMember(AddMemberCommand),
    Backup(BackupCommand),
    Restore(RestoreCommand),
}

#[derive(Debug, Args)]
struct AddMemberCommand {
    team: String,
    member: String,

    #[arg(long, default_value = "general-purpose")]
    agent_type: String,

    #[arg(long, default_value = "unknown")]
    model: String,

    #[arg(long)]
    cwd: Option<PathBuf>,

    #[arg(
        long = "pane-id",
        help = "tmux pane id in '%<number>' form or a bare numeric pane id"
    )]
    pane_id: Option<String>,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct BackupCommand {
    team: String,

    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct RestoreCommand {
    team: String,

    #[arg(long)]
    from: Option<PathBuf>,

    #[arg(long = "dry-run")]
    dry_run: bool,

    #[arg(long)]
    json: bool,
}

impl TeamsCommand {
    /// Execute the `atm teams` command.
    pub fn run(self, _observability: &CliObservability) -> Result<()> {
        let home_dir = home::atm_home()?;
        match self.command {
            None => {
                let outcome = team_admin::list_teams(home_dir, std::env::current_dir()?)?;
                output::print_teams_result(&outcome, self.json)
            }
            Some(TeamsSubcommand::AddMember(command)) => command.run(home_dir),
            Some(TeamsSubcommand::Backup(command)) => command.run(home_dir),
            Some(TeamsSubcommand::Restore(command)) => command.run(home_dir),
        }
    }
}

impl AddMemberCommand {
    fn run(self, home_dir: PathBuf) -> Result<()> {
        let json = self.json;
        let cwd = match self.cwd.clone() {
            Some(path) => path,
            None => std::env::current_dir()?,
        };
        let request = self.build_request(home_dir, cwd)?;
        let outcome = team_admin::add_member(request)?;
        output::print_add_member_result(&outcome, json)
    }

    fn build_request(self, home_dir: PathBuf, cwd: PathBuf) -> Result<AddMemberRequest> {
        Ok(AddMemberRequest {
            home_dir,
            team: self.team.parse::<TeamName>()?,
            member: self.member.parse::<AgentName>()?,
            agent_type: self.agent_type,
            model: self.model,
            cwd,
            tmux_pane_id: self.pane_id,
        })
    }
}

impl BackupCommand {
    fn run(self, home_dir: PathBuf) -> Result<()> {
        let outcome = team_admin::backup_team(BackupRequest {
            home_dir,
            team: self.team,
        })?;
        output::print_backup_result(&outcome, self.json)
    }
}

impl RestoreCommand {
    fn run(self, home_dir: PathBuf) -> Result<()> {
        match team_admin::restore_team(RestoreRequest {
            home_dir,
            team: self.team,
            from: self.from,
            dry_run: self.dry_run,
        })? {
            RestoreResult::Applied(outcome) => output::print_restore_result(&outcome, self.json),
            RestoreResult::DryRun(plan) => output::print_restore_plan(&plan, self.json),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AddMemberCommand;

    #[test]
    fn build_request_rejects_invalid_team_before_core() {
        let command = AddMemberCommand {
            team: "../evil".to_string(),
            member: "arch-ctm".to_string(),
            agent_type: "worker".to_string(),
            model: "gpt-5".to_string(),
            cwd: None,
            pane_id: None,
            json: false,
        };

        let error = command
            .build_request(".".into(), ".".into())
            .expect_err("invalid team");

        assert!(error.to_string().contains("team name"));
    }

    #[test]
    fn build_request_rejects_invalid_member_before_core() {
        let command = AddMemberCommand {
            team: "atm-dev".to_string(),
            member: "../evil".to_string(),
            agent_type: "worker".to_string(),
            model: "gpt-5".to_string(),
            cwd: None,
            pane_id: None,
            json: false,
        };

        let error = command
            .build_request(".".into(), ".".into())
            .expect_err("invalid member");

        assert!(error.to_string().contains("agent name"));
    }
}
