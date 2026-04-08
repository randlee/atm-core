//! Adapter layer bridging ATM CLI observability to `sc_observability`.
//!
//! This module is the sole sanctioned import site for `sc_observability`; all
//! shared observability calls route through this adapter to preserve the
//! inversion-of-control boundary between `atm` and `atm-core`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use atm_core::error::AtmError;
use atm_core::observability::{
    self, AtmLogQuery, AtmLogRecord, AtmLogSnapshot, AtmObservabilityHealth,
    AtmObservabilityHealthState, CommandEvent, LogFieldMap, LogFieldMatch, LogLevelFilter,
    LogOrder, LogTailSession, ObservabilityPort,
};
use chrono::{DateTime, Utc};
use sc_observability::{
    ConsoleSink, JsonlFileSink, Logger, LoggerBuilder, LoggerConfig, RetainedSinkFaultInjector,
    RetentionPolicy, RotationPolicy, SinkRegistration,
};
use sc_observability_types::{
    ActionName, CorrelationId, DiagnosticInfo, Level, LogEvent, LogQuery, OutcomeLabel,
    ProcessIdentity, QueryError, SchemaVersion, ServiceName, TargetCategory, Timestamp,
};
use serde_json::Map;
use time::OffsetDateTime;

const ATM_SERVICE_NAME: &str = "atm";
const ATM_COMMAND_TARGET: &str = "atm.command";
const ATM_OBSERVABILITY_RETAINED_SINK_FAULT_ENV: &str = "ATM_OBSERVABILITY_RETAINED_SINK_FAULT";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConsoleLogRoute {
    Disabled,
    Stderr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetainedSinkFaultMode {
    Degraded,
    Unavailable,
}

pub(crate) fn new_sc_observability_adapter(
    home_dir: &Path,
    stderr_logs: bool,
) -> Result<Box<dyn ObservabilityPort + Send + Sync>, AtmError> {
    let console_log_route = if stderr_logs {
        ConsoleLogRoute::Stderr
    } else {
        ConsoleLogRoute::Disabled
    };
    Ok(Box::new(ScObservabilityAdapter::new(
        home_dir,
        console_log_route,
    )?))
}

struct ScObservabilityAdapter {
    logger: Logger,
    service_name: ServiceName,
    target_category: TargetCategory,
}

impl observability::sealed::Sealed for ScObservabilityAdapter {}

impl ScObservabilityAdapter {
    fn new(home_dir: &Path, console_log_route: ConsoleLogRoute) -> Result<Self, AtmError> {
        let service_name = ServiceName::new(ATM_SERVICE_NAME).map_err(|source| {
            AtmError::observability_bootstrap("failed to validate ATM service name")
                .with_source(source)
        })?;
        let target_category = TargetCategory::new(ATM_COMMAND_TARGET).map_err(|source| {
            AtmError::observability_bootstrap("failed to validate ATM observability target")
                .with_source(source)
        })?;
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
        let logger = builder.build();

        Ok(Self {
            logger,
            service_name,
            target_category,
        })
    }
}

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

fn log_root(home_dir: &Path) -> PathBuf {
    home_dir.join(".local").join("share")
}

fn fault_injection_log_path(home_dir: &Path) -> PathBuf {
    log_root(home_dir)
        .join("logs")
        .join("atm-fault-injection.log.jsonl")
}

fn retained_sink_fault_mode() -> Result<Option<RetainedSinkFaultMode>, AtmError> {
    // WARNING: production-reachable diagnostic seam. Phase L intentionally
    // keeps this env hook available so live degraded/unavailable validation
    // can exercise the real shared adapter before initial release.
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
    let injector = RetainedSinkFaultInjector::new();
    let sink = Arc::new(JsonlFileSink::new(
        fault_injection_log_path(home_dir),
        RotationPolicy::default(),
        RetentionPolicy::default(),
    ));
    builder.register_sink(SinkRegistration::new(injector.wrap(sink)));

    match mode {
        RetainedSinkFaultMode::Degraded => injector.force_degraded(),
        RetainedSinkFaultMode::Unavailable => injector.force_unavailable(),
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

    // emit path: builds sc_observability_types fields directly; exempt from
    // REQ-CORE-OBS-001 centralization per architect ruling (outputs to foreign type)
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
            serde_json::Value::String(task_id.clone()),
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
            tracing::warn!(
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
mod tests {
    use sc_observability_types::Level;

    use super::level_for_outcome;

    #[test]
    fn unknown_outcome_maps_to_warn() {
        assert_eq!(level_for_outcome("future-outcome"), Level::Warn);
    }

    #[test]
    fn level_for_outcome_matches_documented_outcomes() {
        let cases = [
            ("ok", Level::Info),
            ("sent", Level::Info),
            ("dry_run", Level::Info),
            ("timeout", Level::Warn),
            ("error", Level::Error),
            ("failed", Level::Error),
        ];

        for (outcome, expected) in cases {
            assert_eq!(level_for_outcome(outcome), expected, "outcome={outcome}");
        }
    }
}
