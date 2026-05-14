use std::io::ErrorKind;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};
use std::{fs, fs::OpenOptions};

use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::home;
use atm_core::observability::{
    AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState, CommandEvent,
    LogTailSession, ObservabilityPort, RetainedSinkFaultMode,
};
use serde_json::Map;

use atm_daemon::DaemonSubsystem;
use atm_daemon::{DaemonEvent, TeamScope};

type ActionName = sc_observability_types::ActionName;
type CorrelationId = sc_observability_types::CorrelationId;
type DiagnosticSummary = sc_observability_types::DiagnosticSummary;
type ErrorCode = sc_observability_types::ErrorCode;
type ErrorContext = sc_observability_types::ErrorContext;
type Level = sc_observability_types::Level;
type SharedLevelFilter = sc_observability_types::LevelFilter;
type LogEvent = sc_observability_types::LogEvent;
type OutcomeLabel = sc_observability_types::OutcomeLabel;
type ProcessIdentity = sc_observability_types::ProcessIdentity;
type Remediation = sc_observability_types::Remediation;
type SchemaVersion = sc_observability_types::SchemaVersion;
type ServiceName = sc_observability_types::ServiceName;
type SinkHealth = sc_observability_types::SinkHealth;
type SinkHealthState = sc_observability_types::SinkHealthState;
type SinkName = sc_observability_types::SinkName;
type TargetCategory = sc_observability_types::TargetCategory;
type Timestamp = sc_observability_types::Timestamp;

const ATM_SERVICE_NAME: &str = "atm";
const ATM_DAEMON_TARGET: &str = "atm.daemon";
const ATM_LOG_LEVEL_ENV: &str = "ATM_LOG";
const RETAINED_LOG_ROTATION_MAX_BYTES: u64 = 10 * 1024 * 1024;
const RETAINED_LOG_ROTATION_MAX_FILES: u32 = 5;
const RETAINED_LOG_RETENTION_MAX_AGE_DAYS: u32 = 7;
const RETAINED_LOG_PRUNE_INTERVAL: Duration = Duration::from_secs(60);
#[cfg(test)]
const ATM_OBSERVABILITY_RETAINED_SINK_FAULT_ENV: &str = "ATM_OBSERVABILITY_RETAINED_SINK_FAULT";

pub struct DaemonObservability {
    logger: Arc<sc_observability::Logger>,
    active_log_path: PathBuf,
    retained_sink: Arc<RetainedJsonlFileSink>,
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
            retained_sink: Arc::clone(&self.retained_sink),
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
        let service_name = ServiceName::new(ATM_SERVICE_NAME).map_err(|source| {
            AtmError::observability_bootstrap("failed to validate ATM daemon service name")
                .with_source(source)
        })?;
        let target_category = TargetCategory::new(ATM_DAEMON_TARGET).map_err(|source| {
            AtmError::observability_bootstrap("failed to validate ATM daemon observability target")
                .with_source(source)
        })?;
        let retained_sink_fault = retained_sink_fault_mode()?;
        let (logger, active_log_path, retained_sink) = build_logger(
            &log_dir,
            retained_sink_fault,
            &service_name,
            rotation_max_bytes,
        )?;
        Ok(Self {
            logger: Arc::new(logger),
            active_log_path,
            retained_sink,
            service_name,
            target_category,
        })
    }

    #[cfg(test)]
    fn bootstrap_at_log_dir_with_rotation_for_test(
        log_dir: PathBuf,
        rotation_max_bytes: u64,
    ) -> Result<Self, AtmError> {
        Self::bootstrap_at_log_dir_with_rotation(log_dir, rotation_max_bytes)
    }

    pub(crate) fn emit_daemon_event(&self, event: DaemonEvent) -> Result<(), AtmError> {
        let mut fields = Map::new();
        fields.insert(
            "subsystem".to_string(),
            serde_json::Value::String(event.subsystem.as_str().to_string()),
        );
        match &event.team {
            TeamScope::Team(team) => {
                fields.insert(
                    "team".to_string(),
                    serde_json::Value::String(team.to_string()),
                );
            }
            TeamScope::None => {
                fields.insert(
                    "team_scope".to_string(),
                    serde_json::Value::String("none".to_string()),
                );
            }
        }
        if let Some(agent) = event.agent.as_ref() {
            fields.insert(
                "agent".to_string(),
                serde_json::Value::String(agent.to_string()),
            );
        }
        if let Some(sender) = event.sender.as_ref() {
            fields.insert(
                "sender".to_string(),
                serde_json::Value::String(sender.to_string()),
            );
        }
        if let Some(recipient) = event.recipient.as_ref() {
            fields.insert(
                "recipient".to_string(),
                serde_json::Value::String(recipient.to_string()),
            );
        }
        if let Some(message_id) = event.message_id.as_ref() {
            fields.insert(
                "message_id".to_string(),
                serde_json::Value::String(message_id.to_string()),
            );
        }
        if let Some(task_id) = event.task_id.as_ref() {
            fields.insert(
                "task_id".to_string(),
                serde_json::Value::String(task_id.to_string()),
            );
        }

        self.emit_log_event(EmitLogEvent {
            scope: "lifecycle",
            action: event.action,
            outcome: event.outcome,
            message: Some(event.detail.into_owned()),
            request_id: event.message_id.as_ref().map(|value| value.to_string()),
            correlation_id: event
                .task_id
                .as_ref()
                .map(|value| value.as_str().to_string()),
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
        self.logger.emit(event).map_err(|source| {
            let code = sc_observability_types::DiagnosticInfo::diagnostic(&source)
                .code
                .as_str()
                .to_string();
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
        self.retained_sink
            .sync_last_written_file()
            .map_err(|source| {
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
            action: ActionName::new(event.action).map_err(|source| {
                AtmError::observability_emit("failed to validate ATM daemon observability action")
                    .with_source(source)
            })?,
            outcome: OutcomeLabel::new(event.outcome).map_err(|source| {
                AtmError::observability_emit("failed to validate ATM daemon observability outcome")
                    .with_source(source)
            })?,
            message: Some(format!(
                "ATM daemon handled {} with outcome {}",
                event.command, event.outcome
            )),
            request_id: event.message_id.map(|value| value.to_string()),
            correlation_id: event
                .task_id
                .as_ref()
                .map(|value| value.as_str().to_string()),
            fields,
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

struct EmitLogEvent {
    scope: &'static str,
    action: ActionName,
    outcome: OutcomeLabel,
    message: Option<String>,
    request_id: Option<String>,
    correlation_id: Option<String>,
    fields: Map<String, serde_json::Value>,
}

impl DaemonObservability {
    fn emit_log_event(&self, event: EmitLogEvent) -> Result<(), AtmError> {
        let event = LogEvent {
            version: SchemaVersion::new(
                sc_observability_types::constants::OBSERVATION_ENVELOPE_VERSION,
            )
            .map_err(|source| {
                AtmError::observability_emit(format!(
                    "failed to validate ATM daemon {} schema version",
                    event.scope
                ))
                .with_source(source)
            })?,
            timestamp: Timestamp::now_utc(),
            level: level_for_outcome(event.outcome.as_str()),
            service: self.service_name.clone(),
            target: self.target_category.clone(),
            action: event.action,
            message: event.message,
            identity: ProcessIdentity::default(),
            trace: None,
            request_id: event
                .request_id
                .as_deref()
                .map(CorrelationId::new)
                .transpose()
                .map_err(|source| {
                    AtmError::observability_emit("failed to validate ATM daemon request id")
                        .with_source(source)
                })?,
            correlation_id: event
                .correlation_id
                .as_deref()
                .map(CorrelationId::new)
                .transpose()
                .map_err(|source| {
                    AtmError::observability_emit("failed to validate ATM daemon correlation id")
                        .with_source(source)
                })?,
            outcome: Some(event.outcome),
            diagnostic: None,
            state_transition: None,
            fields: event.fields,
        };
        self.logger.emit(event).map_err(|source| {
            let code = sc_observability_types::DiagnosticInfo::diagnostic(&source)
                .code
                .as_str()
                .to_string();
            AtmError::observability_emit(format!(
                "shared daemon observability emit failed ({code})"
            ))
            .with_source(source)
        })
    }
}

fn build_logger(
    log_dir: &Path,
    retained_sink_fault: Option<RetainedSinkFaultMode>,
    service_name: &ServiceName,
    rotation_max_bytes: u64,
) -> Result<
    (
        sc_observability::Logger,
        PathBuf,
        Arc<RetainedJsonlFileSink>,
    ),
    AtmError,
> {
    let active_log_path = log_dir.join("atm.log.jsonl");
    ensure_retained_log_ready(log_dir, &active_log_path)?;
    let mut config =
        sc_observability::LoggerConfig::default_for(service_name.clone(), PathBuf::new());
    config.level = logger_level_override()?.unwrap_or(SharedLevelFilter::Info);
    config.enable_console_sink = false;
    config.enable_file_sink = false;
    let mut builder = sc_observability::Logger::builder(config).map_err(|source| {
        AtmError::observability_bootstrap("failed to initialize shared daemon observability logger")
            .with_source(source)
    })?;
    // sc-observability 1.0.0 JsonlFileSink performs synchronous local-file writes.
    // atm-daemon satisfies ADR-011's non-blocking-executor rule by never calling
    // this sink from an async executor thread; retained writes stay on ordinary
    // daemon OS threads, and shutdown flush runs on a dedicated finalizer thread.
    let retained_sink = Arc::new(RetainedJsonlFileSink::new(
        active_log_path.clone(),
        sc_observability::RotationPolicy {
            max_bytes: rotation_max_bytes,
            max_files: RETAINED_LOG_ROTATION_MAX_FILES,
        },
        sc_observability::RetentionPolicy {
            max_age_days: RETAINED_LOG_RETENTION_MAX_AGE_DAYS,
        },
    ));
    #[cfg(test)]
    let sink: Arc<dyn sc_observability::LogSink> = match retained_sink_fault {
        Some(mode) => Arc::new(RetainedSinkHealthOverride::new(retained_sink.clone(), mode)),
        None => retained_sink.clone(),
    };
    #[cfg(not(test))]
    let sink: Arc<dyn sc_observability::LogSink> = {
        let _ = retained_sink_fault;
        retained_sink.clone()
    };
    builder.register_sink(sc_observability::SinkRegistration::new(sink));
    Ok((builder.build(), active_log_path, retained_sink))
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

#[derive(Debug)]
// The retained sink keeps three separate mutexes because the health state is read and updated on
// the hot path, while the last-written file handle and prune timestamp are touched far less often.
// Keeping them independent avoids serializing unrelated reads/writes behind one coarse lock.
// Lock order matters when more than one is needed: write() updates last_written_file before health.
struct RetainedJsonlFileSink {
    path: PathBuf,
    rotation: sc_observability::RotationPolicy,
    retention: sc_observability::RetentionPolicy,
    health: Mutex<SinkHealth>,
    last_written_file: Mutex<Option<std::fs::File>>,
    prune_in_progress: Arc<AtomicBool>,
    last_prune_request_at: Mutex<Option<SystemTime>>,
}

impl RetainedJsonlFileSink {
    fn new(
        path: PathBuf,
        rotation: sc_observability::RotationPolicy,
        retention: sc_observability::RetentionPolicy,
    ) -> Self {
        Self {
            path,
            rotation,
            retention,
            health: Mutex::new(SinkHealth {
                name: SinkName::new("jsonl_file_sink").expect("jsonl sink constant is valid"),
                state: SinkHealthState::Healthy,
                last_error: None,
            }),
            last_written_file: Mutex::new(None),
            prune_in_progress: Arc::new(AtomicBool::new(false)),
            last_prune_request_at: Mutex::new(None),
        }
    }

    fn sync_last_written_file(&self) -> std::io::Result<()> {
        let last_written = self
            .last_written_file
            .lock()
            .map_err(|_| std::io::Error::other("retained sink sync handle lock poisoned"))?;
        if let Some(file) = last_written.as_ref() {
            file.sync_all()?;
        }
        Ok(())
    }

    fn rotate_if_needed(&self, incoming_len: u64) {
        if let Ok(metadata) = fs::metadata(&self.path)
            && metadata.len().saturating_add(incoming_len) > self.rotation.max_bytes
        {
            for idx in (1..self.rotation.max_files).rev() {
                let src = self.rotated_path(idx);
                let dest = self.rotated_path(idx + 1);
                let _ = rename_if_present(&src, &dest);
            }
            let rotated = self.rotated_path(1);
            let _ = rename_if_present(&self.path, &rotated);
        }
        self.schedule_prune_old_files();
    }

    fn rotated_path(&self, index: u32) -> PathBuf {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let file_name = self
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("atm.log.jsonl");
        parent.join(format!("{file_name}.{index}"))
    }

    fn schedule_prune_old_files(&self) {
        let now = SystemTime::now();
        {
            let Ok(mut last_request_at) = self.last_prune_request_at.lock() else {
                tracing::warn!(
                    "retained sink prune request lock poisoned; skipping one prune scheduling attempt"
                );
                return;
            };
            if let Some(last_request_at) = *last_request_at
                && now
                    .duration_since(last_request_at)
                    .is_ok_and(|elapsed| elapsed < RETAINED_LOG_PRUNE_INTERVAL)
            {
                return;
            }
            *last_request_at = Some(now);
        }

        if self
            .prune_in_progress
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let path = self.path.clone();
        let retention = self.retention;
        let prune_in_progress = Arc::clone(&self.prune_in_progress);
        if thread::Builder::new()
            .name("atm-log-prune".to_string())
            .spawn(move || {
                prune_old_files_at_path(&path, retention);
                prune_in_progress.store(false, Ordering::Release);
            })
            .is_err()
        {
            self.prune_in_progress.store(false, Ordering::Release);
        }
    }

    fn mark_failure<E>(&self, error: E) -> sc_observability_types::LogSinkError
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        let message = error.to_string();
        let diagnostic = ErrorContext::new(
            ErrorCode::new_static("SC_LOGGER_SINK_WRITE_FAILED"),
            "jsonl file sink write failed",
            Remediation::not_recoverable(
                "file sink write failure handling is owned by the logger runtime",
            ),
        )
        .cause(message)
        .source(Box::new(error));
        if let Ok(mut health) = self.health.lock() {
            health.state = SinkHealthState::DegradedDropping;
            health.last_error = Some(DiagnosticSummary::from(diagnostic.diagnostic()));
        } else {
            tracing::warn!("file sink health lock poisoned while recording sink failure");
        }
        sc_observability_types::LogSinkError(Box::new(diagnostic))
    }
}

fn prune_old_files_at_path(path: &Path, retention: sc_observability::RetentionPolicy) {
    let Some(parent) = path.parent() else {
        return;
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    let retention_cutoff =
        SystemTime::now() - Duration::from_secs(u64::from(retention.max_age_days) * 86_400);
    let active_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    for entry in entries.flatten() {
        let candidate = entry.path();
        let Some(file_name) = candidate.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !file_name.starts_with(active_name) || file_name == active_name {
            continue;
        }
        if let Ok(metadata) = entry.metadata()
            && let Ok(modified) = metadata.modified()
            && modified < retention_cutoff
        {
            let _ = fs::remove_file(candidate);
        }
    }
}

impl sc_observability::LogSink for RetainedJsonlFileSink {
    fn write(&self, event: &LogEvent) -> Result<(), sc_observability_types::LogSinkError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| self.mark_failure(error))?;
        }
        let mut line = serde_json::to_vec(event).map_err(|error| self.mark_failure(error))?;
        line.push(b'\n');
        self.rotate_if_needed(line.len() as u64);
        // Reopen per append intentionally: retained daemon events prioritize append-safety across
        // rotation/replacement over holding one long-lived write handle open on the hot path.
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| self.mark_failure(error))?;
        file.write_all(&line)
            .and_then(|()| file.flush())
            .map_err(|error| self.mark_failure(error))?;
        *self.last_written_file.lock().map_err(|_| {
            self.mark_failure(std::io::Error::other(
                "retained sink file handle lock poisoned",
            ))
        })? = Some(file);
        let mut health = self.health.lock().map_err(|_| {
            self.mark_failure(std::io::Error::other("file sink health lock poisoned"))
        })?;
        health.state = SinkHealthState::Healthy;
        Ok(())
    }

    fn flush(&self) -> Result<(), sc_observability_types::LogSinkError> {
        let mut last_written = self.last_written_file.lock().map_err(|_| {
            self.mark_failure(std::io::Error::other(
                "retained sink file handle lock poisoned",
            ))
        })?;
        if let Some(file) = last_written.as_mut() {
            file.flush().map_err(|error| self.mark_failure(error))?;
        }
        Ok(())
    }

    fn health(&self) -> SinkHealth {
        match self.health.lock() {
            Ok(health) => health.clone(),
            Err(_) => {
                tracing::warn!("file sink health lock poisoned; reporting unavailable sink health");
                SinkHealth {
                    name: SinkName::new("jsonl_file_sink").expect("jsonl sink constant is valid"),
                    state: SinkHealthState::Unavailable,
                    last_error: None,
                }
            }
        }
    }
}

fn rename_if_present(src: &Path, dest: &Path) -> std::io::Result<()> {
    match fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
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
    }
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
            .map_err(|source| {
                AtmError::observability_emit(
                    "failed to validate ATM daemon subsystem-event schema version",
                )
                .with_source(source)
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
        level: level_for_outcome(outcome.as_str()),
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
            // No global tracing subscriber is installed in production; daemon lifecycle records
            // route through emit_daemon_event(), so this unknown-outcome warning is intentionally silent.
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

    use atm_core::observability::{AtmObservabilityHealthState, ObservabilityPort};
    use atm_core::test_support::EnvGuard;
    use serial_test::serial;
    use tempfile::TempDir;

    use super::{DaemonObservability, observability_test_event};

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
        let log_path = tempdir.path().join("atm.log.jsonl");
        std::fs::write(&log_path, "active\n").expect("active log");
        let rotated_log_path = tempdir.path().join("atm.log.jsonl.1");
        std::fs::write(&rotated_log_path, "stale\n").expect("rotated log");

        let sink = super::RetainedJsonlFileSink::new(
            log_path,
            sc_observability::RotationPolicy {
                max_bytes: 1024,
                max_files: 5,
            },
            sc_observability::RetentionPolicy { max_age_days: 0 },
        );
        sink.schedule_prune_old_files();

        let deadline = Instant::now() + Duration::from_secs(2);
        while rotated_log_path.exists() && Instant::now() < deadline {
            std::thread::yield_now();
        }

        assert!(
            !rotated_log_path.exists(),
            "background prune worker should remove expired rotated files"
        );
    }
}
