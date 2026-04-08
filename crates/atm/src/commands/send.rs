use std::path::PathBuf;

use anyhow::Result;
use atm_core::home;
use atm_core::send::{self, SendMessageSource, SendRequest};
use atm_core::types::{AgentName, TeamName};
use clap::Args;

use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
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
        let message_source = self.build_message_source()?;

        let outcome = send::send_mail(
            SendRequest {
                home_dir,
                current_dir,
                sender_override: self.from.map(AgentName::from),
                to: self.to,
                team_override: self.team.map(TeamName::from),
                message_source,
                summary_override: self.summary,
                requires_ack: self.requires_ack,
                task_id: self.task_id,
                dry_run: self.dry_run,
            },
            observability,
        )?;

        output::print_send_result(&outcome, self.json)
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
