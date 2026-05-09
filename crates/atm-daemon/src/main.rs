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
    init_tracing();
    atm_daemon::run_daemon()
}

fn init_tracing() {
    let level = match std::env::var("ATM_LOG")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
    {
        Some(value) => match value.as_str() {
            "trace" => tracing::Level::TRACE,
            "debug" => tracing::Level::DEBUG,
            "warn" => tracing::Level::WARN,
            "error" => tracing::Level::ERROR,
            _ => tracing::Level::INFO,
        },
        None => tracing::Level::WARN,
    };

    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_max_level(level)
        .without_time()
        .try_init();
}
