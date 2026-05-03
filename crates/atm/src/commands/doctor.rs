use atm_core::doctor::{DoctorQuery, DoctorReport};
use atm_core::error::AtmError;
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
    pub fn run(self, _observability: &CliObservability) -> Result<(), AtmError> {
        let current_dir = std::env::current_dir()
            .map_err(|error| AtmError::home_directory_unavailable().with_source(error))?;
        let home_dir = home::atm_home()?;
        let payload_json = atm_daemon::request_doctor_json_with_autostart(DoctorQuery {
            home_dir,
            current_dir,
            team_override: self.team.map(|value| value.parse()).transpose()?,
        })?;
        let report: DoctorReport = serde_json::from_str(&payload_json).map_err(|error| {
            AtmError::daemon_protocol("failed to decode doctor response payload").with_source(error)
        })?;
        let has_errors = report.has_errors();
        output::print_doctor_result(&report, self.json).map_err(|error| {
            AtmError::daemon_protocol(format!("failed to render doctor report: {error}"))
        })?;
        if has_errors {
            std::process::exit(1);
        }
        Ok(())
    }
}
