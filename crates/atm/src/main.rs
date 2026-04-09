mod commands;
mod observability;
mod output;
mod sc_observability_adapter;

use clap::Parser;
use clap::error::ErrorKind;

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

fn run() -> anyhow::Result<()> {
    let cli = match commands::Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                error.print()?;
                return Ok(());
            }
            let validation_error = atm_core::error::AtmError::validation(error.to_string());
            observability::CliObservability::fallback()
                .emit_fatal_error("parse", &validation_error);
            return Err(error.into());
        }
    };

    let observability = match observability::init(cli.stderr_logs()) {
        Ok(observability) => observability,
        Err(error) => {
            let fallback = observability::CliObservability::fallback();
            fallback.emit_fatal_error("bootstrap", &error);
            return Err(error.into());
        }
    };

    match cli.run(&observability) {
        Ok(()) => Ok(()),
        Err(error) => {
            observability.emit_fatal_error("service", error.as_ref());
            Err(error)
        }
    }
}
