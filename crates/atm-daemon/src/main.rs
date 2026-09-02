use std::process::ExitCode;
use std::sync::Arc;

use atm_core::error::AtmError;
use daemon_observability::DaemonObservability;

#[allow(
    dead_code,
    private_interfaces,
    reason = "the adapter retains private lifecycle exercises for its in-module retained-log contract tests"
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
    // Retained-log preparation performs filesystem I/O. Keep it off the
    // Tokio/Axum runtime worker while retaining the process-owned adapter at
    // this active replacement-daemon composition boundary.
    let observability = Arc::new(
        tokio::task::spawn_blocking(DaemonObservability::bootstrap)
            .await
            .map_err(|source| {
                AtmError::observability_bootstrap(format!(
                    "daemon observability bootstrap worker did not complete: {source}"
                ))
            })??,
    );
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
