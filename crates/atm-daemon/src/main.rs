use std::process::ExitCode;

use atm_core::error::AtmError;

#[tokio::main]
async fn main() -> ExitCode {
    if version_requested(std::env::args().nth(1).as_deref()) {
        println!("{}", version_string());
        return ExitCode::SUCCESS;
    }

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(replacement_exit_code(&error))
        }
    }
}

fn version_requested(argument: Option<&str>) -> bool {
    matches!(argument, Some("--version" | "-V"))
}

fn version_string() -> String {
    format!("atm-daemon {}", env!("CARGO_PKG_VERSION"))
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

#[cfg(test)]
mod tests {
    use super::{version_requested, version_string};

    #[test]
    fn version_string_matches_the_binary_contract() {
        assert_eq!(version_string(), "atm-daemon 1.5.5");
        assert!(version_requested(Some("--version")));
        assert!(version_requested(Some("-V")));
        assert!(!version_requested(Some("--help")));
        assert!(!version_requested(None));
    }
}
