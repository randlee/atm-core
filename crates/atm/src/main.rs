mod commands;
mod observability;
mod output;
mod sc_observability_adapter;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use atm_core::error::AtmError;
use atm_core::home;
use clap::Parser;
use clap::error::ErrorKind;
use sc_observability::{
    ConsoleSink, JsonlFileSink, LogSink, Logger, LoggerBuilder, LoggerConfig, RetentionPolicy,
    RotationPolicy, SinkRegistration,
};
use sc_observability_types::{ServiceName, SinkHealth, SinkHealthState, TargetCategory};

const ATM_SERVICE_NAME: &str = "atm";
const ATM_COMMAND_TARGET: &str = "atm.command";
const ATM_OBSERVABILITY_RETAINED_SINK_FAULT_ENV: &str = "ATM_OBSERVABILITY_RETAINED_SINK_FAULT";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConsoleLogRoute {
    Disabled,
    Stderr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetainedSinkFaultMode {
    Degraded,
    Unavailable,
}

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

    let observability = match init_observability(cli.stderr_logs()) {
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

fn init_observability(stderr_logs: bool) -> Result<observability::CliObservability, AtmError> {
    let home_dir = home::atm_home()?;
    let console_log_route = if stderr_logs {
        ConsoleLogRoute::Stderr
    } else {
        ConsoleLogRoute::Disabled
    };
    let service_name = ServiceName::new(ATM_SERVICE_NAME).map_err(|source| {
        AtmError::observability_bootstrap("failed to validate ATM service name").with_source(source)
    })?;
    let target_category = TargetCategory::new(ATM_COMMAND_TARGET).map_err(|source| {
        AtmError::observability_bootstrap("failed to validate ATM observability target")
            .with_source(source)
    })?;
    let logger = build_logger(&home_dir, console_log_route, &service_name)?;

    Ok(observability::CliObservability::from_boxed_port(Box::new(
        sc_observability_adapter::ScObservabilityAdapter::new(
            logger,
            service_name,
            target_category,
        ),
    )))
}

pub(crate) fn build_logger(
    home_dir: &Path,
    console_log_route: ConsoleLogRoute,
    service_name: &ServiceName,
) -> Result<Logger, AtmError> {
    let mut config = LoggerConfig::default_for(service_name.clone(), log_root(home_dir));
    // ATM CLI owns stdout/stderr UX by default; only opt into a shared
    // console sink when the CLI routing rule explicitly selects one.
    config.enable_console_sink = false;
    let mut builder = Logger::builder(config).map_err(|source| {
        AtmError::observability_bootstrap("failed to initialize shared observability logger")
            .with_source(source)
    })?;
    if console_log_route == ConsoleLogRoute::Stderr {
        builder.register_sink(SinkRegistration::new(Arc::new(ConsoleSink::stderr())));
    }
    if let Some(mode) = retained_sink_fault_mode()? {
        register_retained_sink_fault(&mut builder, home_dir, mode);
    }
    Ok(builder.build())
}

fn log_root(home_dir: &Path) -> PathBuf {
    home_dir.join(".local").join("share")
}

fn fault_injection_log_path(home_dir: &Path) -> PathBuf {
    log_root(home_dir)
        .join("logs")
        .join("atm-fault-injection.log.jsonl")
}

fn retained_sink_fault_mode() -> Result<Option<RetainedSinkFaultMode>, AtmError> {
    let Some(value) = std::env::var(ATM_OBSERVABILITY_RETAINED_SINK_FAULT_ENV)
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    match value.as_str() {
        "degraded" => Ok(Some(RetainedSinkFaultMode::Degraded)),
        "unavailable" => Ok(Some(RetainedSinkFaultMode::Unavailable)),
        _ => Err(AtmError::observability_bootstrap(format!(
            "invalid {ATM_OBSERVABILITY_RETAINED_SINK_FAULT_ENV} value `{value}`; use `degraded` or `unavailable`"
        ))),
    }
}

fn register_retained_sink_fault(
    builder: &mut LoggerBuilder,
    home_dir: &Path,
    mode: RetainedSinkFaultMode,
) {
    let sink = Arc::new(JsonlFileSink::new(
        fault_injection_log_path(home_dir),
        RotationPolicy::default(),
        RetentionPolicy::default(),
    ));
    builder.register_sink(SinkRegistration::new(Arc::new(
        RetainedSinkHealthOverride::new(sink, mode),
    )));
}

struct RetainedSinkHealthOverride {
    inner: Arc<dyn LogSink>,
    mode: RetainedSinkFaultMode,
}

impl RetainedSinkHealthOverride {
    fn new(inner: Arc<dyn LogSink>, mode: RetainedSinkFaultMode) -> Self {
        Self { inner, mode }
    }

    fn forced_state(&self) -> SinkHealthState {
        match self.mode {
            RetainedSinkFaultMode::Degraded => SinkHealthState::DegradedDropping,
            RetainedSinkFaultMode::Unavailable => SinkHealthState::Unavailable,
        }
    }
}

impl LogSink for RetainedSinkHealthOverride {
    fn write(
        &self,
        event: &sc_observability_types::LogEvent,
    ) -> Result<(), sc_observability_types::LogSinkError> {
        self.inner.write(event)
    }

    fn flush(&self) -> Result<(), sc_observability_types::LogSinkError> {
        self.inner.flush()
    }

    fn health(&self) -> SinkHealth {
        let mut health = self.inner.health();
        health.state = self.forced_state();
        health
    }
}
