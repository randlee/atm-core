use std::time::Duration;

use anyhow::{Context, Result};
use atm_core::address::AgentAddress;
use atm_core::clear::{self, ClearQuery};
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
        let query = self.build_query(home_dir, current_dir)?;
        let outcome = clear::clear_mail(query, observability)?;
        output::print_clear_result(&outcome, dry_run, json)
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
}
