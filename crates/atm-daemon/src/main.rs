use std::sync::Arc;

use atm_core::error::AtmError;
use sc_observability as _;

mod daemon_observability;

use daemon_observability::DaemonObservability;

const _: Option<fn(sc_observability::Logger)> = None;

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
    atm_daemon_bootstrap::install_sqlite_retained_runtime_factory();
    let observability: Arc<dyn atm_daemon::DaemonRuntimeObservability> =
        Arc::new(DaemonObservability::bootstrap()?);
    atm_daemon::run_daemon_with_observability(observability)
}
