use std::process::ExitCode;
use std::sync::Arc;

use atm_core::error::AtmError;
use daemon_observability::DaemonObservability;

#[allow(
    dead_code,
    private_interfaces,
    reason = "the retained logger adapter preserves its shutdown helpers while AL.9 consumes only its core ObservabilityPort boundary"
)]
mod daemon_observability;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(replacement_exit_code(&error))
        }
    }
}

async fn run() -> Result<(), AtmError> {
    let observability = Arc::new(DaemonObservability::bootstrap()?);
    atm_daemon_bootstrap::run_replacement_daemon_with_observability(observability).await
}

fn replacement_exit_code(error: &AtmError) -> u8 {
    if error.is_validation()
        || matches!(
            error.code(),
            atm_core::error::AtmErrorCode::ConfigParseFailed
                | atm_core::error::AtmErrorCode::ConfigHomeUnavailable
                | atm_core::error::AtmErrorCode::DaemonServingStateRejected
        )
    {
        64
    } else if error.code() == atm_core::error::AtmErrorCode::DaemonUnavailable {
        70
    } else {
        1
    }
}
