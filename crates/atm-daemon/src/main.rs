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
async fn main() {
    let exit_code = match run().await {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            replacement_exit_code(&error)
        }
    };
    std::process::exit(exit_code);
}

async fn run() -> Result<(), AtmError> {
    let observability = Arc::new(DaemonObservability::bootstrap()?);
    atm_daemon_bootstrap::run_replacement_daemon_with_observability(observability).await
}

fn replacement_exit_code(error: &AtmError) -> i32 {
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
