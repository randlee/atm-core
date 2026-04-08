use anyhow::Result;
use clap::{Parser, Subcommand};

pub mod ack;
pub mod clear;
pub mod doctor;
pub mod log;
pub mod members;
pub mod read;
pub mod send;
pub mod teams;

pub use ack::AckCommand;
pub use clear::ClearCommand;
pub use doctor::DoctorCommand;
pub use log::LogCommand;
pub use members::MembersCommand;
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
    Read(ReadCommand),
    Ack(AckCommand),
    Clear(ClearCommand),
    Log(LogCommand),
    Doctor(DoctorCommand),
    Teams(TeamsCommand),
    Members(MembersCommand),
}

impl Command {
    fn run(self, observability: &CliObservability) -> Result<()> {
        match self {
            Self::Send(command) => command.run(observability),
            Self::Read(command) => command.run(observability),
            Self::Ack(command) => command.run(observability),
            Self::Clear(command) => command.run(observability),
            Self::Log(command) => command.run(observability),
            Self::Doctor(command) => command.run(observability),
            Self::Teams(command) => command.run(observability),
            Self::Members(command) => command.run(observability),
        }
    }
}
