//! Feature-gated isolated benchmark entrypoint.
//!
//! The shipped `atm-daemon` binary cannot select a disabled received hook.
//! Capacity tooling must build this target with `benchmark-harness` and pass
//! the mode explicitly on its command line.

use atm_core::error::AtmError;
use atm_daemon_bootstrap::BenchmarkHookMode;

#[tokio::main]
async fn main() {
    let result = match hook_mode_from_args() {
        Ok(mode) => atm_daemon_bootstrap::run_benchmark_daemon(mode).await,
        Err(error) => Err(error),
    };
    let exit_code = match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            replacement_exit_code(&error)
        }
    };
    std::process::exit(exit_code);
}

fn hook_mode_from_args() -> Result<BenchmarkHookMode, AtmError> {
    let mut args = std::env::args().skip(1);
    match (args.next().as_deref(), args.next()) {
        (Some("--hook-mode"), Some(value)) => BenchmarkHookMode::parse(&value),
        _ => Err(AtmError::config(
            "usage: atm-daemon-benchmark --hook-mode <active|disabled>",
        )),
    }
}

fn replacement_exit_code(error: &AtmError) -> i32 {
    if error.is_validation() || error.code() == atm_core::error::AtmErrorCode::DaemonUnavailable {
        64
    } else {
        1
    }
}
