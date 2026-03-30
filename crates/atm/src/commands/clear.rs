use std::time::Duration;

use anyhow::{Context, Result};
use atm_core::clear::{self, ClearQuery};
use atm_core::home;
use clap::Args;

use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
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
    pub fn run(self, observability: &CliObservability) -> Result<()> {
        let current_dir = std::env::current_dir()?;
        let home_dir = home::atm_home()?;
        let older_than = self.older_than.as_deref().map(parse_duration).transpose()?;

        let outcome = clear::clear_mail(
            ClearQuery {
                home_dir,
                current_dir,
                actor_override: self.actor_override,
                target_address: self.target,
                team_override: self.team,
                older_than,
                idle_only: self.idle_only,
                dry_run: self.dry_run,
            },
            observability,
        )?;

        output::print_clear_result(&outcome, self.dry_run, self.json)
    }
}

fn parse_duration(raw: &str) -> Result<Duration> {
    let value = raw.trim();
    let (amount, unit) = value.split_at(value.len().saturating_sub(1));
    if amount.is_empty() {
        anyhow::bail!("invalid duration: {value}");
    }

    let amount = amount
        .parse::<u64>()
        .with_context(|| format!("invalid duration: {value}"))?;

    match unit {
        "s" => Ok(Duration::from_secs(amount)),
        "m" => Ok(Duration::from_secs(amount * 60)),
        "h" => Ok(Duration::from_secs(amount * 60 * 60)),
        "d" => Ok(Duration::from_secs(amount * 60 * 60 * 24)),
        _ => anyhow::bail!("invalid duration unit in {value}; use s, m, h, or d"),
    }
}
