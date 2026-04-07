use anyhow::Result;
use atm_core::doctor::{self, DoctorQuery};
use atm_core::home;
use clap::Args;

use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
pub struct DoctorCommand {
    #[arg(long, help = "Override the resolved team for the doctor check.")]
    team: Option<String>,

    #[arg(long, help = "Emit the doctor report as JSON.")]
    json: bool,
}

impl DoctorCommand {
    pub fn run(self, observability: &CliObservability) -> Result<()> {
        let current_dir = std::env::current_dir()?;
        let home_dir = home::atm_home()?;
        let report = doctor::run_doctor(
            DoctorQuery {
                home_dir,
                current_dir,
                team_override: self.team,
            },
            observability,
        )?;

        let has_errors = report.has_errors();
        output::print_doctor_result(&report, self.json)?;
        if has_errors {
            std::process::exit(1);
        }
        Ok(())
    }
}
