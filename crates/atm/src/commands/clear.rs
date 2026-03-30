use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct ClearCommand {}

impl ClearCommand {
    pub fn run(self) -> Result<()> {
        println!("clear not yet implemented");
        Ok(())
    }
}
