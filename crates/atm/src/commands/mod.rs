use anyhow::Result;
#[cfg(any(test, feature = "cli-surface-dump"))]
use clap::ValueEnum;
use clap::{Parser, Subcommand};

pub mod ack;
pub mod api;
pub(crate) mod caller_context;
pub mod clear;
pub mod compose;
pub mod doctor;
pub mod help;
pub(crate) mod internal_heartbeat;
pub(crate) mod internal_nudge;
pub(crate) mod internal_queue_get;
pub mod list;
pub mod log;
pub mod members;
pub mod peek;
pub mod peer;
pub mod queue;
pub mod read;
pub(crate) mod retained_roster;
pub mod search;
pub mod send;
pub(crate) mod send_fan_out;
pub(crate) mod send_to;
pub(crate) mod sender_roster;
pub(crate) mod task_ledger;
pub mod teams;
pub mod templates;
pub(crate) mod util;

pub use ack::AckCommand;
pub use api::ApiCommand;
pub use clear::ClearCommand;
pub use compose::ComposeCommand;
pub use doctor::DoctorCommand;
pub use help::HelpCommand;
pub(crate) use internal_heartbeat::InternalHeartbeatCommand;
pub(crate) use internal_nudge::InternalNudgeCommand;
pub(crate) use internal_queue_get::InternalQueueGetCommand;
pub use list::ListCommand;
pub use log::LogCommand;
pub use members::MembersCommand;
pub use peek::PeekCommand;
pub use peer::PeerCommand;
pub use queue::QueueCommand;
pub use read::ReadCommand;
pub use search::SearchCommand;
pub use send::SendCommand;
pub use teams::TeamsCommand;
pub use templates::TemplatesCommand;

use crate::observability::CliObservability;

/// Output format for the hidden maintainer CLI-surface dump command.
#[cfg(any(test, feature = "cli-surface-dump"))]
#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum CliSurfaceFormat {
    Json,
    Markdown,
}

/// Emit the live clap command tree for maintainers and the structural CLI gate.
#[cfg(any(test, feature = "cli-surface-dump"))]
#[derive(Debug, clap::Args)]
pub(crate) struct DumpCliSurfaceCommand {
    #[arg(long, value_enum)]
    format: CliSurfaceFormat,
}

#[cfg(any(test, feature = "cli-surface-dump"))]
impl DumpCliSurfaceCommand {
    fn run(self, _observability: &CliObservability) -> Result<()> {
        crate::dump_cli_surface(self.format).map_err(Into::into)
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "atm",
    about = "ATM CLI",
    version,
    disable_help_subcommand = true
)]
/// Top-level ATM command-line entrypoint.
pub struct Cli {
    /// Route retained observability console logs to stderr.
    ///
    /// ATM owns normal command stdout output; this flag opts the shared
    /// console sink into stderr so retained diagnostics do not pollute stdout.
    #[arg(long = "stderr-logs", global = true)]
    stderr_logs: bool,

    #[command(subcommand)]
    command: Command,
}

impl Cli {
    /// Return whether retained console logs should be routed to stderr.
    pub fn stderr_logs(&self) -> bool {
        self.stderr_logs
    }

    /// Run the selected ATM subcommand.
    pub async fn run(self, observability: &CliObservability) -> Result<()> {
        self.command.run(observability).await
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    Api(ApiCommand),
    Send(SendCommand),
    Queue(QueueCommand),
    Compose(ComposeCommand),
    List(ListCommand),
    Peek(PeekCommand),
    Peer(PeerCommand),
    Read(ReadCommand),
    Search(Box<SearchCommand>),
    Ack(AckCommand),
    Clear(ClearCommand),
    Log(LogCommand),
    Doctor(DoctorCommand),
    Help(HelpCommand),
    #[command(hide = true)]
    InternalNudge(InternalNudgeCommand),
    #[command(name = "_internal-heartbeat", hide = true)]
    InternalHeartbeat(InternalHeartbeatCommand),
    #[command(name = "_internal-queue-get", hide = true)]
    InternalQueueGet(InternalQueueGetCommand),
    #[cfg(any(test, feature = "cli-surface-dump"))]
    #[command(name = "__dump-cli-surface", hide = true)]
    DumpCliSurface(DumpCliSurfaceCommand),
    Teams(TeamsCommand),
    Members(MembersCommand),
    Templates(TemplatesCommand),
}

impl Command {
    async fn run(self, observability: &CliObservability) -> Result<()> {
        match self {
            Self::Api(command) => command.run(observability),
            Self::Send(command) => command.run(observability).await,
            Self::Queue(command) => command.run(observability).await,
            Self::Compose(command) => command.run(),
            Self::List(command) => command.run(observability).await,
            Self::Peek(command) => command.run(observability).await,
            Self::Peer(command) => command.run(observability).await,
            Self::Read(command) => command.run(observability).await,
            Self::Search(command) => command.run(observability).await,
            Self::Ack(command) => command.run(observability).await,
            Self::Clear(command) => command.run(observability).await,
            Self::Log(command) => command.run(observability),
            Self::Doctor(command) => command.run(observability).await,
            Self::Help(command) => command.run(observability),
            Self::InternalNudge(command) => command.run(observability).await,
            Self::InternalHeartbeat(command) => command.run(observability).await,
            Self::InternalQueueGet(command) => command.run(observability).await,
            #[cfg(any(test, feature = "cli-surface-dump"))]
            Self::DumpCliSurface(command) => command.run(observability),
            Self::Teams(command) => command.run(observability).await,
            Self::Members(command) => command.run(observability).await,
            Self::Templates(command) => command.run(observability).await,
        }
    }
}
