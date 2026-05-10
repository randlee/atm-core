use atm_core::error::AtmError;

fn main() {
    let exit_code = match run() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    };
    std::process::exit(exit_code);
}

fn run() -> Result<(), AtmError> {
    let observability = atm_daemon::bootstrap_observability()?;
    init_tracing()?;
    atm_daemon::run_daemon_with_observability(observability)
}

fn init_tracing() -> Result<(), AtmError> {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_max_level(atm_daemon::tracing_level_override()?)
        .without_time()
        .try_init();
    Ok(())
}
