mod commands;
mod observability;
mod output;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let observability = observability::init()?;
    let cli = commands::Cli::parse();
    cli.run(&observability)
}
