use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{fs, fs::OpenOptions};

use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::home;
use atm_core::observability::{
    AtmLogQuery, AtmLogSnapshot, AtmMaintenanceHealthReport, AtmMaintenanceWorkerState,
    AtmObservabilityDiagnostic, AtmObservabilityHealth, AtmObservabilityHealthState,
    CommandEvent, LogTailSession, ObservabilityPort, RetainedSinkFaultMode, diagnostic_code,
};
use serde_json::Map;

use atm_daemon::DaemonSubsystem;
use atm_daemon::{DaemonEvent, TeamScope};
#[cfg(test)]
use sc_observability::{
    JsonlFileSink, LoggerBuilder, RetentionPolicy, RotationPolicy, SinkRegistration,
};
use sc_observability_types::DiagnosticInfo;

type ActionName = sc_observability_types::ActionName;
type CorrelationId = sc_observability_types::CorrelationId;
type Level = sc_observability_types::Level;
type SharedLevelFilter = sc_observability_types::LevelFilter;
type LogEvent = sc_observability_types::LogEvent;
type OutcomeLabel = sc_observability_types::OutcomeLabel;
type ProcessIdentity = sc_observability_types::ProcessIdentity;
type SchemaVersion = sc_observability_types::SchemaVersion;
type ServiceName = sc_observability_types::ServiceName;
type TargetCategory = sc_observability_types::TargetCategory;
type Timestamp = sc_observability_types::Timestamp;

const ATM_SERVICE_NAME: &str = "atm";
const ATM_DAEMON_TARGET: &str = "atm.daemon";
const ATM_LOG_LEVEL_ENV: &str = "ATM_LOG";
const RETAINED_LOG_ROTATION_MAX_BYTES: u64 = 10 * 1024 * 1024;
const RETAINED_LOG_ROTATION_MAX_FILES: usize = 5;
const RETAINED_LOG_RETENTION_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const RETAINED_LOG_MAINTENANCE_CADENCE: Duration = Duration::from_secs(60);
// Allow one bounded maintenance join during shutdown without turning routine
// daemon stop into a long blocking operation. This stays below the outer 2s
// graceful drain budget so retained-log shutdown cannot consume the entire
// daemon stop window by itself.
const RETAINED_LOG_WRITER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);
#[cfg(test)]
const ATM_OBSERVABILITY_RETAINED_SINK_FAULT_ENV: &str = "ATM_OBSERVABILITY_RETAINED_SINK_FAULT";

enum LoggerLifecycle {
    Running(sc_observability::Logger),
    Stopped(sc_observability::Logger<sc_observability::Stopped>),
}

impl LoggerLifecycle {
    fn health(&self) -> sc_observability_types::LoggingHealthReport {
        match self {
            Self::Running(logger) => logger.health(),
            Self::Stopped(logger) => logger.health(),
        }
    }
}

pub struct DaemonObservability {
    // Keep one shared logger lifecycle behind a mutex so emit/health paths and
    // shutdown can coordinate a single transition into the stopped state.
    logger: Arc<Mutex<Option<LoggerLifecycle>>>,
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
        Self::bootstrap_at_log_dir_with_rotation(log_dir, RETAINED_LOG_ROTATION_MAX_BYTES)
    }

    fn bootstrap_at_log_dir_with_rotation(
        log_dir: PathBuf,
        rotation_max_bytes: u64,
    ) -> Result<Self, AtmError> {
        let service_name = ServiceName::new(ATM_SERVICE_NAME).map_err(|_source| {
            AtmError::observability_bootstrap("failed to validate ATM daemon service name")

        })?;
        let target_category = TargetCategory::new(ATM_DAEMON_TARGET).map_err(|_source| {
            AtmError::observability_bootstrap("failed to validate ATM daemon observability target")

        })?;
        let retained_sink_fault = retained_sink_fault_mode()?;
        let (logger, active_log_path) = build_logger(
            &log_dir,
            retained_sink_fault,
            &service_name,
            retained_log_policy(rotation_max_bytes),
        )?;
        Ok(Self {
            logger: Arc::new(Mutex::new(Some(LoggerLifecycle::Running(logger)))),
            active_log_path,
            service_name,
            target_category,
        })
    }

    #[cfg(test)]
    fn bootstrap_at_log_dir_with_rotation_for_test(
        log_dir: PathBuf,
        rotation_max_bytes: u64,
    ) -> Result<Self, AtmError> {
        Self::bootstrap_at_log_dir_with_policy_for_test(
            log_dir,
            retained_log_policy(rotation_max_bytes),
        )
    }

    #[cfg(test)]
    fn bootstrap_at_log_dir_with_policy_for_test(
        log_dir: PathBuf,
        retained_log_policy: sc_observability::RetainedLogPolicy,
    ) -> Result<Self, AtmError> {
        let service_name = ServiceName::new(ATM_SERVICE_NAME).map_err(|_source| {
            AtmError::observability_bootstrap("failed to validate ATM daemon service name")

        })?;
        let target_category = TargetCategory::new(ATM_DAEMON_TARGET).map_err(|_source| {
            AtmError::observability_bootstrap("failed to validate ATM daemon observability target")

        })?;
        let (logger, active_log_path) = build_logger(&log_dir, None, &service_name, retained_log_policy)?;
        Ok(Self {
            logger: Arc::new(Mutex::new(Some(LoggerLifecycle::Running(logger)))),
            active_log_path,
            service_name,
            target_category,
        })
    }

    pub(crate) fn emit_daemon_event(&self, event: DaemonEvent) -> Result<(), AtmError> {
        let fields = daemon_event_fields(&event);
        self.emit_log_event(EmitLogEvent {
            scope: "lifecycle",
            action: event.action,
            outcome: event.outcome,
            message: Some(event.detail.into_owned()),
            request_id: validated_correlation_id(
                event.message_id.as_ref().map(ToString::to_string),
                "ATM daemon lifecycle request id",
            )?,
            correlation_id: validated_correlation_id(
                event.task_id.as_ref().map(|value| value.as_str().to_string()),
                "ATM daemon lifecycle correlation id",
            )?,
            fields,
        })
    }

    pub(crate) fn emit_subsystem_event(
        &self,
        subsystem: DaemonSubsystem,
        action: &ActionName,
        outcome: &OutcomeLabel,
        message: &str,
        error_code: Option<AtmErrorCode>,
    ) -> Result<(), AtmError> {
        let event = map_subsystem_event(
            &self.service_name,
            &self.target_category,
            subsystem,
            action,
            outcome,
            message,
            error_code,
        )?;
        let logger = self.logger.lock().map_err(|_| {
            AtmError::observability_emit(
                "shared daemon observability emit failed because the logger lock was poisoned",
            )
        })?;
        let Some(lifecycle) = logger.as_ref() else {
            return Err(AtmError::observability_emit(
                "shared daemon observability emit attempted after the retained logger shut down",
            ));
        };
        match lifecycle {
            LoggerLifecycle::Running(logger_runtime) => match logger_runtime.try_log(event) {
                Ok(()) => Ok(()),
                // A committed ATM admission must not wait behind retained-log
                // disk I/O. `try_log` records this drop in logger health, which
                // keeps the loss observable without allowing an overloaded
                // sink to become a synchronous write-path dependency.
                Err(sc_observability::TryLogError::QueueFull(_)) => Ok(()),
                Err(source) => {
                    let code = try_log_error_code(&source);
                    Err(AtmError::observability_emit(format!(
                        "shared daemon observability log admission failed ({code})"
                    )))
                }
            },
            LoggerLifecycle::Stopped(_) => Err(AtmError::observability_emit(
                "shared daemon observability emit attempted after the retained logger shut down",
            )),
        }
    }

    pub(crate) fn best_effort_flush_blocking(&self) -> Result<(), AtmError> {
        let lifecycle = {
            let mut logger = self.logger.lock().map_err(|_| {
                AtmError::observability_health(
                    "failed to finalize daemon observability because the logger lock was poisoned",
                )
            })?;
            logger.take()
        };
        let Some(lifecycle) = lifecycle else {
            return Ok(());
        };
        match lifecycle {
            LoggerLifecycle::Running(logger_runtime) => {
                // Remove the shared logger slot before the blocking
                // flush/shutdown sequence so concurrent emitters fail closed
                // instead of waiting on the same mutex during shutdown.
                let flush_result = logger_runtime.flush().map_err(|_source| {
                    AtmError::observability_health(
                        "failed to flush retained observability sink during daemon shutdown",
                    )

                });
                match flush_result {
                    Ok(()) => {
                        let stopped = logger_runtime.shutdown();
                        let mut logger = self.logger.lock().map_err(|_| {
                            AtmError::observability_health(
                                "failed to record daemon observability shutdown state because the logger lock was poisoned",
                            )
                        })?;
                        *logger = Some(LoggerLifecycle::Stopped(stopped));
                        Ok(())
                    }
                    Err(error) => {
                        let mut logger = self.logger.lock().map_err(|_| {
                            AtmError::observability_health(
                                "failed to restore daemon observability state after a flush error because the logger lock was poisoned",
                            )
                        })?;
                        *logger = Some(LoggerLifecycle::Running(logger_runtime));
                        Err(error)
                    }
                }
            }
            LoggerLifecycle::Stopped(logger_runtime) => {
                let mut logger = self.logger.lock().map_err(|_| {
                    AtmError::observability_health(
                        "failed to record daemon observability shutdown state because the logger lock was poisoned",
                    )
                })?;
                *logger = Some(LoggerLifecycle::Stopped(logger_runtime));
                Ok(())
            }
        }
    }

    pub(crate) fn best_effort_preflush_blocking(&self) -> Result<(), AtmError> {
        let lifecycle = {
            let mut logger = self.logger.lock().map_err(|_| {
                AtmError::observability_health(
                    "failed to preflush daemon observability because the logger lock was poisoned",
                )
            })?;
            logger.take()
        };
        let Some(lifecycle) = lifecycle else {
            return Ok(());
        };
        match lifecycle {
            LoggerLifecycle::Running(logger_runtime) => {
                // Concurrent emitters fail closed while this preflush runs so
                // the final shutdown event cannot be reordered behind older
                // retained-log queue entries during teardown.
                // Drain queued work before the final shutdown event is emitted,
                // but release the mutex first so the blocking flush path cannot
                // deadlock with a concurrent logger admission attempt.
                let flush_result = logger_runtime.flush().map_err(|_source| {
                    AtmError::observability_health(
                        "failed to preflush retained observability sink before daemon shutdown completion",
                    )

                });
                let mut logger = self.logger.lock().map_err(|_| {
                    AtmError::observability_health(
                        "failed to restore daemon observability state after preflush because the logger lock was poisoned",
                    )
                })?;
                *logger = Some(LoggerLifecycle::Running(logger_runtime));
                flush_result
            }
            LoggerLifecycle::Stopped(logger_runtime) => {
                let mut logger = self.logger.lock().map_err(|_| {
                    AtmError::observability_health(
                        "failed to restore daemon observability state after preflush because the logger lock was poisoned",
                    )
                })?;
                *logger = Some(LoggerLifecycle::Stopped(logger_runtime));
                Ok(())
            }
        }
    }
}

impl atm_core::boundary::sealed::Sealed for DaemonObservability {}

impl ObservabilityPort for DaemonObservability {
    fn emit(&self, event: CommandEvent) -> Result<(), AtmError> {
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

        self.emit_log_event(EmitLogEvent {
            scope: "observability",
            action: ActionName::new(event.action.as_str()).map_err(|_source| {
                AtmError::observability_emit("failed to validate ATM daemon command action")

            })?,
            outcome: OutcomeLabel::new(event.outcome.as_str()).map_err(|_source| {
                AtmError::observability_emit("failed to validate ATM daemon command outcome")

            })?,
            message: Some(format!(
                "ATM daemon handled {} with outcome {}",
                event.command, event.outcome
            )),
            request_id: event
                .message_id
                .map(|value| CorrelationId::new(value.to_string()))
                .transpose()
                .map_err(|_source| {
                    AtmError::observability_emit("failed to validate ATM daemon request id")

                })?,
            correlation_id: event
                .task_id
                .as_ref()
                .map(|value| CorrelationId::new(value.as_str().to_string()))
                .transpose()
                .map_err(|_source| {
                    AtmError::observability_emit("failed to validate ATM daemon correlation id")

                })?,
            fields,
        })
    }

    fn query(&self, _req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
        Err(
            AtmError::observability_query(
                "daemon retained-log query is unavailable from the exact-path daemon logger adapter",
            )
            ,
        )
    }

    fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
        Err(
            AtmError::observability_follow(
                "daemon retained-log follow is unavailable from the exact-path daemon logger adapter",
            )
            ,
        )
    }

    fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
        let logger = self.logger.lock().map_err(|_| {
            AtmError::observability_health(
                "failed to read daemon observability health because the logger lock was poisoned",
            )
        })?;
        let Some(lifecycle) = logger.as_ref() else {
            return Ok(AtmObservabilityHealth {
                active_log_path: Some(self.active_log_path.clone()),
                logging_state: AtmObservabilityHealthState::Unavailable,
                query_state: None,
                maintenance: None,
                diagnostic: Some(AtmObservabilityDiagnostic {
                    code: None,
                    message: "daemon observability logger has already shut down".to_string(),
                }),
                detail: Some(
                    "daemon observability logger has already shut down; health is unavailable"
                        .to_string(),
                ),
            });
        };
        let report = lifecycle.health();
        let diagnostic = report.last_error.clone().map(map_diagnostic_summary);
        let detail = build_observability_detail(&report, diagnostic.as_ref());
        Ok(AtmObservabilityHealth {
            active_log_path: Some(self.active_log_path.clone()),
            logging_state: map_logging_state(report.state),
            query_state: None,
            maintenance: report.maintenance.clone().map(map_maintenance_report),
            diagnostic,
            detail,
        })
    }
}

impl atm_daemon::DaemonRuntimeObservability for DaemonObservability {
    fn best_effort_preflush_blocking(&self) -> Result<(), AtmError> {
        Self::best_effort_preflush_blocking(self)
    }

    fn emit_daemon_event(&self, event: DaemonEvent) -> Result<(), AtmError> {
        Self::emit_daemon_event(self, event)
    }

    fn emit_subsystem_event(
        &self,
        subsystem: DaemonSubsystem,
        action: &ActionName,
        outcome: &OutcomeLabel,
        message: &str,
        error_code: Option<AtmErrorCode>,
    ) -> Result<(), AtmError> {
        Self::emit_subsystem_event(self, subsystem, action, outcome, message, error_code)
    }

    fn best_effort_flush_blocking(&self) -> Result<(), AtmError> {
        Self::best_effort_flush_blocking(self)
    }
}

// Keep validated correlation identifiers typed at this internal boundary so
// each emit path does not repeatedly round-trip through String parsing.
struct EmitLogEvent {
    scope: &'static str,
    action: ActionName,
    outcome: OutcomeLabel,
    message: Option<String>,
    request_id: Option<CorrelationId>,
    correlation_id: Option<CorrelationId>,
    fields: Map<String, serde_json::Value>,
}

impl DaemonObservability {
    fn emit_log_event(&self, event: EmitLogEvent) -> Result<(), AtmError> {
        let event = LogEvent {
            version: SchemaVersion::new(
                sc_observability_types::constants::OBSERVATION_ENVELOPE_VERSION,
            )
            .map_err(|_source| {
                AtmError::observability_emit(format!(
                    "failed to validate ATM daemon {} schema version",
                    event.scope
                ))

            })?,
            timestamp: Timestamp::now_utc(),
            level: level_for_outcome(event.scope, &event.action, event.outcome.as_str()),
            service: self.service_name.clone(),
            target: self.target_category.clone(),
            action: event.action,
            message: event.message,
            identity: ProcessIdentity::default(),
            trace: None,
            request_id: event.request_id,
            correlation_id: event.correlation_id,
            outcome: Some(event.outcome),
            diagnostic: None,
            state_transition: None,
            fields: event.fields,
        };
        let logger = self.logger.lock().map_err(|_| {
            AtmError::observability_emit(
                "shared daemon observability emit failed because the logger lock was poisoned",
            )
        })?;
        let Some(lifecycle) = logger.as_ref() else {
            return Err(AtmError::observability_emit(
                "shared daemon observability emit attempted after the retained logger shut down",
            ));
        };
        match lifecycle {
            LoggerLifecycle::Running(logger_runtime) => match logger_runtime.try_log(event) {
                Ok(()) | Err(sc_observability::TryLogError::QueueFull(_)) => Ok(()),
                Err(source) => {
                    let code = try_log_error_code(&source);
                    Err(AtmError::observability_emit(format!(
                        "shared daemon observability log admission failed ({code})"
                    )))
                }
            },
            LoggerLifecycle::Stopped(_) => Err(AtmError::observability_emit(
                "shared daemon observability emit attempted after the retained logger shut down",
            )),
        }
    }
}

fn try_log_error_code(source: &sc_observability::TryLogError) -> &str {
    match source {
        sc_observability::TryLogError::InvalidEvent(error) => error.diagnostic().code.as_str(),
        sc_observability::TryLogError::QueueFull(context)
        | sc_observability::TryLogError::WriterDegraded(context)
        | sc_observability::TryLogError::ShutdownTimedOut(context) => {
            context.diagnostic().code.as_str()
        }
    }
}

fn validated_correlation_id(
    value: Option<String>,
    label: &str,
) -> Result<Option<CorrelationId>, AtmError> {
    value.map(CorrelationId::new).transpose().map_err(|_source| {
        AtmError::observability_emit(format!("failed to validate {label}"))
    })
}

fn daemon_event_fields(event: &DaemonEvent) -> Map<String, serde_json::Value> {
    let mut fields = Map::from_iter([(
        "subsystem".to_string(),
        serde_json::Value::String(event.subsystem.as_str().to_string()),
    )]);
    match &event.team {
        TeamScope::Team(team) => {
            fields.insert("team".to_string(), serde_json::Value::String(team.to_string()));
        }
        TeamScope::None => {
            fields.insert(
                "team_scope".to_string(),
                serde_json::Value::String("none".to_string()),
            );
        }
    }
    for (key, value) in [
        ("agent", event.agent.as_ref().map(ToString::to_string)),
        ("sender", event.sender.as_ref().map(ToString::to_string)),
        ("recipient", event.recipient.as_ref().map(ToString::to_string)),
        ("message_id", event.message_id.as_ref().map(ToString::to_string)),
        ("task_id", event.task_id.as_ref().map(ToString::to_string)),
    ] {
        if let Some(value) = value {
            fields.insert(key.to_string(), serde_json::Value::String(value));
        }
    }
    if let Some(connection_failure) = &event.connection_failure {
        fields.insert(
            "connection_failure".to_string(),
            serde_json::to_value(connection_failure)
                .expect("daemon connection failure fields must serialize"),
        );
    }
    if let Some(context) = &event.transport_context {
        fields.insert(
            "transport_context".to_string(),
            serde_json::Value::String(context.to_string()),
        );
    }
    if let Ok(extra) = serde_json::to_value(&event.extra_fields)
        && let Some(extra_fields) = extra.as_object()
    {
        for (key, value) in extra_fields {
            fields.insert(key.clone(), value.clone());
        }
    }
    fields
}

fn build_logger(
    log_dir: &Path,
    retained_sink_fault: Option<RetainedSinkFaultMode>,
    service_name: &ServiceName,
    retained_log_policy: sc_observability::RetainedLogPolicy,
) -> Result<(sc_observability::Logger, PathBuf), AtmError> {
    let active_log_path = log_dir.join("atm.log.jsonl");
    ensure_retained_log_ready(log_dir, &active_log_path)?;
    let mut config = sc_observability::LoggerConfig::default_for(
        service_name.clone(),
        logger_root_for_log_dir(log_dir)?,
    );
    config.level = logger_level_override()?.unwrap_or(SharedLevelFilter::Info);
    config.retained_log_policy = retained_log_policy;
    config.enable_console_sink = false;
    let builder = sc_observability::Logger::builder(config).map_err(|_source| {
        AtmError::observability_bootstrap("failed to initialize shared daemon observability logger")

    })?;
    #[cfg(test)]
    let builder = if let Some(mode) = retained_sink_fault {
        register_retained_sink_fault(builder, log_dir, mode)
    } else {
        builder
    };
    #[cfg(not(test))]
    let _ = retained_sink_fault;
    Ok((builder.build(), active_log_path))
}

fn ensure_retained_log_ready(log_dir: &Path, active_log_path: &Path) -> Result<(), AtmError> {
    fs::create_dir_all(log_dir).map_err(|_source| {
        AtmError::observability_bootstrap(format!(
            "failed to create retained log directory {}",
            log_dir.display()
        ))

    })?;
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(active_log_path)
        .map(|_| ())
        .map_err(|_source| {
            AtmError::observability_bootstrap(format!(
                "failed to open retained log file {} during startup",
                active_log_path.display()
            ))

        })
}

fn logger_root_for_log_dir(log_dir: &Path) -> Result<PathBuf, AtmError> {
    log_dir.parent().map(Path::to_path_buf).ok_or_else(|| {
        AtmError::observability_bootstrap(format!(
            "failed to determine the retained-log root parent for {}",
            log_dir.display()
        ))
    })
}

fn retained_log_policy(rotation_max_bytes: u64) -> sc_observability::RetainedLogPolicy {
    sc_observability::RetainedLogPolicy {
        rotation_max_bytes: sc_observability::ByteCount::from_bytes(rotation_max_bytes),
        rotation_max_files: sc_observability_types::FileCount::from_usize(
            RETAINED_LOG_ROTATION_MAX_FILES,
        ),
        retention_max_age: sc_observability::RetentionMaxAge::from_duration(
            RETAINED_LOG_RETENTION_MAX_AGE,
        ),
        maintenance_cadence: sc_observability::MaintenanceCadence::new(
            RETAINED_LOG_MAINTENANCE_CADENCE,
        ),
        writer_shutdown_timeout: sc_observability::WriterShutdownTimeout::new(
            RETAINED_LOG_WRITER_SHUTDOWN_TIMEOUT,
        ),
        maintenance_max_work_per_pass: Some(RETAINED_LOG_ROTATION_MAX_FILES),
    }
}

fn build_observability_detail(
    report: &sc_observability_types::LoggingHealthReport,
    diagnostic: Option<&AtmObservabilityDiagnostic>,
) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(diagnostic) = diagnostic {
        parts.push(diagnostic.message.clone());
    }
    parts.push(format!(
        "writer_state={} queue_depth={} queue_capacity={} queue_high_water_mark={} queue_full_drops_total={}",
        writer_state_label(report.writer_state),
        report.queue_depth,
        report.queue_capacity,
        report.queue_high_water_mark,
        report.queue_full_drops_total,
    ));
    if let Some(maintenance) = &report.maintenance {
        let last_pass = maintenance
            .last_pass_at
            .map(|timestamp| timestamp.into_inner().to_string())
            .unwrap_or_else(|| "never".to_string());
        let mut summary = format!(
            "maintenance state={} rotated_files_total={} pruned_files_total={} last_pass_at={last_pass}",
            maintenance_state_label(maintenance.state),
            maintenance.rotated_files_total,
            maintenance.pruned_files_total,
        );
        if let Some(last_error) = &maintenance.last_error {
            summary.push_str(&format!(" maintenance_last_error={}", last_error.message));
        }
        parts.push(summary);
    }
    (!parts.is_empty()).then(|| parts.join(" | "))
}

fn writer_state_label(state: sc_observability_types::WriterState) -> &'static str {
    match state {
        sc_observability_types::WriterState::Running => "running",
        sc_observability_types::WriterState::Degraded => "degraded",
        sc_observability_types::WriterState::Stopped => "stopped",
    }
}

#[cfg(test)]
fn fault_injection_log_path(log_dir: &Path) -> PathBuf {
    log_dir.join("atm-fault-injection.log.jsonl")
}

#[cfg(test)]
fn register_retained_sink_fault(
    mut builder: LoggerBuilder,
    log_dir: &Path,
    mode: RetainedSinkFaultMode,
) -> LoggerBuilder {
    let sink = Arc::new(JsonlFileSink::new(
        fault_injection_log_path(log_dir),
        RotationPolicy::default(),
        RetentionPolicy::default(),
    ));
    builder.register_sink(SinkRegistration::new(Arc::new(
        RetainedSinkHealthOverride::new(sink, mode),
    )));
    builder
}

fn maintenance_state_label(state: sc_observability_types::MaintenanceWorkerState) -> &'static str {
    match state {
        sc_observability_types::MaintenanceWorkerState::Running => "running",
        sc_observability_types::MaintenanceWorkerState::Degraded => "degraded",
        sc_observability_types::MaintenanceWorkerState::Stopped => "stopped",
    }
}

#[cfg(test)]
fn observability_test_event(
    action: &'static str,
    outcome: &'static str,
    detail: impl Into<std::borrow::Cow<'static, str>>,
) -> DaemonEvent {
    DaemonEvent {
        subsystem: DaemonSubsystem::Composition,
        action: ActionName::new(action)
            .expect("daemon observability test actions must be valid ActionName literals"),
        outcome: OutcomeLabel::new(outcome)
            .expect("daemon observability test outcomes must be valid OutcomeLabel literals"),
        team: TeamScope::None,
        agent: None,
        sender: None,
        recipient: None,
        message_id: None,
        task_id: None,
        detail: detail.into(),
        connection_failure: None,
        transport_context: None,
        extra_fields: atm_core::observability::LogFieldMap::default(),
    }
}

fn logger_level_override() -> Result<Option<SharedLevelFilter>, AtmError> {
    // The daemon latches ATM_LOG during bootstrap; changing the environment
    // later does not reconfigure the live retained logger.
    let Some(raw_value) = std::env::var(ATM_LOG_LEVEL_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let normalized = raw_value.to_ascii_lowercase();

    match normalized.as_str() {
        "trace" => Ok(Some(SharedLevelFilter::Trace)),
        "debug" => Ok(Some(SharedLevelFilter::Debug)),
        "info" => Ok(Some(SharedLevelFilter::Info)),
        "warn" => Ok(Some(SharedLevelFilter::Warn)),
        "error" => Ok(Some(SharedLevelFilter::Error)),
        "off" => Ok(Some(SharedLevelFilter::Off)),
        _ => Err(AtmError::observability_bootstrap(format!(
            "invalid {ATM_LOG_LEVEL_ENV} value `{raw_value}`; use `trace`, `debug`, `info`, `warn`, `error`, or `off`"
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

fn map_subsystem_event(
    service_name: &ServiceName,
    target_category: &TargetCategory,
    subsystem: DaemonSubsystem,
    action: &ActionName,
    outcome: &OutcomeLabel,
    message: &str,
    error_code: Option<AtmErrorCode>,
) -> Result<LogEvent, AtmError> {
    let schema_version =
        SchemaVersion::new(sc_observability_types::constants::OBSERVATION_ENVELOPE_VERSION)
            .map_err(|_source| {
                AtmError::observability_emit(
                    "failed to validate ATM daemon subsystem-event schema version",
                )

            })?;
    let mut fields = Map::from_iter([(
        "component".to_string(),
        serde_json::Value::String(subsystem.as_str().to_string()),
    )]);
    if let Some(error_code) = error_code {
        fields.insert(
            "error_code".to_string(),
            serde_json::Value::String(error_code.to_string()),
        );
    }

    Ok(LogEvent {
        version: schema_version,
        timestamp: Timestamp::now_utc(),
        level: level_for_outcome(subsystem.as_str(), action, outcome.as_str()),
        service: service_name.clone(),
        target: target_category.clone(),
        action: action.clone(),
        message: Some(message.to_string()),
        identity: ProcessIdentity::default(),
        trace: None,
        request_id: None,
        correlation_id: None,
        outcome: Some(outcome.clone()),
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

fn map_maintenance_report(
    report: sc_observability_types::MaintenanceHealthReport,
) -> AtmMaintenanceHealthReport {
    AtmMaintenanceHealthReport {
        state: match report.state {
            sc_observability_types::MaintenanceWorkerState::Running => {
                AtmMaintenanceWorkerState::Running
            }
            sc_observability_types::MaintenanceWorkerState::Degraded => {
                AtmMaintenanceWorkerState::Degraded
            }
            sc_observability_types::MaintenanceWorkerState::Stopped => {
                AtmMaintenanceWorkerState::Stopped
            }
        },
        rotated_files_total: report.rotated_files_total.as_usize() as u64,
        pruned_files_total: report.pruned_files_total.as_usize() as u64,
        last_pass_at: report
            .last_pass_at
            .map(|timestamp| {
                chrono::DateTime::parse_from_rfc3339(&timestamp.to_string())
                    .map(|datetime| datetime.with_timezone(&chrono::Utc).into())
                    .map_err(|_source| {
                        AtmError::observability_query(
                            "shared maintenance timestamp could not be converted to chrono",
                        )

                    })
            })
            .transpose()
            .expect("shared maintenance timestamps must project into ATM timestamps"),
    }
}

fn map_diagnostic_summary(
    summary: sc_observability_types::DiagnosticSummary,
) -> AtmObservabilityDiagnostic {
    AtmObservabilityDiagnostic {
        code: summary.code.map(|code| {
            diagnostic_code(code.as_str().to_string())
                .expect("shared diagnostic codes must be non-empty")
        }),
        message: summary.message,
    }
}

fn level_for_outcome(subsystem: &str, action: &ActionName, outcome: &str) -> Level {
    if outcome.starts_with("delivery_policy.") {
        return Level::Debug;
    }

    match outcome {
        "ok" | "sent" | "dry_run" => Level::Info,
        "expected_peer_disconnect" => Level::Info,
        "timeout" => Level::Warn,
        "error" | "failed" | "malformed_request" | "transport_failure" | "request_failure"
        | "saturated" => Level::Error,
        other => {
            tracing::warn!(
                code = %AtmErrorCode::ObservabilityEmitFailed,
                service = ATM_SERVICE_NAME,
                target = ATM_DAEMON_TARGET,
                subsystem,
                action = action.as_str(),
                outcome = other,
                "unknown ATM daemon outcome for observability level"
            );
            Level::Warn
        }
    }
}

#[cfg(test)]
struct RetainedSinkHealthOverride {
    inner: Arc<dyn sc_observability::LogSink>,
    mode: RetainedSinkFaultMode,
}

#[cfg(test)]
impl RetainedSinkHealthOverride {
    fn new(inner: Arc<dyn sc_observability::LogSink>, mode: RetainedSinkFaultMode) -> Self {
        Self { inner, mode }
    }
}

#[cfg(test)]
impl sc_observability::LogSink for RetainedSinkHealthOverride {
    fn write(&self, _event: &LogEvent) -> Result<(), sc_observability_types::LogSinkError> {
        match self.mode {
            RetainedSinkFaultMode::Degraded => Err(sc_observability_types::LogSinkError(Box::new(
                sc_observability_types::ErrorContext::new(
                    sc_observability_types::ErrorCode::new_static("SC_TEST_DAEMON_SINK_DEGRADED"),
                    "daemon retained sink degraded by test fault injection",
                    sc_observability_types::Remediation::not_recoverable(
                        "clear the daemon retained sink fault injection mode before retrying",
                    ),
                ),
            ))),
            RetainedSinkFaultMode::Unavailable => Err(sc_observability_types::LogSinkError(
                Box::new(sc_observability_types::ErrorContext::new(
                    sc_observability_types::ErrorCode::new_static(
                        "SC_TEST_DAEMON_SINK_UNAVAILABLE",
                    ),
                    "daemon retained sink unavailable by test fault injection",
                    sc_observability_types::Remediation::not_recoverable(
                        "clear the daemon retained sink fault injection mode before retrying",
                    ),
                )),
            )),
        }
    }

    fn flush(&self) -> Result<(), sc_observability_types::LogSinkError> {
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
    use std::time::{Duration, Instant};

    use atm_core::observability::{
        AtmMaintenanceWorkerState, AtmObservabilityHealthState, ObservabilityPort,
    };
    use atm_core::test_support::EnvGuard;
    use serial_test::serial;
    use tempfile::TempDir;

    use super::{ActionName, DaemonObservability, observability_test_event};

    #[test]
    fn delivery_policy_outcomes_map_to_debug() {
        assert_eq!(
            super::level_for_outcome(
                "delivery_policy",
                &ActionName::new("test.action").expect("action"),
                "delivery_policy.new_message.primary_nudge",
            ),
            sc_observability_types::Level::Debug
        );
        assert_eq!(
            super::level_for_outcome(
                "delivery_policy",
                &ActionName::new("test.action").expect("action"),
                "delivery_policy.ack_reply.delivered",
            ),
            sc_observability_types::Level::Debug
        );
    }

    #[test]
    fn daemon_outcomes_map_to_documented_levels() {
        for (outcome, expected) in [
            ("ok", sc_observability_types::Level::Info),
            ("sent", sc_observability_types::Level::Info),
            ("dry_run", sc_observability_types::Level::Info),
            ("timeout", sc_observability_types::Level::Warn),
            ("error", sc_observability_types::Level::Error),
            ("failed", sc_observability_types::Level::Error),
            ("future-outcome", sc_observability_types::Level::Warn),
        ] {
            assert_eq!(
                super::level_for_outcome(
                    "daemon_test",
                    &ActionName::new("test.action").expect("action"),
                    outcome,
                ),
                expected,
                "outcome={outcome}"
            );
        }
    }

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
            .emit_daemon_event(observability_test_event(
                "start_requested",
                "ok",
                "daemon start requested",
            ))
            .expect("emit start");
        observability
            .emit_daemon_event(observability_test_event(
                "shutdown_completed",
                "ok",
                "daemon shutdown completed",
            ))
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
        assert_eq!(health.logging_state, AtmObservabilityHealthState::Unavailable);
        assert_eq!(
            health.maintenance.as_ref().map(|report| report.state),
            Some(AtmMaintenanceWorkerState::Stopped)
        );
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
        assert!(error.message().contains("absolute path"));
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
                .message()
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
        assert!(error.message().contains("atm.log.jsonl"));
        assert!(
            error
                .message()
                .contains(&blocked_log_dir.join("atm.log.jsonl").display().to_string())
        );
    }

    #[test]
    fn best_effort_flush_syncs_the_last_written_handle_without_reopening_the_active_path() {
        let tempdir = TempDir::new().expect("tempdir");
        let log_dir = tempdir.path().join("logs");
        std::fs::create_dir_all(&log_dir).expect("log dir");
        let observability =
            super::DaemonObservability::bootstrap_at_log_dir_with_rotation_for_test(
                log_dir.clone(),
                512,
            )
            .expect("bootstrap");
        observability
            .emit_daemon_event(observability_test_event(
                "start_requested",
                "ok",
                "daemon start requested",
            ))
            .expect("emit");

        let active_log_path = log_dir.join("atm.log.jsonl");
        let rotated_log_path = log_dir.join("atm.log.jsonl.1");
        std::fs::rename(&active_log_path, &rotated_log_path).expect("rotate active log path");
        std::fs::create_dir(&active_log_path).expect("replace active path with directory");

        observability
            .best_effort_flush_blocking()
            .expect("best effort flush");
    }

    #[test]
    fn retained_log_prune_runs_on_a_background_worker() {
        let tempdir = TempDir::new().expect("tempdir");
        let log_dir = tempdir.path().join("logs");
        std::fs::create_dir_all(&log_dir).expect("log dir");
        let active_log_path = log_dir.join("atm.log.jsonl");
        std::fs::write(&active_log_path, "active\n").expect("active log");
        let rotated_log_path = log_dir.join("atm.log.jsonl.1");
        std::fs::write(&rotated_log_path, "stale\n").expect("rotated log");

        let policy = sc_observability::RetainedLogPolicy {
            rotation_max_bytes: sc_observability::ByteCount::from_bytes(1024),
            rotation_max_files: sc_observability_types::FileCount::from_usize(5),
            retention_max_age: sc_observability::RetentionMaxAge::from_duration(Duration::from_millis(1)),
            maintenance_cadence: sc_observability::MaintenanceCadence::new(Duration::from_millis(10)),
            writer_shutdown_timeout: sc_observability::WriterShutdownTimeout::new(Duration::from_secs(1)),
            maintenance_max_work_per_pass: Some(5),
        };
        let observability =
            super::DaemonObservability::bootstrap_at_log_dir_with_policy_for_test(log_dir, policy)
                .expect("bootstrap");

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut health = observability.health().expect("initial health");
        while Instant::now() < deadline
            && (rotated_log_path.exists()
                || health
                    .maintenance
                    .as_ref()
                    .is_none_or(|report| report.pruned_files_total < 1))
        {
            health = observability.health().expect("health during prune wait");
            std::thread::yield_now();
        }

        assert!(
            !rotated_log_path.exists(),
            "background prune worker should remove expired rotated files"
        );

        assert!(
            health
                .maintenance
                .as_ref()
                .is_some_and(|report| report.pruned_files_total >= 1),
            "maintenance stats should record the background prune pass"
        );
    }
}
