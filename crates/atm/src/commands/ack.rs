use anyhow::{Context, Result};
use atm_core::ack::{self, AckRequest};
use atm_core::home;
use atm_core::schema::LegacyMessageId;
use atm_rusqlite::RusqliteStore;
use clap::Args;

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
        let message_id = self
            .message_id
            .parse::<LegacyMessageId>()
            .with_context(|| format!("invalid message id: {}", self.message_id))?;

        let request = AckRequest {
            home_dir,
            current_dir,
            actor_override: self.actor.map(|value| value.parse()).transpose()?,
            team_override: self.team.map(|value| value.parse()).transpose()?,
            message_id,
            reply_body: self.reply,
        };
        let team = ack::resolve_store_team(&request)?;
        let store = RusqliteStore::open_for_team_home(&request.home_dir, &team)?;
        let outcome = ack::ack_mail(request, &store, observability)?;

        output::print_ack_result(&outcome, self.json)
    }
}
