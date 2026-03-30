use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct DoctorCommand {}

impl DoctorCommand {
    pub fn run(self) -> Result<()> {
        println!("doctor not yet implemented");
        Ok(())
    }
}
