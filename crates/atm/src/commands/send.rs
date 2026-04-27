use std::path::PathBuf;

use anyhow::Result;
use atm_core::home;
use atm_core::send::{self, SendMessageSource, SendRequest};
use clap::Args;

use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
#[command(
    after_help = "Post-send hooks can be configured in .atm.toml via one or more [[atm.post_send_hooks]] rules with recipient = \"name-or-*\" and command = [\"argv\", ...]. Matching rules run after a successful non-dry-run send, in config order. Path-like command[0] values resolve relative to the declaring .atm.toml; bare executables like bash or python3 use normal PATH resolution. Recipient non-match is silent. For hook troubleshooting, combine --stderr-logs with ATM_LOG=debug to surface debug-level hook diagnostics on stderr."
)]
/// Send one ATM mailbox message.
pub struct SendCommand {
    #[arg()]
    to: String,

    #[arg(index = 2)]
    message: Option<String>,

    #[arg(long)]
    from: Option<String>,

    #[arg(long)]
    team: Option<String>,

    #[arg(long)]
    file: Option<PathBuf>,

    #[arg(long)]
    stdin: bool,

    #[arg(long)]
    summary: Option<String>,

    #[arg(long = "requires-ack")]
    requires_ack: bool,

    #[arg(long = "task-id")]
    task_id: Option<String>,

    #[arg(long)]
    dry_run: bool,

    #[arg(long)]
    json: bool,
}

impl SendCommand {
    /// Execute the `atm send` command.
    pub fn run(self, observability: &CliObservability) -> Result<()> {
        let current_dir = std::env::current_dir()?;
        let home_dir = home::atm_home()?;
        let json = self.json;
        let request = self.build_request(home_dir, current_dir)?;
        let outcome = send::send_mail(request, observability)?;

        output::print_send_result(&outcome, json)
    }

    fn build_request(self, home_dir: PathBuf, current_dir: PathBuf) -> Result<SendRequest> {
        let message_source = self.build_message_source()?;
        SendRequest::new(
            home_dir,
            current_dir,
            self.from.as_deref(),
            &self.to,
            self.team.as_deref(),
            message_source,
            self.summary,
            self.requires_ack,
            self.task_id,
            self.dry_run,
        )
        .map_err(Into::into)
    }

    fn build_message_source(&self) -> Result<SendMessageSource> {
        if self.stdin && self.file.is_some() {
            anyhow::bail!("--stdin and --file are mutually exclusive");
        }

        if self.stdin && self.message.is_some() {
            anyhow::bail!("--stdin and positional message text are mutually exclusive");
        }

        match (&self.file, self.stdin, &self.message) {
            (Some(path), false, message) => Ok(SendMessageSource::File {
                path: path.clone(),
                message: message.clone(),
            }),
            (None, true, None) => Ok(SendMessageSource::Stdin),
            (None, false, Some(message)) => Ok(SendMessageSource::Inline(message.clone())),
            (None, false, None) => {
                anyhow::bail!("provide message text, --file, or --stdin")
            }
            (Some(_), true, _) => unreachable!("validated above"),
            (None, true, Some(_)) => unreachable!("validated above"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SendCommand;

    #[test]
    fn build_request_rejects_invalid_target_before_core() {
        let command = SendCommand {
            to: "../evil".to_string(),
            message: Some("hello".to_string()),
            from: Some("team-lead".to_string()),
            team: Some("atm-dev".to_string()),
            file: None,
            stdin: false,
            summary: None,
            requires_ack: false,
            task_id: None,
            dry_run: false,
            json: false,
        };

        let error = command
            .build_request(".".into(), ".".into())
            .expect_err("invalid target");

        assert!(error.to_string().contains("agent name"));
    }
}
