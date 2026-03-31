use std::time::Duration;

use anyhow::{Context, Result};
use atm_core::clear::{self, ClearQuery};
use atm_core::home;
use atm_core::observability::ObservabilityPort;
use clap::Args;

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
    pub fn run(self, observability: &dyn ObservabilityPort) -> Result<()> {
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
