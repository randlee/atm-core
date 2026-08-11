use std::time::Duration;

use anyhow::{Context, Result};
use atm_core::clear::ClearQuery;
use clap::Args;

use crate::commands::caller_context::{CallerTeamOverride, resolve_cli_mutation_caller_context};
use crate::composition::{
    AtmHomePath, CliComposition, InvocationDir, resolve_command_runtime_context,
};
use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
/// Clear read or acknowledged messages from a mailbox.
pub struct ClearCommand {
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
    pub async fn run(self, observability: &CliObservability) -> Result<()> {
        let (home_dir, current_dir) = resolve_command_runtime_context("clear")?;
        let dry_run = self.dry_run;
        let json = self.json;
        let query = self.build_query(home_dir.clone(), current_dir.clone())?;
        let composition = CliComposition::bootstrap(
            "clear",
            observability,
            InvocationDir::new(&current_dir),
            AtmHomePath::new(&home_dir),
        )?;
        let outcome = composition.clear(query).await?;
        output::print_clear_result(&outcome, dry_run, json)
    }

    fn build_query(
        self,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    ) -> Result<ClearQuery> {
        let caller_context =
            resolve_cli_mutation_caller_context(self.team.as_deref().map(CallerTeamOverride))?;
        let older_than = self.older_than.as_deref().map(parse_duration).transpose()?;

        Ok(ClearQuery {
            home_dir,
            current_dir,
            caller_identity: caller_context.caller_identity,
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
    use atm_core::test_support::{EnvGuard, TEST_TEAM};
    use clap::Parser;
    use serial_test::serial;

    use super::ClearCommand;

    #[test]
    fn cli_rejects_positional_mailbox_target_for_owner_only_clear() {
        let error = crate::commands::Cli::try_parse_from(["atm", "clear", "recipient@test-team"])
            .expect_err("owner-only clear must reject positional target");

        let rendered = error.to_string();
        assert!(
            rendered.contains("unexpected argument 'recipient@test-team'")
                || rendered.contains("unexpected argument"),
            "{rendered}"
        );
    }

    #[test]
    #[serial(env)]
    fn build_query_uses_environment_when_overrides_are_absent() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("sender-a")),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);
        let command = ClearCommand {
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
    fn build_query_preserves_environment_identity_with_team_override() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("env-sender")),
            ("ATM_TEAM", Some("env-team")),
        ]);
        let command = ClearCommand {
            team: Some(TEST_TEAM.to_string()),
            older_than: None,
            idle_only: false,
            dry_run: false,
            json: false,
        };

        let query = command.build_query(".".into(), ".".into()).expect("query");

        assert_eq!(query.caller_identity.as_str(), "env-sender");
        assert_eq!(query.caller_team.as_str(), TEST_TEAM);
    }
}
