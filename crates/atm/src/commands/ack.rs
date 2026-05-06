use anyhow::{Context, Result};
use atm_core::ack::AckRequest;
use atm_core::home;
use atm_core::schema::LegacyMessageId;
use clap::Args;

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
        let composition = CliComposition::bootstrap(observability)?;
        let json = self.json;
        let outcome = composition.ack(self.build_request(home_dir, current_dir)?)?;

        output::print_ack_result(&outcome, json)
    }

    fn build_request(
        self,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    ) -> Result<AckRequest> {
        let message_id = self
            .message_id
            .parse::<LegacyMessageId>()
            .with_context(|| format!("invalid message id: {}", self.message_id))?;

        Ok(AckRequest {
            home_dir,
            current_dir,
            actor_override: self.actor.map(|value| value.parse()).transpose()?,
            team_override: self.team.map(|value| value.parse()).transpose()?,
            message_id,
            reply_body: self.reply,
        })
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::AckCommand;

    const VALID_MESSAGE_ID: &str = "550e8400-e29b-41d4-a716-446655440000";

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
            team: None,
            actor: None,
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
            team: None,
            actor: None,
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

        assert_eq!(
            request.team_override.as_ref().map(|value| value.as_str()),
            Some("test-team")
        );
        assert_eq!(
            request.actor_override.as_ref().map(|value| value.as_str()),
            Some("sender-a")
        );
        assert_eq!(request.reply_body, "received");
    }
}
