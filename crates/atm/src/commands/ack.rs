use anyhow::{Context, Result};
use atm_core::schema::AtmMessageId;
use atm_core::send::SendRequest;
use clap::Args;

use crate::commands::caller_context::{CallerTeamOverride, resolve_cli_mutation_caller_context};
use crate::composition::{
    AtmHomePath, CliComposition, InvocationDir, resolve_command_runtime_context,
};
use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
/// Acknowledge one pending-ack message and emit a reply when required.
pub struct AckCommand {
    message_id: String,
    reply: String,

    #[arg(long)]
    team: Option<String>,

    #[arg(long)]
    json: bool,
}

impl AckCommand {
    /// Execute the `atm ack` command.
    pub fn run(self, observability: &CliObservability) -> Result<()> {
        let (home_dir, current_dir) = resolve_command_runtime_context("ack")?;
        let json = self.json;
        let request = self.build_request(home_dir.clone(), current_dir.clone())?;
        let composition = CliComposition::bootstrap(
            "ack",
            observability,
            InvocationDir::new(&current_dir),
            AtmHomePath::new(&home_dir),
        )?;
        let outcome = composition.send(request)?;

        output::print_send_result(&outcome, json)
    }

    fn build_request(
        self,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    ) -> Result<SendRequest> {
        let caller_context =
            resolve_cli_mutation_caller_context(self.team.as_deref().map(CallerTeamOverride))?;
        let message_id = self
            .message_id
            .parse::<AtmMessageId>()
            .with_context(|| format!("invalid message id: {}", self.message_id))?;

        Ok(SendRequest::acknowledgement(
            home_dir,
            current_dir,
            caller_context.caller_identity,
            caller_context.caller_team,
            message_id,
            self.reply,
        )?)
    }
}

#[cfg(test)]
mod tests {
    use atm_core::test_support::EnvGuard;
    use atm_core::test_support::TEST_TEAM;
    use serial_test::serial;
    use tempfile::TempDir;

    use super::AckCommand;

    fn test_paths() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
        let tempdir = TempDir::new().expect("tempdir");
        let home_dir = tempdir.path().join("home");
        let current_dir = tempdir.path().join("cwd");
        (tempdir, home_dir, current_dir)
    }

    #[test]
    #[serial(env)]
    fn build_request_rejects_empty_message_id() {
        let _env = EnvGuard::set_many([("ATM_IDENTITY", Some("sender-a"))]);
        let command = AckCommand {
            message_id: String::new(),
            reply: "working on it".to_string(),
            team: Some(TEST_TEAM.to_string()),
            json: false,
        };

        let (_tempdir, home_dir, current_dir) = test_paths();
        let error = command
            .build_request(home_dir, current_dir)
            .expect_err("empty message id");

        assert!(error.to_string().contains("invalid message id"));
    }
}
