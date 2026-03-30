use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct LogCommand {}

impl LogCommand {
    pub fn run(self) -> Result<()> {
        println!("log not yet implemented");
        Ok(())
    }
}
