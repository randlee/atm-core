use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{fs, fs::OpenOptions};

use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::home;
use atm_core::observability::{
    AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState, CommandEvent,
    LogTailSession, ObservabilityPort, RetainedSinkFaultMode,
};
use sc_observability::{
    JsonlFileSink, LogSink, Logger, LoggerConfig, RetentionPolicy, RotationPolicy, SinkRegistration,
};
#[cfg(test)]
use sc_observability_types::LogSinkError;
use sc_observability_types::{
    ActionName, CorrelationId, DiagnosticInfo, Level, LevelFilter as SharedLevelFilter, LogEvent,
    OutcomeLabel, ProcessIdentity, SchemaVersion, ServiceName, TargetCategory, Timestamp,
};
use serde_json::Map;

const ATM_SERVICE_NAME: &str = "atm";
const ATM_DAEMON_TARGET: &str = "atm.daemon";
const ATM_LOG_LEVEL_ENV: &str = "ATM_LOG";
const RETAINED_LOG_ROTATION_MAX_BYTES: u64 = 10 * 1024 * 1024;
const RETAINED_LOG_ROTATION_MAX_FILES: u32 = 5;
const RETAINED_LOG_RETENTION_MAX_AGE_DAYS: u32 = 7;
#[cfg(test)]
const ATM_OBSERVABILITY_RETAINED_SINK_FAULT_ENV: &str = "ATM_OBSERVABILITY_RETAINED_SINK_FAULT";

pub struct DaemonObservability {
    logger: Arc<Logger>,
    active_log_path: PathBuf,
    service_name: ServiceName,
    target_category: TargetCategory,
}

impl std::fmt::Debug for DaemonObservability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonObservability")
            .field("active_log_path", &self.active_log_path)
            .field("service_name", &self.service_name)
            .field("target_category", &self.target_category)
            .finish_non_exhaustive()
    }
}

impl Clone for DaemonObservability {
    fn clone(&self) -> Self {
        Self {
            logger: Arc::clone(&self.logger),
            active_log_path: self.active_log_path.clone(),
            service_name: self.service_name.clone(),
            target_category: self.target_category.clone(),
        }
    }
}

impl DaemonObservability {
    pub(crate) fn bootstrap() -> Result<Self, AtmError> {
        Self::bootstrap_at_log_dir(home::host_log_dir()?)
    }

    fn bootstrap_at_log_dir(log_dir: PathBuf) -> Result<Self, AtmError> {
        let service_name = ServiceName::new(ATM_SERVICE_NAME).map_err(|source| {
            AtmError::observability_bootstrap("failed to validate ATM daemon service name")
                .with_source(source)
        })?;
        let target_category = TargetCategory::new(ATM_DAEMON_TARGET).map_err(|source| {
            AtmError::observability_bootstrap("failed to validate ATM daemon observability target")
                .with_source(source)
        })?;
        let retained_sink_fault = retained_sink_fault_mode()?;
        let (logger, active_log_path) = build_logger(&log_dir, retained_sink_fault, &service_name)?;
        Ok(Self {
            logger: Arc::new(logger),
            active_log_path,
            service_name,
            target_category,
        })
    }

    pub(crate) fn emit_runtime_event(
        &self,
        action: &'static str,
        outcome: &'static str,
        message: &'static str,
    ) -> Result<(), AtmError> {
        let event = map_runtime_event(
            &self.service_name,
            &self.target_category,
            action,
            outcome,
            message,
        )?;
        self.logger.emit(event).map_err(|source| {
            let code = source.diagnostic().code.as_str().to_string();
            AtmError::observability_emit(format!(
                "shared daemon observability emit failed ({code})"
            ))
            .with_source(source)
        })
    }

    pub(crate) fn best_effort_flush_blocking(&self) -> Result<(), AtmError> {
        self.logger.flush().map_err(|source| {
            AtmError::observability_health(
                "failed to flush retained observability sink during daemon shutdown",
            )
            .with_source(source)
        })?;
        let file = match OpenOptions::new().append(true).open(&self.active_log_path) {
            Ok(file) => file,
            Err(source) if source.kind() == ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(AtmError::observability_health(format!(
                    "failed to open retained observability sink at {} for best-effort flush",
                    self.active_log_path.display()
                ))
                .with_source(source));
            }
        };
        file.sync_all().map_err(|source| {
            AtmError::observability_health(format!(
                "failed to sync retained observability sink at {}",
                self.active_log_path.display()
            ))
            .with_source(source)
        })
    }
}

impl atm_core::boundary::sealed::Sealed for DaemonObservability {}

impl ObservabilityPort for DaemonObservability {
    fn emit(&self, event: CommandEvent) -> Result<(), AtmError> {
        let event = map_command_event(&self.service_name, &self.target_category, event)?;
        self.logger.emit(event).map_err(|source| {
            let code = source.diagnostic().code.as_str().to_string();
            AtmError::observability_emit(format!(
                "shared daemon observability emit failed ({code})"
            ))
            .with_source(source)
        })
    }

    fn query(&self, _req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
        Err(
            AtmError::observability_query(
                "daemon retained-log query is unavailable from the exact-path daemon logger adapter",
            )
            .with_recovery(
                "Use the CLI-owned retained log query surface for historical log reads until daemon query support is explicitly extracted.",
            ),
        )
    }

    fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
        Err(
            AtmError::observability_follow(
                "daemon retained-log follow is unavailable from the exact-path daemon logger adapter",
            )
            .with_recovery(
                "Use the CLI-owned retained log follow surface until daemon follow support is explicitly extracted.",
            ),
        )
    }

    fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
        let report = self.logger.health();
        Ok(AtmObservabilityHealth {
            active_log_path: Some(self.active_log_path.clone()),
            logging_state: map_logging_state(report.state),
            query_state: None,
            detail: report.last_error.map(render_diagnostic_summary),
        })
    }
}

impl atm_daemon::DaemonRuntimeObservability for DaemonObservability {
    fn emit_runtime_event(
        &self,
        action: &'static str,
        outcome: &'static str,
        message: &'static str,
    ) -> Result<(), AtmError> {
        Self::emit_runtime_event(self, action, outcome, message)
    }

    fn best_effort_flush_blocking(&self) -> Result<(), AtmError> {
        Self::best_effort_flush_blocking(self)
    }
}

fn build_logger(
    log_dir: &Path,
    retained_sink_fault: Option<RetainedSinkFaultMode>,
    service_name: &ServiceName,
) -> Result<(Logger, PathBuf), AtmError> {
    let active_log_path = log_dir.join("atm.log.jsonl");
    ensure_retained_log_ready(log_dir, &active_log_path)?;
    let mut config = LoggerConfig::default_for(service_name.clone(), PathBuf::new());
    config.level = logger_level_override()?.unwrap_or(SharedLevelFilter::Info);
    config.enable_console_sink = false;
    config.enable_file_sink = false;
    let mut builder = Logger::builder(config).map_err(|source| {
        AtmError::observability_bootstrap("failed to initialize shared daemon observability logger")
            .with_source(source)
    })?;
    // sc-observability 1.0.0 JsonlFileSink performs synchronous local-file writes.
    // atm-daemon satisfies ADR-011's non-blocking-executor rule by never calling
    // this sink from an async executor thread; retained writes stay on ordinary
    // daemon OS threads, and shutdown flush runs on a dedicated finalizer thread.
    let sink = Arc::new(JsonlFileSink::new(
        active_log_path.clone(),
        RotationPolicy {
            max_bytes: RETAINED_LOG_ROTATION_MAX_BYTES,
            max_files: RETAINED_LOG_ROTATION_MAX_FILES,
        },
        RetentionPolicy {
            max_age_days: RETAINED_LOG_RETENTION_MAX_AGE_DAYS,
        },
    ));
    #[cfg(test)]
    let sink: Arc<dyn LogSink> = match retained_sink_fault {
        Some(mode) => Arc::new(RetainedSinkHealthOverride::new(sink, mode)),
        None => sink,
    };
    #[cfg(not(test))]
    let sink: Arc<dyn LogSink> = {
        let _ = retained_sink_fault;
        sink
    };
    builder.register_sink(SinkRegistration::new(sink));
    Ok((builder.build(), active_log_path))
}

fn ensure_retained_log_ready(log_dir: &Path, active_log_path: &Path) -> Result<(), AtmError> {
    fs::create_dir_all(log_dir).map_err(|source| {
        AtmError::observability_bootstrap(format!(
            "failed to create retained log directory {}",
            log_dir.display()
        ))
        .with_source(source)
    })?;
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(active_log_path)
        .map(|_| ())
        .map_err(|source| {
            AtmError::observability_bootstrap(format!(
                "failed to open retained log file {} during startup",
                active_log_path.display()
            ))
            .with_source(source)
        })
}

fn logger_level_override() -> Result<Option<SharedLevelFilter>, AtmError> {
    let Some(value) = std::env::var(ATM_LOG_LEVEL_ENV)
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    match value.as_str() {
        "trace" => Ok(Some(SharedLevelFilter::Trace)),
        "debug" => Ok(Some(SharedLevelFilter::Debug)),
        "info" => Ok(Some(SharedLevelFilter::Info)),
        "warn" => Ok(Some(SharedLevelFilter::Warn)),
        "error" => Ok(Some(SharedLevelFilter::Error)),
        "off" => Ok(Some(SharedLevelFilter::Off)),
        _ => Err(AtmError::observability_bootstrap(format!(
            "invalid {ATM_LOG_LEVEL_ENV} value `{value}`; use `trace`, `debug`, `info`, `warn`, `error`, or `off`"
        ))),
    }
}

fn retained_sink_fault_mode() -> Result<Option<RetainedSinkFaultMode>, AtmError> {
    #[cfg(not(test))]
    {
        Ok(None)
    }
    #[cfg(test)]
    {
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
}

fn map_command_event(
    service_name: &ServiceName,
    target_category: &TargetCategory,
    event: CommandEvent,
) -> Result<LogEvent, AtmError> {
    let schema_version =
        SchemaVersion::new(sc_observability_types::constants::OBSERVATION_ENVELOPE_VERSION)
            .map_err(|source| {
                AtmError::observability_emit(
                    "failed to validate ATM daemon observability schema version",
                )
                .with_source(source)
            })?;
    let action = ActionName::new(event.action).map_err(|source| {
        AtmError::observability_emit("failed to validate ATM daemon observability action")
            .with_source(source)
    })?;
    let request_id = event
        .message_id
        .map(|value| CorrelationId::new(value.to_string()))
        .transpose()
        .map_err(|source| {
            AtmError::observability_emit("failed to validate ATM daemon request id")
                .with_source(source)
        })?;
    let correlation_id = event
        .task_id
        .as_deref()
        .map(CorrelationId::new)
        .transpose()
        .map_err(|source| {
            AtmError::observability_emit("failed to validate ATM daemon correlation id")
                .with_source(source)
        })?;
    let outcome = OutcomeLabel::new(event.outcome).map_err(|source| {
        AtmError::observability_emit("failed to validate ATM daemon outcome label")
            .with_source(source)
    })?;

    let mut fields = Map::new();
    fields.insert(
        "command".to_string(),
        serde_json::Value::String(event.command.to_string()),
    );
    fields.insert(
        "team".to_string(),
        serde_json::Value::String(event.team.to_string()),
    );
    fields.insert(
        "agent".to_string(),
        serde_json::Value::String(event.agent.to_string()),
    );
    fields.insert(
        "sender".to_string(),
        serde_json::Value::String(event.sender.to_string()),
    );
    fields.insert(
        "requires_ack".to_string(),
        serde_json::Value::Bool(event.requires_ack),
    );
    fields.insert(
        "dry_run".to_string(),
        serde_json::Value::Bool(event.dry_run),
    );
    if let Some(message_id) = event.message_id {
        fields.insert(
            "message_id".to_string(),
            serde_json::Value::String(message_id.to_string()),
        );
    }
    if let Some(task_id) = &event.task_id {
        fields.insert(
            "task_id".to_string(),
            serde_json::Value::String(task_id.to_string()),
        );
    }
    if let Some(error_code) = event.error_code {
        fields.insert(
            "error_code".to_string(),
            serde_json::Value::String(error_code.to_string()),
        );
    }
    if let Some(error_message) = &event.error_message {
        fields.insert(
            "error_message".to_string(),
            serde_json::Value::String(error_message.clone()),
        );
    }

    Ok(LogEvent {
        version: schema_version,
        timestamp: Timestamp::now_utc(),
        level: level_for_outcome(event.outcome),
        service: service_name.clone(),
        target: target_category.clone(),
        action,
        message: Some(format!(
            "ATM daemon handled {} with outcome {}",
            event.command, event.outcome
        )),
        identity: ProcessIdentity::default(),
        trace: None,
        request_id,
        correlation_id,
        outcome: Some(outcome),
        diagnostic: None,
        state_transition: None,
        fields,
    })
}

fn map_runtime_event(
    service_name: &ServiceName,
    target_category: &TargetCategory,
    action: &'static str,
    outcome: &'static str,
    message: &'static str,
) -> Result<LogEvent, AtmError> {
    let schema_version =
        SchemaVersion::new(sc_observability_types::constants::OBSERVATION_ENVELOPE_VERSION)
            .map_err(|source| {
                AtmError::observability_emit(
                    "failed to validate ATM daemon lifecycle schema version",
                )
                .with_source(source)
            })?;
    let action = ActionName::new(action).map_err(|source| {
        AtmError::observability_emit("failed to validate ATM daemon lifecycle action")
            .with_source(source)
    })?;
    let outcome = OutcomeLabel::new(outcome).map_err(|source| {
        AtmError::observability_emit("failed to validate ATM daemon lifecycle outcome")
            .with_source(source)
    })?;
    let fields = Map::from_iter([(
        "component".to_string(),
        serde_json::Value::String("daemon_runtime".to_string()),
    )]);

    Ok(LogEvent {
        version: schema_version,
        timestamp: Timestamp::now_utc(),
        level: level_for_outcome(outcome.as_str()),
        service: service_name.clone(),
        target: target_category.clone(),
        action,
        message: Some(message.to_string()),
        identity: ProcessIdentity::default(),
        trace: None,
        request_id: None,
        correlation_id: None,
        outcome: Some(outcome),
        diagnostic: None,
        state_transition: None,
        fields,
    })
}

fn map_logging_state(
    state: sc_observability_types::LoggingHealthState,
) -> AtmObservabilityHealthState {
    match state {
        sc_observability_types::LoggingHealthState::Healthy => AtmObservabilityHealthState::Healthy,
        sc_observability_types::LoggingHealthState::DegradedDropping => {
            AtmObservabilityHealthState::Degraded
        }
        sc_observability_types::LoggingHealthState::Unavailable => {
            AtmObservabilityHealthState::Unavailable
        }
    }
}

fn render_diagnostic_summary(summary: sc_observability_types::DiagnosticSummary) -> String {
    match summary.code {
        Some(code) => format!("{}: {}", code.as_str(), summary.message),
        None => summary.message,
    }
}

fn level_for_outcome(outcome: &str) -> Level {
    match outcome {
        "ok" | "sent" | "dry_run" => Level::Info,
        "timeout" => Level::Warn,
        "error" | "failed" => Level::Error,
        other => {
            tracing::warn!(
                code = %AtmErrorCode::ObservabilityEmitFailed,
                outcome = other,
                "unknown ATM daemon outcome for observability level"
            );
            Level::Warn
        }
    }
}

#[cfg(test)]
struct RetainedSinkHealthOverride {
    inner: Arc<dyn LogSink>,
    mode: RetainedSinkFaultMode,
}

#[cfg(test)]
impl RetainedSinkHealthOverride {
    fn new(inner: Arc<dyn LogSink>, mode: RetainedSinkFaultMode) -> Self {
        Self { inner, mode }
    }
}

#[cfg(test)]
impl LogSink for RetainedSinkHealthOverride {
    fn write(&self, _event: &LogEvent) -> Result<(), LogSinkError> {
        match self.mode {
            RetainedSinkFaultMode::Degraded => Err(LogSinkError(Box::new(
                sc_observability_types::ErrorContext::new(
                    sc_observability_types::ErrorCode::new_static("SC_TEST_DAEMON_SINK_DEGRADED"),
                    "daemon retained sink degraded by test fault injection",
                    sc_observability_types::Remediation::not_recoverable(
                        "clear the daemon retained sink fault injection mode before retrying",
                    ),
                ),
            ))),
            RetainedSinkFaultMode::Unavailable => Err(LogSinkError(Box::new(
                sc_observability_types::ErrorContext::new(
                    sc_observability_types::ErrorCode::new_static(
                        "SC_TEST_DAEMON_SINK_UNAVAILABLE",
                    ),
                    "daemon retained sink unavailable by test fault injection",
                    sc_observability_types::Remediation::not_recoverable(
                        "clear the daemon retained sink fault injection mode before retrying",
                    ),
                ),
            ))),
        }
    }

    fn flush(&self) -> Result<(), LogSinkError> {
        self.inner.flush()
    }

    fn health(&self) -> sc_observability_types::SinkHealth {
        let mut health = self.inner.health();
        health.state = match self.mode {
            RetainedSinkFaultMode::Degraded => {
                sc_observability_types::SinkHealthState::DegradedDropping
            }
            RetainedSinkFaultMode::Unavailable => {
                sc_observability_types::SinkHealthState::Unavailable
            }
        };
        health
    }
}

#[cfg(test)]
mod tests {
    use atm_core::observability::{AtmObservabilityHealthState, ObservabilityPort};
    use atm_core::test_support::EnvGuard;
    use serial_test::serial;
    use tempfile::TempDir;

    use super::DaemonObservability;

    #[test]
    #[serial]
    fn bootstrap_writes_lifecycle_events_to_host_scoped_retained_log() {
        let tempdir = TempDir::new().expect("tempdir");
        let _env = EnvGuard::set_many([
            ("ATM_LOG", Some("info")),
            ("ATM_LOG_DIR", None),
            ("HOME", Some(tempdir.path().to_str().expect("utf8 path"))),
            ("USERPROFILE", None),
            ("ATM_OBSERVABILITY_RETAINED_SINK_FAULT", None),
        ]);

        let observability = DaemonObservability::bootstrap().expect("bootstrap");
        observability
            .emit_runtime_event("start_requested", "ok", "daemon start requested")
            .expect("emit start");
        observability
            .emit_runtime_event("shutdown_completed", "ok", "daemon shutdown completed")
            .expect("emit shutdown");
        observability
            .best_effort_flush_blocking()
            .expect("best effort flush");

        let active_log_path = tempdir
            .path()
            .join(".atm")
            .join("logs")
            .join("atm.log.jsonl");
        let output = std::fs::read_to_string(&active_log_path).expect("retained log");
        assert!(output.contains("daemon start requested"));
        assert!(output.contains("daemon shutdown completed"));

        let health = observability.health().expect("health");
        assert_eq!(health.active_log_path, Some(active_log_path));
        assert_eq!(health.logging_state, AtmObservabilityHealthState::Healthy);
    }

    #[test]
    #[serial]
    fn bootstrap_fails_closed_when_atm_log_dir_is_invalid() {
        let tempdir = TempDir::new().expect("tempdir");
        let _env = EnvGuard::set_many([
            ("ATM_LOG_DIR", Some("relative/logs")),
            ("HOME", Some(tempdir.path().to_str().expect("utf8 path"))),
            ("USERPROFILE", None),
            ("ATM_OBSERVABILITY_RETAINED_SINK_FAULT", None),
        ]);

        let error = DaemonObservability::bootstrap().expect_err("invalid ATM_LOG_DIR");
        assert!(error.is_config());
        assert!(error.message.contains("absolute path"));
    }

    #[test]
    #[serial]
    fn bootstrap_fails_closed_when_retained_log_dir_cannot_be_created() {
        let tempdir = TempDir::new().expect("tempdir");
        let blocked_parent = tempdir.path().join("blocked-parent");
        std::fs::write(&blocked_parent, "not a directory").expect("blocked parent");
        let blocked_log_dir = blocked_parent.join("logs");
        let _env = EnvGuard::set_many([
            (
                "ATM_LOG_DIR",
                Some(blocked_log_dir.to_str().expect("utf8 blocked log dir")),
            ),
            ("HOME", Some(tempdir.path().to_str().expect("utf8 path"))),
            ("USERPROFILE", None),
            ("ATM_OBSERVABILITY_RETAINED_SINK_FAULT", None),
        ]);

        let error =
            DaemonObservability::bootstrap().expect_err("retained log dir create should fail");
        assert!(error.is_observability_bootstrap());
        assert!(
            error
                .message
                .contains(&blocked_log_dir.display().to_string())
        );
    }

    #[test]
    #[serial]
    fn bootstrap_fails_closed_when_retained_log_file_is_not_appendable() {
        let tempdir = TempDir::new().expect("tempdir");
        let blocked_log_dir = tempdir.path().join("logs");
        std::fs::create_dir_all(&blocked_log_dir).expect("blocked log dir");
        std::fs::create_dir(blocked_log_dir.join("atm.log.jsonl"))
            .expect("non-appendable log path");
        let _env = EnvGuard::set_many([
            (
                "ATM_LOG_DIR",
                Some(blocked_log_dir.to_str().expect("utf8 blocked log dir")),
            ),
            ("HOME", Some(tempdir.path().to_str().expect("utf8 path"))),
            ("USERPROFILE", None),
            ("ATM_OBSERVABILITY_RETAINED_SINK_FAULT", None),
        ]);

        let error =
            DaemonObservability::bootstrap().expect_err("retained log file open should fail");
        assert!(error.is_observability_bootstrap());
        assert!(error.message.contains("atm.log.jsonl"));
        assert!(
            error
                .message
                .contains(&blocked_log_dir.join("atm.log.jsonl").display().to_string())
        );
    }
}
