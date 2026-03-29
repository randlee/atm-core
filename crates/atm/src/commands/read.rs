use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct ReadCommand {}

impl ReadCommand {
    pub fn run(self) -> Result<()> {
        println!("read not yet implemented");
        Ok(())
    }
}
