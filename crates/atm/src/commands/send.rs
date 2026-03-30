use anyhow::Result;
use atm_core::send::{self, SendMessageSource, SendRequest};
use clap::Args;

use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
pub struct SendCommand {
    #[arg(long = "to")]
    to: String,

    #[arg(long = "message", short = 'm')]
    message: Option<String>,

    #[arg(long)]
    ack: bool,

    #[arg(long)]
    task: Option<String>,

    #[arg(long)]
    dry_run: bool,

    #[arg(long)]
    json: bool,
}

impl SendCommand {
    pub fn run(self, observability: &CliObservability) -> Result<()> {
        let current_dir = std::env::current_dir()?;
        let message_source = match self.message {
            Some(message) => SendMessageSource::InlineText(message),
            None => SendMessageSource::StdinText(send::input::read_message_from_stdin()?),
        };

        let outcome = send::execute(
            SendRequest {
                current_dir,
                sender_override: None,
                target_address: self.to,
                team_override: None,
                message_source,
                summary_override: None,
                requires_ack: self.ack,
                task_id: self.task,
                dry_run: self.dry_run,
            },
            observability,
        )?;

        output::print_send_result(&outcome, self.json)
    }
}
