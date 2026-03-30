use anyhow::{Context, Result};
use atm_core::ack::{self, AckRequest};
use atm_core::home;
use clap::Args;
use uuid::Uuid;

use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
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
    pub fn run(self, observability: &CliObservability) -> Result<()> {
        let current_dir = std::env::current_dir()?;
        let home_dir = home::atm_home()?;
        let message_id = Uuid::parse_str(&self.message_id)
            .with_context(|| format!("invalid message id: {}", self.message_id))?;

        let outcome = ack::ack_mail(
            AckRequest {
                home_dir,
                current_dir,
                actor_override: self.actor,
                team_override: self.team,
                message_id,
                reply_body: self.reply,
            },
            observability,
        )?;

        output::print_ack_result(&outcome, self.json)
    }
}
