use std::time::Duration;

use anyhow::{Context, Result};
use atm_core::address::AgentAddress;
use atm_core::clear::ClearQuery;
use atm_core::home;
use clap::Args;

use crate::commands::caller_context::{
    CallerContextOverrides, CallerIdentityOverride, CallerTeamOverride, resolve_cli_caller_context,
};
use crate::composition::CliComposition;
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
        let composition = CliComposition::bootstrap("clear", observability)?;
        let outcome = composition.clear(query)?;
        output::print_clear_result(&outcome, dry_run, json)
    }

    fn build_query(
        self,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    ) -> Result<ClearQuery> {
        let caller_context = resolve_cli_caller_context(CallerContextOverrides {
            identity_override: self.actor_override.as_deref().map(CallerIdentityOverride),
            team_override: self.team.as_deref().map(CallerTeamOverride),
        })?;
        let older_than = self.older_than.as_deref().map(parse_duration).transpose()?;
        let target_address = self
            .target
            .as_deref()
            .map(str::parse::<AgentAddress>)
            .transpose()?;

        Ok(ClearQuery {
            home_dir,
            current_dir,
            caller_identity: caller_context.caller_identity,
            target_address,
            caller_team: caller_context.caller_team,
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
    use atm_core::test_support::{EnvGuard, ROLE_TEAM_LEAD, TEST_TEAM};
    use serial_test::serial;

    use super::ClearCommand;

    #[test]
    fn build_query_rejects_invalid_target_before_core() {
        let command = ClearCommand {
            target: Some("../evil".to_string()),
            actor_override: Some(ROLE_TEAM_LEAD.to_string()),
            team: Some(TEST_TEAM.to_string()),
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

    #[test]
    #[serial(env)]
    fn build_query_uses_environment_when_overrides_are_absent() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("sender-a")),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);
        let command = ClearCommand {
            target: None,
            actor_override: None,
            team: None,
            older_than: None,
            idle_only: false,
            dry_run: false,
            json: false,
        };

        let query = command.build_query(".".into(), ".".into()).expect("query");

        assert_eq!(query.caller_identity.as_str(), "sender-a");
        assert_eq!(query.caller_team.as_str(), TEST_TEAM);
    }

    #[test]
    #[serial(env)]
    fn build_query_prefers_cli_overrides_over_environment() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("env-sender")),
            ("ATM_TEAM", Some("env-team")),
        ]);
        let command = ClearCommand {
            target: None,
            actor_override: Some(ROLE_TEAM_LEAD.to_string()),
            team: Some(TEST_TEAM.to_string()),
            older_than: None,
            idle_only: false,
            dry_run: false,
            json: false,
        };

        let query = command.build_query(".".into(), ".".into()).expect("query");

        assert_eq!(query.caller_identity.as_str(), ROLE_TEAM_LEAD);
        assert_eq!(query.caller_team.as_str(), TEST_TEAM);
    }
}
