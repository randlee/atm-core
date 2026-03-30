use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct AckCommand {}

impl AckCommand {
    pub fn run(self) -> Result<()> {
        println!("ack not yet implemented");
        Ok(())
    }
}
