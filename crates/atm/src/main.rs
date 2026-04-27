mod commands;
mod observability;
mod output;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::home;
use atm_core::observability::{
    AtmLogQuery, AtmLogRecord, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState,
    CommandEvent, LogFieldMap, LogFieldMatch, LogLevelFilter, LogOrder, LogTailSession,
    ObservabilityPort,
};
use chrono::{DateTime, Utc};
use clap::Parser;
use clap::error::ErrorKind;
use sc_observability::{
    ConsoleSink, JsonlFileSink, LogSink, Logger, LoggerBuilder, LoggerConfig, RetentionPolicy,
    RotationPolicy, SinkRegistration,
};
use sc_observability_types::{
    ActionName, CorrelationId, DiagnosticInfo, Level, LevelFilter as SharedLevelFilter, LogEvent,
    LogQuery, OutcomeLabel, ProcessIdentity, QueryError, SchemaVersion, ServiceName, SinkHealth,
    SinkHealthState, TargetCategory, Timestamp,
};
use serde_json::Map;
use time::OffsetDateTime;
use tracing_subscriber::filter::LevelFilter as TracingLevelFilter;

const ATM_SERVICE_NAME: &str = "atm";
const ATM_COMMAND_TARGET: &str = "atm.command";
const ATM_LOG_LEVEL_ENV: &str = "ATM_LOG";
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

    if let Err(error) = init_tracing(cli.stderr_logs()) {
        let fallback = observability::CliObservability::fallback();
        fallback.emit_fatal_error("bootstrap", &error);
        return Err(error.into());
    }

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
        ScObservabilityAdapter::new(logger, service_name, target_category),
    )))
}

pub(crate) fn build_logger(
    home_dir: &Path,
    console_log_route: ConsoleLogRoute,
    service_name: &ServiceName,
) -> Result<Logger, AtmError> {
    let mut config = LoggerConfig::default_for(service_name.clone(), log_root(home_dir));
    if let Some(level) = logger_level_override()? {
        config.level = level;
    }
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

    tracing::subscriber::set_global_default(subscriber).map_err(|source| {
        AtmError::observability_bootstrap("failed to initialize ATM tracing subscriber")
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

struct ScObservabilityAdapter {
    logger: Logger,
    service_name: ServiceName,
    target_category: TargetCategory,
}

impl ScObservabilityAdapter {
    fn new(logger: Logger, service_name: ServiceName, target_category: TargetCategory) -> Self {
        Self {
            logger,
            service_name,
            target_category,
        }
    }
}

impl atm_core::observability::sealed::Sealed for ScObservabilityAdapter {}

impl ObservabilityPort for ScObservabilityAdapter {
    fn emit(&self, event: CommandEvent) -> Result<(), AtmError> {
        let event = map_command_event(&self.service_name, &self.target_category, event)?;
        self.logger.emit(event).map_err(|source| {
            let code = source.diagnostic().code.as_str().to_string();
            AtmError::observability_emit(format!("shared observability emit failed ({code})"))
                .with_source(source)
        })
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
        let query_detail = report
            .query
            .as_ref()
            .and_then(|query| query.last_error.clone().map(render_diagnostic_summary));
        Ok(AtmObservabilityHealth {
            active_log_path: Some(report.active_log_path),
            logging_state: map_logging_state(report.state),
            query_state,
            detail: report
                .last_error
                .map(render_diagnostic_summary)
                .or(query_detail),
        })
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
                AtmError::observability_emit("failed to validate ATM observability schema version")
                    .with_source(source)
            })?;
    let action = ActionName::new(event.action).map_err(|source| {
        AtmError::observability_emit("failed to validate ATM observability action")
            .with_source(source)
    })?;
    let request_id = event
        .message_id
        .map(|value| CorrelationId::new(value.to_string()))
        .transpose()
        .map_err(|source| {
            AtmError::observability_emit("failed to validate ATM observability request id")
                .with_source(source)
        })?;
    let correlation_id = event
        .task_id
        .as_deref()
        .map(CorrelationId::new)
        .transpose()
        .map_err(|source| {
            AtmError::observability_emit("failed to validate ATM observability correlation id")
                .with_source(source)
        })?;
    let outcome = OutcomeLabel::new(event.outcome).map_err(|source| {
        AtmError::observability_emit("failed to validate ATM observability outcome label")
            .with_source(source)
    })?;

    let mut fields = Map::new();
    fields.insert(
        "command".to_string(),
        serde_json::Value::String(event.command.to_string()),
    );
    fields.insert(
        "team".to_string(),
        serde_json::Value::String(event.team.clone()),
    );
    fields.insert(
        "agent".to_string(),
        serde_json::Value::String(event.agent.clone()),
    );
    fields.insert(
        "sender".to_string(),
        serde_json::Value::String(event.sender.clone()),
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
    let value = serde_json::to_value(&field_match.value).map_err(|source| {
        AtmError::observability_query("failed to encode ATM log field match value")
            .with_source(source)
    })?;

    Ok(sc_observability_types::LogFieldMatch::equals(key, value))
}

fn map_snapshot(snapshot: sc_observability_types::LogSnapshot) -> Result<AtmLogSnapshot, AtmError> {
    let records = snapshot
        .events
        .into_iter()
        .map(map_record)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AtmLogSnapshot {
        records,
        truncated: snapshot.truncated,
    })
}

fn map_record(event: LogEvent) -> Result<AtmLogRecord, AtmError> {
    let fields = serde_json::from_value::<LogFieldMap>(serde_json::Value::Object(event.fields))
        .map_err(|source| {
            AtmError::observability_query("failed to project shared log fields into ATM types")
                .with_source(source)
        })?;
    Ok(AtmLogRecord {
        timestamp: map_timestamp_back(event.timestamp)?,
        severity: map_level_back(event.level),
        service: event.service.to_string(),
        target: Some(event.target.to_string()),
        action: Some(event.action.to_string()),
        message: event.message,
        fields,
    })
}

fn map_timestamp(timestamp: atm_core::types::IsoTimestamp) -> Result<Timestamp, AtmError> {
    let datetime = timestamp.into_inner();
    let nanos = datetime.timestamp_nanos_opt().ok_or_else(|| {
        AtmError::observability_query("ATM timestamp could not be converted to nanoseconds")
    })?;
    let offset = OffsetDateTime::from_unix_timestamp_nanos(nanos.into()).map_err(|source| {
        AtmError::observability_query("failed to convert ATM timestamp to shared timestamp")
            .with_source(source)
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
    match outcome {
        "ok" | "sent" | "dry_run" => Level::Info,
        "timeout" => Level::Warn,
        "error" | "failed" => Level::Error,
        other => {
            tracing::warn!(code = %AtmErrorCode::ObservabilityEmitFailed,
                outcome = other,
                "unknown ATM command outcome for observability level"
            );
            Level::Warn
        }
    }
}

fn map_query_error(source: QueryError) -> AtmError {
    let code = source.code().as_str().to_string();
    AtmError::observability_query(format!("shared observability query failed ({code})"))
        .with_source(source)
}

fn map_follow_error(phase: &str, source: QueryError) -> AtmError {
    let code = source.code().as_str().to_string();
    AtmError::observability_follow(format!(
        "shared observability follow {phase} failed ({code})"
    ))
    .with_source(source)
}

fn render_diagnostic_summary(summary: sc_observability_types::DiagnosticSummary) -> String {
    match summary.code {
        Some(code) => format!("{}: {}", code.as_str(), summary.message),
        None => summary.message,
    }
}

#[cfg(test)]
pub(crate) fn new_adapter_port_for_tests(
    home_dir: &std::path::Path,
    stderr_logs: bool,
) -> Result<Box<dyn ObservabilityPort + Send + Sync>, AtmError> {
    let service_name = ServiceName::new(ATM_SERVICE_NAME).map_err(|source| {
        AtmError::observability_bootstrap("failed to validate ATM service name").with_source(source)
    })?;
    let target_category = TargetCategory::new(ATM_COMMAND_TARGET).map_err(|source| {
        AtmError::observability_bootstrap("failed to validate ATM observability target")
            .with_source(source)
    })?;
    let console_log_route = if stderr_logs {
        ConsoleLogRoute::Stderr
    } else {
        ConsoleLogRoute::Disabled
    };
    let logger = build_logger(home_dir, console_log_route, &service_name)?;
    Ok(Box::new(ScObservabilityAdapter::new(
        logger,
        service_name,
        target_category,
    )))
}

#[cfg(test)]
mod adapter_tests {
    use std::sync::{Mutex, OnceLock};

    use sc_observability_types::LevelFilter as SharedLevelFilter;
    use serial_test::serial;
    use tracing_subscriber::filter::LevelFilter as TracingLevelFilter;

    use super::{
        ATM_LOG_LEVEL_ENV, level_for_outcome, logger_level_override, tracing_level_filter,
    };

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn with_env_var<R>(key: &str, value: Option<&str>, f: impl FnOnce() -> R) -> R {
        let _guard = env_lock().lock().expect("env lock");
        let previous = std::env::var_os(key);
        match value {
            Some(value) => {
                // SAFETY: this test helper serializes process environment access.
                unsafe { std::env::set_var(key, value) }
            }
            None => {
                // SAFETY: this test helper serializes process environment access.
                unsafe { std::env::remove_var(key) }
            }
        }
        let result = f();
        match previous {
            Some(value) => {
                // SAFETY: this test helper serializes process environment access.
                unsafe { std::env::set_var(key, value) }
            }
            None => {
                // SAFETY: this test helper serializes process environment access.
                unsafe { std::env::remove_var(key) }
            }
        }
        result
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
            ("timeout", sc_observability_types::Level::Warn),
            ("error", sc_observability_types::Level::Error),
            ("failed", sc_observability_types::Level::Error),
        ];

        for (outcome, expected) in cases {
            assert_eq!(level_for_outcome(outcome), expected, "outcome={outcome}");
        }
    }

    #[test]
    #[serial]
    fn logger_level_override_accepts_debug() {
        with_env_var(ATM_LOG_LEVEL_ENV, Some("debug"), || {
            assert_eq!(
                logger_level_override().expect("override"),
                Some(SharedLevelFilter::Debug)
            );
        });
    }

    #[test]
    #[serial]
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
}
