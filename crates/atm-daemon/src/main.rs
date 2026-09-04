use std::process::ExitCode;

use atm_core::error::AtmError;

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
    let observability = atm_daemon_bootstrap::bootstrap_replacement_observability().await?;
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
