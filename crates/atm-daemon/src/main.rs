use atm_core::error::AtmError;

#[tokio::main]
async fn main() {
    let exit_code = match atm_daemon_bootstrap::run_replacement_daemon().await {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            replacement_exit_code(&error)
        }
    };
    std::process::exit(exit_code);
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
