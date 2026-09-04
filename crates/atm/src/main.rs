#[cfg(any(test, feature = "cli-surface-dump"))]
mod cli_surface;
mod commands;
mod composition;
mod constants;
mod observability;
mod output;
mod output_contract;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::error_codes::AtmErrorCode::{
    BindPreflightFailed, CertificateOperationFailed, MessageIdConflict, MessageValidationFailed,
    PeerConfigValidationFailed, SelfAddressedSendInvalid,
};
use atm_core::home;
#[cfg(any(test, feature = "fault-injection"))]
use atm_core::observability::RetainedSinkFaultMode;
use atm_core::observability::{
    AtmLogQuery, AtmLogRecord, AtmLogSnapshot, AtmMaintenanceHealthReport,
    AtmMaintenanceWorkerState, AtmObservabilityDiagnostic, AtmObservabilityHealth,
    AtmObservabilityHealthState, CommandEvent, LogFieldMap, LogFieldMatch, LogLevelFilter,
    LogOrder, LogTailSession, ObservabilityPort, diagnostic_code, service_name,
    standard_level_for_outcome,
};
#[cfg(any(test, feature = "fault-injection"))]
use atm_observability::retained_sink_fault_mode as shared_retained_sink_fault_mode;
use atm_observability::{
    logger_level_override as shared_logger_level_override,
    logger_root_for_log_dir as shared_logger_root_for_log_dir, prepare_retained_log,
};
use chrono::{DateTime, Utc};
#[cfg(any(test, feature = "cli-surface-dump"))]
use clap::CommandFactory;
use clap::Parser;
use clap::error::ErrorKind;
#[cfg(any(test, feature = "fault-injection"))]
use sc_observability::LogSink;
#[cfg(any(test, feature = "fault-injection"))]
use sc_observability::LoggerBuilder;
use sc_observability::{ConsoleSink, Logger, LoggerConfig, SinkRegistration};
#[cfg(any(test, feature = "fault-injection"))]
use sc_observability::{JsonlFileSink, RetentionPolicy, RotationPolicy};
use sc_observability_types::{
    ActionName, CorrelationId, DiagnosticInfo, Level, LevelFilter as SharedLevelFilter, LogEvent,
    LogQuery, OutcomeLabel, ProcessIdentity, QueryError, SchemaVersion, ServiceName,
    TargetCategory, Timestamp,
};
#[cfg(any(test, feature = "fault-injection"))]
use sc_observability_types::{SinkHealth, SinkHealthState};
use serde_json::Map;
use time::OffsetDateTime;
use tracing_subscriber::filter::LevelFilter as TracingLevelFilter;

const ATM_COMMAND_TARGET: &str = "atm.command";
#[cfg(test)]
const ATM_LOG_LEVEL_ENV: &str = "ATM_LOG";
const MAX_RETAINED_QUERY_RECORD_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConsoleLogRoute {
    Disabled,
    Stderr,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let exit_code = match run().await {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            exit_code_for_atm_error(&error)
        }
    };
    std::process::exit(exit_code);
}

#[cfg(test)]
fn exit_code_for_error(error: &anyhow::Error) -> i32 {
    error
        .downcast_ref::<AtmError>()
        .map_or(1, exit_code_for_atm_error)
}

#[allow(
    clippy::too_many_lines,
    reason = "centralized stable CLI error-code mapping"
)]
fn exit_code_for_atm_error(error: &AtmError) -> i32 {
    if is_warning_or_internal_error(error.code()) {
        return 1;
    }
    match error.code() {
        PeerConfigValidationFailed | CertificateOperationFailed | BindPreflightFailed => 3,
        AtmErrorCode::ConfigHomeUnavailable
        | AtmErrorCode::AtmHomeUnresolved
        | AtmErrorCode::ConfigParseFailed
        | AtmErrorCode::ConfigRetiredHookMembersKey
        | AtmErrorCode::ConfigRetiredLegacyHookKeys
        | AtmErrorCode::ConfigTeamParseFailed
        | AtmErrorCode::ConfigTeamMissing
        // `atm compose` is a process-level adapter for `sc-compose`. Keep
        // its source/load/include/render failures in the upstream CLI's
        // configuration/validation exit category so callers can distinguish
        // them from ATM request validation (3) and internal failures (1).
        | AtmErrorCode::TemplateLoadFailed
        | AtmErrorCode::TemplateRenderVerificationFailed
        | AtmErrorCode::TemplateIncludeUnresolved => 2,
        AtmErrorCode::IdentityUnavailable
        | AtmErrorCode::IdentityInvalid
        | AtmErrorCode::IdentityConflict
        | AtmErrorCode::MemberAlreadyExists
        | AtmErrorCode::MemberNotFound
        | AtmErrorCode::AddressParseFailed
        | AtmErrorCode::TeamUnavailable
        | AtmErrorCode::TeamInvalid
        | AtmErrorCode::TeamNotFound
        | AtmErrorCode::AgentNotFound
        | MessageValidationFailed
        | MessageIdConflict
        | SelfAddressedSendInvalid
        | AtmErrorCode::EmptyNudgeTemplateBody
        | AtmErrorCode::CallerContextRequestInvalid
        | AtmErrorCode::AckInvalidState
        | AtmErrorCode::ClearInvalidState
        | AtmErrorCode::HelpTopicNotFound
        | AtmErrorCode::TestFakeTransportInjectionFailed => 3,
        AtmErrorCode::DaemonUnavailable
        | AtmErrorCode::RuntimeRootInvalid
        | AtmErrorCode::RuntimeBootstrapRefused
        | AtmErrorCode::SocketOverrideForbidden
        | AtmErrorCode::DaemonMayHaveExecuted
        | AtmErrorCode::DaemonLifecycleWedge
        | AtmErrorCode::DaemonLaunchGateRejected
        | AtmErrorCode::DaemonServingStateRejected
        | AtmErrorCode::DaemonStaleOwnerRecoveryFailed
        | AtmErrorCode::DaemonAutoStartFailed
        | AtmErrorCode::DaemonConnectionSaturated
        | AtmErrorCode::ClientDaemonVersionIncompatible
        | AtmErrorCode::DaemonAdvisorySessionAlreadyRegistered
        | AtmErrorCode::DaemonAdvisorySessionNotRegistered
        | AtmErrorCode::DaemonAdvisorySessionCleanupFailed
        | AtmErrorCode::WarningSqliteHealthDegraded => 4,
        AtmErrorCode::MailboxReadFailed
        | AtmErrorCode::MailboxWriteFailed
        | AtmErrorCode::MailboxLockFailed
        | AtmErrorCode::MailboxLockReadOnlyFilesystem
        | AtmErrorCode::MailboxLockTimeout => 5,
        AtmErrorCode::FilePolicyRejected | AtmErrorCode::FileReferenceRewriteFailed => 6,
        AtmErrorCode::ObservabilityEmitFailed
        | AtmErrorCode::ObservabilityQueryFailed
        | AtmErrorCode::ObservabilityFollowFailed
        | AtmErrorCode::ObservabilityHealthFailed
        | AtmErrorCode::ObservabilityBootstrapFailed
        | AtmErrorCode::ObservabilityHealthOk => 7,
        AtmErrorCode::SerializationFailed => 8,
        AtmErrorCode::WaitTimeout => 9,
        _ => 1,
    }
}

fn is_warning_or_internal_error(code: AtmErrorCode) -> bool {
    matches!(
        code,
        AtmErrorCode::WarningInvalidTeamMemberSkipped
            | AtmErrorCode::WarningMailboxRecordSkipped
            | AtmErrorCode::WarningMalformedAtmFieldIgnored
            | AtmErrorCode::WarningObservabilityHealthDegraded
            | AtmErrorCode::WarningOriginInboxEntrySkipped
            | AtmErrorCode::WarningMissingTeamConfigFallback
            | AtmErrorCode::WarningSendAlertStateDegraded
            | AtmErrorCode::WarningIdentityDrift
            | AtmErrorCode::WarningRosterDrift
            | AtmErrorCode::WarningBaselineMemberMissing
            | AtmErrorCode::WarningRestoreInProgress
            | AtmErrorCode::WarningStaleMailboxLock
            | AtmErrorCode::WarningHookSkipped
            | AtmErrorCode::WarningHookExecutionFailed
            | AtmErrorCode::PostSendPaneMissing
            | AtmErrorCode::PostSendTmuxSendFailed
            | AtmErrorCode::PostSendGraftUnavailable
            | AtmErrorCode::PostSendAdvisoryDeliveryFailed
            | AtmErrorCode::InternalError
    )
}

async fn run() -> Result<(), AtmError> {
    let cli = match commands::Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                error.print().map_err(|_source| {
                    AtmError::validation("failed to write ATM help/version output")
                })?;
                return Ok(());
            }
            let validation_error = atm_core::error::AtmError::validation(error.to_string());
            observability::CliObservability::fallback()
                .report_fatal_error("parse", &validation_error);
            return Err(validation_error);
        }
    };

    if let Err(error) = init_tracing(cli.stderr_logs()) {
        let fallback = observability::CliObservability::fallback();
        fallback.report_fatal_error("bootstrap", &error);
        return Err(error);
    }

    let observability = match init_observability(cli.stderr_logs()) {
        Ok(observability) => observability,
        Err(error) => {
            let fallback = observability::CliObservability::fallback();
            fallback.report_fatal_error("bootstrap", &error);
            return Err(error);
        }
    };

    if let Ok(launch_cwd) = home::command_invocation_dir() {
        tracing::info!(launch_cwd = %launch_cwd.display(), "atm process started");
    }

    match cli.run(&observability).await {
        Ok(()) => Ok(()),
        Err(error) => Err(report_and_map_service_error(&observability, error)),
    }
}

/// Prints the live CLI-surface tree in the requested `mode` (`json` or
/// `markdown`) to stdout. Called only by the hidden parsed
/// `atm __dump-cli-surface --format <json|markdown>` command.
#[cfg(any(test, feature = "cli-surface-dump"))]
pub(crate) fn dump_cli_surface(mode: commands::CliSurfaceFormat) -> Result<(), AtmError> {
    let mut root = commands::Cli::command();
    // `Command::build()` finalizes derived properties (e.g. `num_args`,
    // default values) that clap otherwise only resolves lazily during
    // parsing. Introspection without this call would see stale/empty
    // values for those fields.
    root.build();
    match mode {
        commands::CliSurfaceFormat::Json => {
            let surface = cli_surface::command_surface_json(&root);
            let rendered = serde_json::to_string_pretty(&surface)
                .map_err(|_source| AtmError::validation("failed to render CLI-surface JSON"))?;
            println!("{rendered}");
            Ok(())
        }
        commands::CliSurfaceFormat::Markdown => {
            println!("{}", cli_surface::command_surface_markdown(&root));
            Ok(())
        }
    }
}

fn report_and_map_service_error(
    observability: &observability::CliObservability,
    error: anyhow::Error,
) -> AtmError {
    match error.downcast::<AtmError>() {
        Ok(error) => {
            observability.report_fatal_error("service", &error);
            error
        }
        Err(error) => {
            let mapped =
                AtmError::validation(format!("ATM CLI command failed unexpectedly: {error}"));
            observability.report_fatal_error("service", &mapped);
            mapped
        }
    }
}

fn init_observability(stderr_logs: bool) -> Result<observability::CliObservability, AtmError> {
    let log_dir = home::host_log_dir()?;
    let console_log_route = if stderr_logs {
        ConsoleLogRoute::Stderr
    } else {
        ConsoleLogRoute::Disabled
    };
    let service_name = ServiceName::new(constants::ATM_SERVICE_NAME).map_err(|_source| {
        AtmError::observability_bootstrap("failed to validate ATM service name")
    })?;
    let target_category = TargetCategory::new(ATM_COMMAND_TARGET).map_err(|_source| {
        AtmError::observability_bootstrap("failed to validate ATM observability target")
    })?;
    let (logger, active_log_path) = build_logger(&log_dir, console_log_route, &service_name)?;

    Ok(observability::CliObservability::from_boxed_port(Box::new(
        ScObservabilityAdapter::new(logger, active_log_path, service_name, target_category),
    )))
}

pub(crate) fn build_logger(
    log_dir: &Path,
    console_log_route: ConsoleLogRoute,
    service_name: &ServiceName,
) -> Result<(Logger, PathBuf), AtmError> {
    let active_log_path = prepare_retained_log(log_dir)?;
    let mut config =
        LoggerConfig::default_for(service_name.clone(), logger_root_for_log_dir(log_dir)?);
    config.level = logger_level_override()?.unwrap_or(SharedLevelFilter::Info);
    // Make the retained file threshold explicit so lifecycle info! events stay
    // in the default retained log unless ATM_LOG overrides the level.
    // ATM CLI owns stdout/stderr UX by default; only opt into a shared
    // console sink when the CLI routing rule explicitly selects one.
    config.enable_console_sink = false;
    let mut builder = Logger::builder(config).map_err(|_source| {
        AtmError::observability_bootstrap("failed to initialize shared observability logger")
    })?;
    if console_log_route == ConsoleLogRoute::Stderr {
        builder.register_sink(SinkRegistration::new(Arc::new(ConsoleSink::stderr())));
    }
    #[cfg(any(test, feature = "fault-injection"))]
    if let Some(mode) = retained_sink_fault_mode()? {
        register_retained_sink_fault(&mut builder, log_dir, mode);
    }
    Ok((builder.build(), active_log_path))
}

#[cfg(test)]
fn ensure_retained_log_ready(log_dir: &Path, active_log_path: &Path) -> Result<(), AtmError> {
    let prepared = prepare_retained_log(log_dir)?;
    debug_assert_eq!(prepared, active_log_path);
    Ok(())
}

fn logger_root_for_log_dir(log_dir: &Path) -> Result<PathBuf, AtmError> {
    shared_logger_root_for_log_dir(log_dir)
}

#[cfg(any(test, feature = "fault-injection"))]
fn fault_injection_log_path(log_dir: &Path) -> PathBuf {
    log_dir.join("atm-fault-injection.log.jsonl")
}

fn init_tracing(stderr_logs: bool) -> Result<(), AtmError> {
    if !stderr_logs {
        return Ok(());
    }

    let subscriber = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_max_level(tracing_level_filter(
            logger_level_override()?.unwrap_or(SharedLevelFilter::Info),
        ))
        .without_time()
        .finish();

    tracing::subscriber::set_global_default(subscriber).map_err(|_source| {
        AtmError::observability_bootstrap("failed to initialize ATM tracing subscriber")
    })
}

fn logger_level_override() -> Result<Option<SharedLevelFilter>, AtmError> {
    shared_logger_level_override()
}

fn tracing_level_filter(level: SharedLevelFilter) -> TracingLevelFilter {
    match level {
        SharedLevelFilter::Trace => TracingLevelFilter::TRACE,
        SharedLevelFilter::Debug => TracingLevelFilter::DEBUG,
        SharedLevelFilter::Info => TracingLevelFilter::INFO,
        SharedLevelFilter::Warn => TracingLevelFilter::WARN,
        SharedLevelFilter::Error => TracingLevelFilter::ERROR,
        SharedLevelFilter::Off => TracingLevelFilter::OFF,
    }
}

#[cfg(any(test, feature = "fault-injection"))]
fn retained_sink_fault_mode() -> Result<Option<RetainedSinkFaultMode>, AtmError> {
    shared_retained_sink_fault_mode()
}

#[cfg(any(test, feature = "fault-injection"))]
fn register_retained_sink_fault(
    builder: &mut LoggerBuilder,
    log_dir: &Path,
    mode: RetainedSinkFaultMode,
) {
    let sink = Arc::new(JsonlFileSink::new(
        fault_injection_log_path(log_dir),
        RotationPolicy::default(),
        RetentionPolicy::default(),
    ));
    builder.register_sink(SinkRegistration::new(Arc::new(
        RetainedSinkHealthOverride::new(sink, mode),
    )));
}

#[cfg(any(test, feature = "fault-injection"))]
struct RetainedSinkHealthOverride {
    inner: Arc<dyn LogSink>,
    mode: RetainedSinkFaultMode,
}

#[cfg(any(test, feature = "fault-injection"))]
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

#[cfg(any(test, feature = "fault-injection"))]
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

struct ScObservabilityAdapter {
    logger: Logger,
    active_log_path: PathBuf,
    service_name: ServiceName,
    target_category: TargetCategory,
}

impl ScObservabilityAdapter {
    fn new(
        logger: Logger,
        active_log_path: PathBuf,
        service_name: ServiceName,
        target_category: TargetCategory,
    ) -> Self {
        Self {
            logger,
            active_log_path,
            service_name,
            target_category,
        }
    }
}

impl atm_core::boundary::sealed::Sealed for ScObservabilityAdapter {}

impl ObservabilityPort for ScObservabilityAdapter {
    fn emit(&self, event: CommandEvent) -> Result<(), AtmError> {
        // The CLI is a short-lived synchronous caller, so per-command flush is
        // the explicit durability barrier here. Do not reuse this adapter as a
        // daemon or async runtime logger without revisiting that contract.
        let event = map_command_event(&self.service_name, &self.target_category, event)?;
        self.logger.log(event).map_err(map_log_error)?;
        self.logger.flush().map_err(map_flush_error)
    }

    fn query(&self, req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
        let query = map_query(&self.service_name, &self.target_category, req)?;
        let snapshot = self.logger.query(&query).map_err(map_query_error)?;
        map_snapshot(snapshot)
    }

    fn follow(&self, req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
        let query = map_query(&self.service_name, &self.target_category, req)?;
        let mut session = self
            .logger
            .follow(query)
            .map_err(|source| map_follow_error("start", source))?;
        Ok(LogTailSession::from_poller(move || {
            let snapshot = session
                .poll()
                .map_err(|source| map_follow_error("poll", source))?;
            map_snapshot(snapshot)
        }))
    }

    fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
        let report = self.logger.health();
        let query_state = report
            .query
            .as_ref()
            .map(|query| map_query_state(query.state));
        let query_diagnostic = report
            .query
            .as_ref()
            .and_then(|query| query.last_error.clone().map(map_diagnostic_summary));
        let diagnostic = report
            .last_error
            .clone()
            .map(map_diagnostic_summary)
            .or(query_diagnostic);
        let detail = Some(build_logging_health_detail(&report, diagnostic.as_ref()));
        Ok(AtmObservabilityHealth {
            active_log_path: Some(self.active_log_path.clone()),
            logging_state: map_logging_state(report.state),
            query_state,
            maintenance: report.maintenance.map(map_maintenance_report).transpose()?,
            diagnostic,
            detail,
        })
    }
}

fn map_log_error(source: sc_observability::LogError) -> AtmError {
    let code = match &source {
        sc_observability::LogError::InvalidEvent(error) => error.diagnostic().code.as_str(),
        sc_observability::LogError::WriterDegraded(context)
        | sc_observability::LogError::ShutdownTimedOut(context) => {
            context.diagnostic().code.as_str()
        }
    };
    AtmError::observability_emit(format!(
        "shared observability log admission failed ({code})"
    ))
}

fn map_flush_error(source: sc_observability_types::FlushError) -> AtmError {
    let code = source.diagnostic().code.as_str();
    AtmError::observability_emit(format!(
        "shared observability durability flush failed ({code})"
    ))
}

fn build_logging_health_detail(
    report: &sc_observability_types::LoggingHealthReport,
    diagnostic: Option<&AtmObservabilityDiagnostic>,
) -> String {
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
        parts.push(format!(
            "maintenance state={} rotated_files_total={} pruned_files_total={} last_pass_at={}",
            maintenance_state_label(maintenance.state),
            maintenance.rotated_files_total,
            maintenance.pruned_files_total,
            maintenance
                .last_pass_at
                .map(|timestamp| timestamp.into_inner().to_string())
                .unwrap_or_else(|| "never".to_string())
        ));
    }
    parts.join(" | ")
}

fn writer_state_label(state: sc_observability_types::WriterState) -> &'static str {
    match state {
        sc_observability_types::WriterState::Running => "running",
        sc_observability_types::WriterState::Degraded => "degraded",
        sc_observability_types::WriterState::Stopped => "stopped",
    }
}

fn maintenance_state_label(state: sc_observability_types::MaintenanceWorkerState) -> &'static str {
    match state {
        sc_observability_types::MaintenanceWorkerState::Running => "running",
        sc_observability_types::MaintenanceWorkerState::Degraded => "degraded",
        sc_observability_types::MaintenanceWorkerState::Stopped => "stopped",
    }
}

fn map_maintenance_report(
    report: sc_observability_types::MaintenanceHealthReport,
) -> Result<AtmMaintenanceHealthReport, AtmError> {
    Ok(AtmMaintenanceHealthReport {
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
            .map(map_timestamp_back)
            .transpose()
            .map_err(|_source| {
                AtmError::observability_health(
                    "failed to project shared maintenance timestamp into ATM health",
                )
            })?,
    })
}

fn build_command_event_fields(event: &CommandEvent) -> Map<String, serde_json::Value> {
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
    fields
}

fn map_command_event(
    service_name: &ServiceName,
    target_category: &TargetCategory,
    event: CommandEvent,
) -> Result<LogEvent, AtmError> {
    let schema_version =
        SchemaVersion::new(sc_observability_types::constants::OBSERVATION_ENVELOPE_VERSION)
            .map_err(|_source| {
                AtmError::observability_emit("failed to validate ATM observability schema version")
            })?;
    let request_id = event
        .message_id
        .map(|value| CorrelationId::new(value.to_string()))
        .transpose()
        .map_err(|_source| {
            AtmError::observability_emit("failed to validate ATM observability request id")
        })?;
    let correlation_id = event
        .task_id
        .as_deref()
        .map(CorrelationId::new)
        .transpose()
        .map_err(|_source| {
            AtmError::observability_emit("failed to validate ATM observability correlation id")
        })?;
    let fields = build_command_event_fields(&event);
    let action = ActionName::new(event.action.as_str()).map_err(|_source| {
        AtmError::observability_emit("failed to validate ATM observability action")
    })?;
    let outcome = OutcomeLabel::new(event.outcome.as_str()).map_err(|_source| {
        AtmError::observability_emit("failed to validate ATM observability outcome")
    })?;
    Ok(LogEvent {
        version: schema_version,
        timestamp: Timestamp::now_utc(),
        level: level_for_outcome(event.outcome.as_str()),
        service: service_name.clone(),
        target: target_category.clone(),
        action,
        message: Some(format!(
            "ATM command {} completed with outcome {}",
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

fn map_query(
    service_name: &ServiceName,
    target_category: &TargetCategory,
    req: AtmLogQuery,
) -> Result<LogQuery, AtmError> {
    let field_matches = req
        .field_matches
        .into_iter()
        .map(map_field_match)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(LogQuery {
        service: Some(service_name.clone()),
        levels: req.levels.into_iter().map(map_level).collect(),
        target: Some(target_category.clone()),
        action: None,
        request_id: None,
        correlation_id: None,
        since: req.since.map(map_timestamp).transpose()?,
        until: req.until.map(map_timestamp).transpose()?,
        field_matches,
        limit: req.limit,
        order: map_order(req.order),
    })
}

fn map_field_match(
    field_match: LogFieldMatch,
) -> Result<sc_observability_types::LogFieldMatch, AtmError> {
    let key = field_match.key.as_str().to_string();
    let value = serde_json::to_value(&field_match.value).map_err(|_source| {
        AtmError::observability_query("failed to encode ATM log field match value")
    })?;

    Ok(sc_observability_types::LogFieldMatch::equals(key, value))
}

fn map_snapshot(snapshot: sc_observability_types::LogSnapshot) -> Result<AtmLogSnapshot, AtmError> {
    let records = snapshot.events.into_iter().try_fold(
        Vec::new(),
        |mut records, event| -> Result<Vec<AtmLogRecord>, AtmError> {
            if let Some(record) = map_record(event)? {
                records.push(record);
            }
            Ok(records)
        },
    )?;
    Ok(AtmLogSnapshot {
        records,
        truncated: snapshot.truncated,
    })
}

fn map_record(event: LogEvent) -> Result<Option<AtmLogRecord>, AtmError> {
    let encoded = serde_json::to_vec(&event).map_err(|_source| {
        AtmError::observability_query("failed to encode shared retained log event")
    })?;
    if encoded.len() > MAX_RETAINED_QUERY_RECORD_BYTES {
        tracing::warn!(
            bytes = encoded.len(),
            max_bytes = MAX_RETAINED_QUERY_RECORD_BYTES,
            service = %event.service,
            target = %event.target,
            action = %event.action,
            "dropping oversized retained log record during ATM projection"
        );
        return Ok(None);
    }
    let fields = serde_json::from_value::<LogFieldMap>(serde_json::Value::Object(event.fields))
        .map_err(|_source| {
            AtmError::observability_query("failed to project shared log fields into ATM types")
        })?;
    Ok(Some(AtmLogRecord {
        timestamp: map_timestamp_back(event.timestamp)?,
        level: map_level_back(event.level),
        service: service_name(event.service.as_str().to_string())?,
        target: Some(event.target.to_string()),
        action: Some(event.action.to_string()),
        message: event.message,
        fields,
    }))
}

fn map_timestamp(timestamp: atm_core::types::IsoTimestamp) -> Result<Timestamp, AtmError> {
    let datetime = timestamp.into_inner();
    let nanos = datetime.timestamp_nanos_opt().ok_or_else(|| {
        AtmError::observability_query("ATM timestamp could not be converted to nanoseconds")
    })?;
    let offset = OffsetDateTime::from_unix_timestamp_nanos(nanos.into()).map_err(|_source| {
        AtmError::observability_query("failed to convert ATM timestamp to shared timestamp")
    })?;
    Ok(Timestamp::from(offset))
}

fn map_timestamp_back(timestamp: Timestamp) -> Result<atm_core::types::IsoTimestamp, AtmError> {
    let offset: OffsetDateTime = timestamp.into();
    let datetime = DateTime::<Utc>::from_timestamp(offset.unix_timestamp(), offset.nanosecond())
        .ok_or_else(|| {
            AtmError::observability_query(
                "shared observability timestamp could not be converted to chrono",
            )
        })?;
    Ok(datetime.into())
}

fn map_level(level: LogLevelFilter) -> Level {
    match level {
        LogLevelFilter::Trace => Level::Trace,
        LogLevelFilter::Debug => Level::Debug,
        LogLevelFilter::Info => Level::Info,
        LogLevelFilter::Warn => Level::Warn,
        LogLevelFilter::Error => Level::Error,
    }
}

fn map_level_back(level: Level) -> LogLevelFilter {
    match level {
        Level::Trace => LogLevelFilter::Trace,
        Level::Debug => LogLevelFilter::Debug,
        Level::Info => LogLevelFilter::Info,
        Level::Warn => LogLevelFilter::Warn,
        Level::Error => LogLevelFilter::Error,
    }
}

fn map_order(order: LogOrder) -> sc_observability_types::LogOrder {
    match order {
        LogOrder::NewestFirst => sc_observability_types::LogOrder::NewestFirst,
        LogOrder::OldestFirst => sc_observability_types::LogOrder::OldestFirst,
    }
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

fn map_query_state(state: sc_observability_types::QueryHealthState) -> AtmObservabilityHealthState {
    match state {
        sc_observability_types::QueryHealthState::Healthy => AtmObservabilityHealthState::Healthy,
        sc_observability_types::QueryHealthState::Degraded => AtmObservabilityHealthState::Degraded,
        sc_observability_types::QueryHealthState::Unavailable => {
            AtmObservabilityHealthState::Unavailable
        }
    }
}

fn level_for_outcome(outcome: &str) -> Level {
    if matches!(
        outcome,
        "initial_miss"
            | "retry_attempt"
            | "pending"
            | "connected"
            | "acquired"
            | "launched"
            | "spawn_requested"
            | "publish_wait_started"
            | "publish_wait_continuing"
            | "auto_started"
    ) {
        return Level::Debug;
    }
    match standard_level_for_outcome(outcome) {
        atm_core::observability::Level::Trace => Level::Trace,
        atm_core::observability::Level::Debug => Level::Debug,
        atm_core::observability::Level::Info => Level::Info,
        atm_core::observability::Level::Warn => Level::Warn,
        atm_core::observability::Level::Error => Level::Error,
    }
}

fn map_query_error(_source: QueryError) -> AtmError {
    AtmError::observability_query("shared observability query failed")
}

fn map_follow_error(phase: &str, _source: QueryError) -> AtmError {
    AtmError::observability_follow(format!("shared observability follow {phase} failed"))
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

#[allow(
    unfulfilled_lint_expectations,
    reason = "The adapter-port constructor is exercised in normal builds even though the dead-code expectation remains documented for narrower configurations."
)]
#[expect(
    dead_code,
    reason = "The adapter-port constructor remains part of the CLI observability surface even when specific test configurations do not call it directly."
)]
pub(crate) fn new_adapter_port(
    home_dir: &std::path::Path,
    stderr_logs: bool,
) -> Result<Box<dyn ObservabilityPort + Send + Sync>, AtmError> {
    let service_name = ServiceName::new(constants::ATM_SERVICE_NAME).map_err(|_source| {
        AtmError::observability_bootstrap("failed to validate ATM service name")
    })?;
    let target_category = TargetCategory::new(ATM_COMMAND_TARGET).map_err(|_source| {
        AtmError::observability_bootstrap("failed to validate ATM observability target")
    })?;
    let console_log_route = if stderr_logs {
        ConsoleLogRoute::Stderr
    } else {
        ConsoleLogRoute::Disabled
    };
    let test_log_dir = resolve_adapter_log_dir(home_dir)?;
    let (logger, active_log_path) = build_logger(&test_log_dir, console_log_route, &service_name)?;
    Ok(Box::new(ScObservabilityAdapter::new(
        logger,
        active_log_path,
        service_name,
        target_category,
    )))
}

fn resolve_adapter_log_dir(_home_dir: &Path) -> Result<PathBuf, AtmError> {
    match home::host_log_dir() {
        Ok(log_dir) => Ok(log_dir),
        #[cfg(test)]
        Err(error) if error.code() == AtmErrorCode::ConfigHomeUnavailable => {
            Ok(home::host_log_dir_from_home(_home_dir))
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod adapter_tests {
    use anyhow::anyhow;
    use atm_core::error::{AtmError, AtmErrorCode};
    use atm_core::test_support::EnvGuard;
    use sc_observability_types::LevelFilter as SharedLevelFilter;
    use serial_test::serial;
    use tempfile::TempDir;
    use tracing_subscriber::filter::LevelFilter as TracingLevelFilter;

    use super::{
        ATM_LOG_LEVEL_ENV, ensure_retained_log_ready, exit_code_for_atm_error, exit_code_for_error,
        init_observability, level_for_outcome, logger_level_override, tracing_level_filter,
    };

    fn with_env_var<R>(key: &'static str, value: Option<&str>, f: impl FnOnce() -> R) -> R {
        match value {
            Some(value) => {
                let _guard = EnvGuard::set_raw(key, value);
                f()
            }
            None => {
                let _guard = EnvGuard::unset_raw(key);
                f()
            }
        }
    }

    #[test]
    fn unknown_outcome_maps_to_warn() {
        assert_eq!(
            level_for_outcome("future-outcome"),
            sc_observability_types::Level::Warn
        );
    }

    #[test]
    fn level_for_outcome_matches_documented_outcomes() {
        let cases = [
            ("ok", sc_observability_types::Level::Info),
            ("sent", sc_observability_types::Level::Info),
            ("dry_run", sc_observability_types::Level::Info),
            ("initial_miss", sc_observability_types::Level::Debug),
            ("retry_attempt", sc_observability_types::Level::Debug),
            ("pending", sc_observability_types::Level::Debug),
            ("connected", sc_observability_types::Level::Debug),
            ("acquired", sc_observability_types::Level::Debug),
            ("launched", sc_observability_types::Level::Debug),
            ("spawn_requested", sc_observability_types::Level::Debug),
            ("publish_wait_started", sc_observability_types::Level::Debug),
            (
                "publish_wait_continuing",
                sc_observability_types::Level::Debug,
            ),
            ("auto_started", sc_observability_types::Level::Debug),
            ("timeout", sc_observability_types::Level::Warn),
            ("error", sc_observability_types::Level::Error),
            ("failed", sc_observability_types::Level::Error),
        ];

        for (outcome, expected) in cases {
            assert_eq!(level_for_outcome(outcome), expected, "outcome={outcome}");
        }
    }

    #[test]
    #[serial(env)]
    fn logger_level_override_accepts_debug() {
        with_env_var(ATM_LOG_LEVEL_ENV, Some("debug"), || {
            assert_eq!(
                logger_level_override().expect("override"),
                Some(SharedLevelFilter::Debug)
            );
        });
    }

    #[test]
    #[serial(env)]
    fn logger_level_override_rejects_invalid_values() {
        with_env_var(ATM_LOG_LEVEL_ENV, Some("verbose"), || {
            let error = logger_level_override().expect_err("invalid override");
            assert!(
                error
                    .to_string()
                    .contains("invalid ATM_LOG value `verbose`"),
                "{error}"
            );
        });
    }

    #[test]
    fn tracing_level_filter_maps_off() {
        assert_eq!(
            tracing_level_filter(SharedLevelFilter::Off),
            TracingLevelFilter::OFF
        );
    }

    #[test]
    #[serial(env)]
    fn init_observability_fails_closed_when_atm_log_dir_is_invalid() {
        let tempdir = TempDir::new().expect("tempdir");
        let _env = EnvGuard::set_many([
            ("ATM_LOG", Some("info")),
            ("ATM_LOG_DIR", Some("relative/logs")),
            ("HOME", Some(tempdir.path().to_str().expect("utf8 path"))),
        ]);

        let error = init_observability(false).expect_err("invalid ATM_LOG_DIR should fail closed");
        assert!(error.is_config());
        assert!(error.message().contains("absolute path"));
    }

    #[test]
    fn retained_log_open_failure_preserves_the_os_error() {
        let tempdir = TempDir::new().expect("tempdir");
        let log_dir = tempdir.path().join("logs");
        std::fs::create_dir_all(&log_dir).expect("log dir");
        let active_log_path = log_dir.join("atm.log.jsonl");
        std::fs::create_dir(&active_log_path).expect("non-appendable log path");
        let expected = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&active_log_path)
            .expect_err("directory must not be appendable")
            .to_string();

        let error = ensure_retained_log_ready(&log_dir, &active_log_path)
            .expect_err("retained log open must fail closed");

        assert!(error.is_observability_bootstrap());
        assert!(
            error
                .message()
                .contains(&active_log_path.display().to_string())
        );
        assert!(error.message().contains(&expected), "{error}");
    }

    #[test]
    fn retained_log_directory_failure_preserves_the_os_error() {
        let tempdir = TempDir::new().expect("tempdir");
        let blocked_parent = tempdir.path().join("blocked-parent");
        std::fs::write(&blocked_parent, "not a directory").expect("blocked parent");
        let log_dir = blocked_parent.join("logs");
        let expected = std::fs::create_dir_all(&log_dir)
            .expect_err("child of a regular file must not be a directory")
            .to_string();

        let error = ensure_retained_log_ready(&log_dir, &log_dir.join("atm.log.jsonl"))
            .expect_err("retained log directory creation must fail closed");

        assert!(error.is_observability_bootstrap());
        assert!(error.message().contains(&log_dir.display().to_string()));
        assert!(error.message().contains(&expected), "{error}");
    }

    #[test]
    fn exit_code_categories_map_to_distinct_values() {
        assert_eq!(
            exit_code_for_atm_error(&AtmError::home_directory_unavailable()),
            2
        );
        assert_eq!(
            exit_code_for_atm_error(&AtmError::validation("bad input")),
            3
        );
        assert_eq!(
            exit_code_for_atm_error(&AtmError::new(
                AtmErrorCode::TemplateLoadFailed,
                "template unavailable"
            )),
            2
        );
        assert_eq!(
            exit_code_for_atm_error(&AtmError::new(
                AtmErrorCode::TemplateIncludeUnresolved,
                "template include unavailable"
            )),
            2
        );
        assert_eq!(
            exit_code_for_atm_error(&AtmError::new(
                AtmErrorCode::TemplateRenderVerificationFailed,
                "template verification failed"
            )),
            2
        );
        assert_eq!(
            exit_code_for_atm_error(&AtmError::daemon_unavailable("daemon down")),
            4
        );
        assert_eq!(
            exit_code_for_atm_error(&AtmError::mailbox_lock("locked")),
            5
        );
        assert_eq!(
            exit_code_for_atm_error(&AtmError::file_policy("blocked")),
            6
        );
        assert_eq!(
            exit_code_for_atm_error(&AtmError::observability_emit("emit failed")),
            7
        );
    }

    #[test]
    fn non_atm_errors_fall_back_to_exit_code_one() {
        let error = anyhow!("plain anyhow failure");
        assert_eq!(exit_code_for_error(&error), 1);
    }
}
