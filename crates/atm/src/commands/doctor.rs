use crate::observability::CliObservability;
use crate::output;
use anyhow::Result;
use atm_core::doctor::{self, DoctorQuery};
use atm_runtime::assemble_default_runtime;
use clap::Args;

use crate::composition::{CliComposition, resolve_command_runtime_context};

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
    pub fn run(self, observability: &CliObservability) -> Result<()> {
        let (home_dir, current_dir) = resolve_command_runtime_context("doctor")?;
        let json = self.json;
        let report = self.execute(observability, home_dir, current_dir)?;

        let has_errors = report.has_errors();
        output::print_doctor_result(&report, json)?;
        if has_errors {
            std::process::exit(1);
        }
        Ok(())
    }

    fn build_query(
        &self,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    ) -> Result<DoctorQuery> {
        let team_override = self
            .team
            .as_ref()
            .map(|value| {
                value.parse::<atm_core::types::TeamName>().map_err(|error| {
                    error.with_recovery(
                        "Use `--team <team>` with a valid ATM team name when running `atm doctor`.",
                    )
                })
            })
            .transpose()?;
        Ok(DoctorQuery {
            home_dir,
            current_dir,
            team_override,
        })
    }

    fn execute(
        self,
        observability: &CliObservability,
        home_dir: std::path::PathBuf,
        current_dir: std::path::PathBuf,
    ) -> Result<atm_core::doctor::DoctorReport> {
        let query = self.build_query(home_dir.clone(), current_dir.clone())?;
        let runtime = assemble_default_runtime()?;
        let local_report = doctor::run_doctor_with_runtime_ports(
            query,
            observability,
            &runtime.service_runtime,
            &runtime.doctor_ports,
            None,
        )
        .map_err(anyhow::Error::from)?;
        let query = self.build_query(home_dir, current_dir)?;

        match CliComposition::bootstrap(
            "doctor",
            observability,
            &query.current_dir,
            &query.home_dir,
        ) {
            Ok(composition) => composition.doctor(query).map_err(anyhow::Error::from),
            Err(_) => Ok(local_report),
        }
    }
}

#[cfg(test)]
mod tests {
    use atm_core::error::AtmError;
    use atm_core::test_support::EnvGuard;
    use tempfile::TempDir;

    use super::DoctorCommand;
    use crate::observability::CliObservability;

    fn test_paths() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
        let tempdir = TempDir::new().expect("tempdir");
        let home_dir = tempdir.path().join("home");
        let current_dir = tempdir.path().join("cwd");
        (tempdir, home_dir, current_dir)
    }

    #[test]
    fn build_query_preserves_team_override() {
        let command = DoctorCommand {
            team: Some("test-team".to_string()),
            json: true,
        };

        let (_tempdir, home_dir, current_dir) = test_paths();
        let query = command.build_query(home_dir, current_dir).expect("query");

        assert_eq!(
            query.team_override.as_ref().map(|value| value.as_str()),
            Some("test-team")
        );
    }

    #[test]
    fn build_query_adds_recovery_for_invalid_team_override() {
        let command = DoctorCommand {
            team: Some("bad team".to_string()),
            json: false,
        };

        let (_tempdir, home_dir, current_dir) = test_paths();
        let error = command
            .build_query(home_dir, current_dir)
            .expect_err("invalid team override should fail");
        let atm_error = error.downcast_ref::<AtmError>().expect("atm error");

        assert_eq!(
            atm_error.primary_recovery(),
            Some(
                "Correct the ATM address format and retry with a valid <agent> or <agent>@<team> target."
            )
        );
    }

    #[test]
    fn execute_runs_direct_local_doctor_path() {
        let observability = CliObservability::fallback();
        let command = DoctorCommand {
            team: None,
            json: false,
        };
        let (_tempdir, home_dir, current_dir) = test_paths();
        std::fs::create_dir_all(&home_dir).expect("home dir");
        std::fs::create_dir_all(&current_dir).expect("current dir");
        std::fs::create_dir_all(home_dir.join(".atm").join("db")).expect("host db dir");
        let _env = EnvGuard::set_many([("HOME", Some(home_dir.to_str().expect("utf8 path")))]);
        let report = command
            .execute(&observability, home_dir, current_dir)
            .expect("report");

        assert!(report.daemon_runtime.is_none());
        assert!(report.runtime_status.is_none());
    }
}
