use anyhow::{Context, Result};
use atm_core::ack::AckRequest;
use atm_core::home;
use atm_core::schema::AtmMessageId;
use clap::Args;

use crate::commands::caller_context::{
    CallerContextOverrides, CallerIdentityOverride, CallerTeamOverride, resolve_cli_caller_context,
};
use crate::composition::CliComposition;
use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
/// Acknowledge one pending-ack message and send a reply.
pub struct AckCommand {
    message_id: String,
    reply: String,

    #[arg(long)]
    team: Option<String>,

    #[arg(long = "as")]
    actor: Option<String>,

    #[arg(long)]
    json: bool,
}

impl AckCommand {
    /// Execute the `atm ack` command.
    pub fn run(self, observability: &CliObservability) -> Result<()> {
        let current_dir = std::env::current_dir()?;
        let home_dir = home::atm_home()?;
        let json = self.json;
        let request = self.build_request(home_dir, current_dir)?;
        let composition = CliComposition::bootstrap("ack", observability)?;
        let outcome = composition.ack(request)?;

        output::print_ack_result(&outcome, json)
    }

    fn build_request(
        self,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    ) -> Result<AckRequest> {
        let caller_context = resolve_cli_caller_context(CallerContextOverrides {
            identity_override: self.actor.as_deref().map(CallerIdentityOverride),
            team_override: self.team.as_deref().map(CallerTeamOverride),
        })?;
        let message_id = self
            .message_id
            .parse::<AtmMessageId>()
            .with_context(|| format!("invalid message id: {}", self.message_id))?;

        Ok(AckRequest {
            home_dir,
            current_dir,
            caller_identity: caller_context.caller_identity,
            caller_team: caller_context.caller_team,
            message_id,
            reply_body: self.reply,
        })
    }
}

#[cfg(test)]
mod tests {
    use atm_core::test_support::EnvGuard;
    use atm_core::test_support::{ROLE_TEAM_LEAD, TEST_TEAM};
    use serial_test::serial;
    use tempfile::TempDir;

    use super::AckCommand;

    const VALID_MESSAGE_ID: &str = "01KRFK5QTF2R6NRS3Q0F8Z9K0S";

    fn test_paths() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
        let tempdir = TempDir::new().expect("tempdir");
        let home_dir = tempdir.path().join("home");
        let current_dir = tempdir.path().join("cwd");
        (tempdir, home_dir, current_dir)
    }

    #[test]
    fn build_request_rejects_empty_message_id() {
        let command = AckCommand {
            message_id: String::new(),
            reply: "working on it".to_string(),
            team: Some(TEST_TEAM.to_string()),
            actor: Some(ROLE_TEAM_LEAD.to_string()),
            json: false,
        };

        let (_tempdir, home_dir, current_dir) = test_paths();
        let error = command
            .build_request(home_dir, current_dir)
            .expect_err("empty message id");

        assert!(error.to_string().contains("invalid message id"));
    }

    #[test]
    fn build_request_rejects_whitespace_only_message_id() {
        let command = AckCommand {
            message_id: "   ".to_string(),
            reply: "working on it".to_string(),
            team: Some(TEST_TEAM.to_string()),
            actor: Some(ROLE_TEAM_LEAD.to_string()),
            json: false,
        };

        let (_tempdir, home_dir, current_dir) = test_paths();
        let error = command
            .build_request(home_dir, current_dir)
            .expect_err("whitespace message id");

        assert!(error.to_string().contains("invalid message id"));
    }

    #[test]
    fn build_request_preserves_team_override_and_actor() {
        let command = AckCommand {
            message_id: VALID_MESSAGE_ID.to_string(),
            reply: "received".to_string(),
            team: Some("test-team".to_string()),
            actor: Some("sender-a".to_string()),
            json: true,
        };

        let (_tempdir, home_dir, current_dir) = test_paths();
        let request = command
            .build_request(home_dir, current_dir)
            .expect("request");

        assert_eq!(Some(request.caller_team.as_str()), Some("test-team"));
        assert_eq!(Some(request.caller_identity.as_str()), Some("sender-a"));
        assert_eq!(request.reply_body, "received");
    }

    #[test]
    #[serial(env)]
    fn build_request_uses_environment_when_overrides_are_absent() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("sender-a")),
            ("ATM_TEAM", Some(TEST_TEAM)),
        ]);
        let command = AckCommand {
            message_id: VALID_MESSAGE_ID.to_string(),
            reply: "received".to_string(),
            team: None,
            actor: None,
            json: false,
        };

        let (_tempdir, home_dir, current_dir) = test_paths();
        let request = command
            .build_request(home_dir, current_dir)
            .expect("request");

        assert_eq!(request.caller_identity.as_str(), "sender-a");
        assert_eq!(request.caller_team.as_str(), TEST_TEAM);
    }

    #[test]
    #[serial(env)]
    fn build_request_prefers_cli_overrides_over_environment() {
        let _env = EnvGuard::set_many([
            ("ATM_IDENTITY", Some("env-sender")),
            ("ATM_TEAM", Some("env-team")),
        ]);
        let command = AckCommand {
            message_id: VALID_MESSAGE_ID.to_string(),
            reply: "received".to_string(),
            team: Some(TEST_TEAM.to_string()),
            actor: Some(ROLE_TEAM_LEAD.to_string()),
            json: false,
        };

        let (_tempdir, home_dir, current_dir) = test_paths();
        let request = command
            .build_request(home_dir, current_dir)
            .expect("request");

        assert_eq!(request.caller_identity.as_str(), ROLE_TEAM_LEAD);
        assert_eq!(request.caller_team.as_str(), TEST_TEAM);
    }
}
