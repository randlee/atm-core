//! Shared retained-log bootstrap policy for ATM process adapters.
//!
//! This crate owns the process-neutral filesystem readiness check and the
//! `ATM_LOG` / test fault-injection parsing contract. Concrete adapters retain
//! ownership of their logger and sink configuration.

use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fs, fs::OpenOptions};

use atm_core::error::AtmError;
use atm_core::observability::RetainedSinkFaultMode;
use sc_observability_types::DiagnosticInfo;

/// Opaque shared retained logger handle. Its concrete backend is deliberately
/// confined to this facade.
pub struct RetainedLogger(RetainedLoggerBackend);

#[allow(clippy::large_enum_variant)]
enum RetainedLoggerBackend {
    Logger(sc_observability::Logger),
    #[cfg(test)]
    QueueFull,
}

/// Process-neutral settings for ATM's retained JSONL logger.
#[derive(Debug, Clone, Copy)]
pub struct RetainedLogPolicy {
    pub rotation_max_bytes: u64,
    pub rotation_max_files: usize,
    pub retention_max_age: Duration,
    pub maintenance_cadence: Duration,
    pub writer_shutdown_timeout: Duration,
    pub maintenance_max_work_per_pass: Option<usize>,
}

/// The only admission outcomes callers need from the retained logger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetainedLogOffer {
    Accepted,
    QueueFull,
    Rejected { diagnostic_code: String },
}

impl RetainedLogger {
    pub fn health(&self) -> sc_observability_types::LoggingHealthReport {
        self.0.health()
    }

    pub fn try_log(&self, event: sc_observability_types::LogEvent) -> RetainedLogOffer {
        #[cfg(test)]
        if queue_full_for_test() {
            return RetainedLogOffer::QueueFull;
        }
        match self.0.try_log(event) {
            Ok(()) => RetainedLogOffer::Accepted,
            Err(sc_observability::TryLogError::QueueFull(_)) => RetainedLogOffer::QueueFull,
            Err(error) => RetainedLogOffer::Rejected {
                diagnostic_code: try_log_error_code(&error).to_string(),
            },
        }
    }

    #[cfg(test)]
    pub(crate) fn force_queue_full_for_test<T>(operation: impl FnOnce() -> T) -> T {
        QUEUE_FULL_FOR_TEST.with(|forced| {
            assert!(!forced.replace(true), "queue-full test mode must not nest");
            let result = operation();
            forced.set(false);
            result
        })
    }
}

/// Builds the daemon's one retained JSONL logger after the log path was checked.
pub fn build_retained_logger(
    service_name: sc_observability_types::ServiceName,
    log_dir: &Path,
    retained_log_policy: RetainedLogPolicy,
) -> Result<RetainedLogger, AtmError> {
    prepare_retained_log(log_dir)?;
    let mut config = sc_observability::LoggerConfig::default_for(
        service_name,
        logger_root_for_log_dir(log_dir)?,
    );
    config.level = logger_level_override()?.unwrap_or(sc_observability_types::LevelFilter::Info);
    config.retained_log_policy = sc_observability::RetainedLogPolicy {
        rotation_max_bytes: sc_observability::ByteCount::from_bytes(
            retained_log_policy.rotation_max_bytes,
        ),
        rotation_max_files: sc_observability_types::FileCount::from_usize(
            retained_log_policy.rotation_max_files,
        ),
        retention_max_age: sc_observability::RetentionMaxAge::from_duration(
            retained_log_policy.retention_max_age,
        ),
        maintenance_cadence: sc_observability::MaintenanceCadence::new(
            retained_log_policy.maintenance_cadence,
        ),
        writer_shutdown_timeout: sc_observability::WriterShutdownTimeout::new(
            retained_log_policy.writer_shutdown_timeout,
        ),
        maintenance_max_work_per_pass: retained_log_policy.maintenance_max_work_per_pass,
    };
    config.enable_console_sink = false;
    sc_observability::Logger::builder(config)
        .map(|builder| RetainedLogger(builder.build()))
        .map_err(|_| {
            AtmError::observability_bootstrap(
                "failed to initialize shared daemon observability logger",
            )
        })
}

#[cfg(test)]
thread_local! {
    static QUEUE_FULL_FOR_TEST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn queue_full_for_test() -> bool {
    QUEUE_FULL_FOR_TEST.with(std::cell::Cell::get)
}

fn try_log_error_code(error: &sc_observability::TryLogError) -> &str {
    match error {
        sc_observability::TryLogError::InvalidEvent(error) => error.diagnostic().code.as_str(),
        sc_observability::TryLogError::QueueFull(context)
        | sc_observability::TryLogError::WriterDegraded(context)
        | sc_observability::TryLogError::ShutdownTimedOut(context) => {
            context.diagnostic().code.as_str()
        }
    }
}

pub mod tracing_bridge;

pub use tracing_bridge::{
    BridgeError, CANONICAL_LOG_FILE_NAME, DiagnosticSink, DropReason, FieldValue,
    GRAFT_FALLBACK_LOG_FILE_NAME, RETAINED_FIELD_ALLOWLIST, RETAINED_INFO_TARGETS, RetainedEvent,
    SinkOffer, TracingBridgeLayer, TracingBridgeStats,
};

pub const ATM_LOG_LEVEL_ENV: &str = "ATM_LOG";
pub const ATM_RETAINED_SINK_FAULT_ENV: &str = "ATM_OBSERVABILITY_RETAINED_SINK_FAULT";

/// Returns AW.4's dedicated, bounded graft fallback satellite path.
pub fn graft_fallback_log_path(log_dir: &Path) -> PathBuf {
    log_dir.join(GRAFT_FALLBACK_LOG_FILE_NAME)
}

/// Validates a retained-log directory and its active log file before logger
/// construction. The returned path is the exact file checked for append access.
pub fn prepare_retained_log(log_dir: &Path) -> Result<PathBuf, AtmError> {
    let active_log_path = log_dir.join(CANONICAL_LOG_FILE_NAME);
    fs::create_dir_all(log_dir).map_err(|source| {
        AtmError::observability_bootstrap(format!(
            "failed to create retained log directory {}: {source}",
            log_dir.display(),
        ))
    })?;
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&active_log_path)
        .map(|_| active_log_path.clone())
        .map_err(|source| {
            AtmError::observability_bootstrap(format!(
                "failed to open retained log file {} during startup: {source}",
                active_log_path.display(),
            ))
        })
}

/// Returns the root consumed by `sc-observability` for a checked log directory.
pub fn logger_root_for_log_dir(log_dir: &Path) -> Result<PathBuf, AtmError> {
    log_dir.parent().map(Path::to_path_buf).ok_or_else(|| {
        AtmError::observability_bootstrap(format!(
            "failed to determine the retained-log root parent for {}",
            log_dir.display()
        ))
    })
}

/// Parses the process-start `ATM_LOG` override once for a concrete logger.
pub fn logger_level_override() -> Result<Option<sc_observability_types::LevelFilter>, AtmError> {
    let Some(raw_value) = std::env::var(ATM_LOG_LEVEL_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    match raw_value.to_ascii_lowercase().as_str() {
        "trace" => Ok(Some(sc_observability_types::LevelFilter::Trace)),
        "debug" => Ok(Some(sc_observability_types::LevelFilter::Debug)),
        "info" => Ok(Some(sc_observability_types::LevelFilter::Info)),
        "warn" => Ok(Some(sc_observability_types::LevelFilter::Warn)),
        "error" => Ok(Some(sc_observability_types::LevelFilter::Error)),
        "off" => Ok(Some(sc_observability_types::LevelFilter::Off)),
        _ => Err(AtmError::observability_bootstrap(format!(
            "invalid {ATM_LOG_LEVEL_ENV} value `{raw_value}`; use `trace`, `debug`, `info`, `warn`, `error`, or `off`"
        ))),
    }
}

/// Parses the test-only retained-sink health override.
pub fn retained_sink_fault_mode() -> Result<Option<RetainedSinkFaultMode>, AtmError> {
    let Some(value) = std::env::var(ATM_RETAINED_SINK_FAULT_ENV)
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
            "invalid {ATM_RETAINED_SINK_FAULT_ENV} value `{value}`; use `degraded` or `unavailable`"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use atm_core::test_support::EnvGuard;
    use tempfile::TempDir;

    use super::{ATM_LOG_LEVEL_ENV, logger_level_override, prepare_retained_log};

    #[test]
    fn prepares_the_active_log_file() {
        let tempdir = TempDir::new().expect("tempdir");
        let log_dir = tempdir.path().join("logs");

        let active_log_path = prepare_retained_log(&log_dir).expect("prepare retained log");

        assert_eq!(active_log_path, log_dir.join("atm.log.jsonl"));
        assert!(active_log_path.is_file());
    }

    #[test]
    fn rejects_invalid_log_levels_with_the_exact_override_name() {
        let _env = EnvGuard::set_many([(ATM_LOG_LEVEL_ENV, Some("loud"))]);

        let error = logger_level_override().expect_err("invalid level");

        assert!(error.is_observability_bootstrap());
        assert!(error.message().contains("invalid ATM_LOG value `loud`"));
    }
}
