use anyhow::Result;
use clap::{Parser, Subcommand};

pub mod ack;
pub(crate) mod caller_context;
pub mod clear;
pub mod daemon;
pub mod doctor;
pub mod help;
pub(crate) mod internal_nudge;
pub mod list;
pub mod log;
pub mod members;
pub mod peek;
pub mod read;
pub(crate) mod retained_roster;
pub mod send;
pub mod teams;
pub(crate) mod util;

pub use ack::AckCommand;
pub use clear::ClearCommand;
pub use daemon::DaemonCommand;
pub use doctor::DoctorCommand;
pub use help::HelpCommand;
pub(crate) use internal_nudge::InternalNudgeCommand;
pub use list::ListCommand;
pub use log::LogCommand;
pub use members::MembersCommand;
pub use peek::PeekCommand;
pub use read::ReadCommand;
pub use send::SendCommand;
pub use teams::TeamsCommand;

use crate::observability::CliObservability;

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
    pub fn run(self, observability: &CliObservability) -> Result<()> {
        self.command.run(observability)
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    Send(SendCommand),
    List(ListCommand),
    Peek(PeekCommand),
    Read(ReadCommand),
    Ack(AckCommand),
    Clear(ClearCommand),
    Daemon(DaemonCommand),
    Log(LogCommand),
    Doctor(DoctorCommand),
    Help(HelpCommand),
    #[command(hide = true)]
    InternalNudge(InternalNudgeCommand),
    Teams(TeamsCommand),
    Members(MembersCommand),
}

impl Command {
    fn run(self, observability: &CliObservability) -> Result<()> {
        match self {
            Self::Send(command) => command.run(observability),
            Self::List(command) => command.run(observability),
            Self::Peek(command) => command.run(observability),
            Self::Read(command) => command.run(observability),
            Self::Ack(command) => command.run(observability),
            Self::Clear(command) => command.run(observability),
            Self::Daemon(command) => command.run(observability),
            Self::Log(command) => command.run(observability),
            Self::Doctor(command) => command.run(observability),
            Self::Help(command) => command.run(observability),
            Self::InternalNudge(command) => command.run(observability),
            Self::Teams(command) => command.run(observability),
            Self::Members(command) => command.run(observability),
        }
    }
}
