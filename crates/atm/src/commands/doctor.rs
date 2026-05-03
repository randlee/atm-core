use anyhow::Result;
use atm_core::doctor::{self, DoctorQuery};
use atm_core::home;
use clap::Args;

use crate::observability::CliObservability;
use crate::output;

#[derive(Debug, Args)]
/// Run ATM health and configuration diagnostics.
pub struct DoctorCommand {
    #[arg(long, help = "Override the resolved team for the doctor check.")]
    team: Option<String>,

    #[arg(long, help = "Emit the doctor report as JSON.")]
    json: bool,
}

impl DoctorCommand {
    // L.5 disposition (UNI-003): keep DoctorCommand injectability deferred for
    // initial release. Current service-level coverage exercises doctor behavior
    // without introducing a wider command abstraction before a concrete need
    // appears.
    /// Execute the `atm doctor` command.
    pub fn run(self, _observability: &CliObservability) -> Result<()> {
        let current_dir = std::env::current_dir()?;
        let home_dir = home::atm_home()?;
        atm_daemon::ensure_daemon_running(&home_dir)?;
        let report = doctor::run_doctor(
            DoctorQuery {
                home_dir,
                current_dir,
                team_override: self.team.map(|value| value.parse()).transpose()?,
            },
            _observability,
        )?;
        let has_errors = report.has_errors();
        output::print_doctor_result(&report, self.json)?;
        if has_errors {
            std::process::exit(1);
        }
        Ok(())
    }
}
