use std::sync::Arc;

use atm_core::error::AtmError;

#[path = "../bin_support/daemon_observability.rs"]
mod daemon_observability;

use daemon_observability::DaemonObservability;

fn main() {
    let exit_code = match run() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            atm_daemon::daemon_exit_code_for_error(&error).as_i32()
        }
    };
    std::process::exit(exit_code);
}

fn run() -> Result<(), AtmError> {
    let observability: Arc<dyn atm_daemon::DaemonRuntimeObservability> =
        Arc::new(DaemonObservability::bootstrap()?);
    atm_daemon::run_daemon_with_observability(observability)
}
