use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct SendCommand {}

impl SendCommand {
    pub fn run(self) -> Result<()> {
        println!("send not yet implemented");
        Ok(())
    }
}
