use anyhow::Result;
use clap::{Parser, Subcommand};

pub mod ack;
pub mod clear;
pub mod doctor;
pub mod log;
pub mod read;
pub mod send;

pub use ack::AckCommand;
pub use clear::ClearCommand;
pub use doctor::DoctorCommand;
pub use log::LogCommand;
pub use read::ReadCommand;
pub use send::SendCommand;

use crate::observability::CliObservability;

#[derive(Debug, Parser)]
#[command(
    name = "atm",
    about = "ATM CLI",
    version,
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

impl Cli {
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
}

impl Command {
    fn run(self, observability: &CliObservability) -> Result<()> {
        match self {
            Self::Send(command) => command.run(observability),
            Self::Read(command) => command.run(),
            Self::Ack(command) => command.run(),
            Self::Clear(command) => command.run(),
            Self::Log(command) => command.run(),
            Self::Doctor(command) => command.run(),
        }
    }
}
