use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::home;
use atm_core::observability::{
    self, AtmLogQuery, AtmLogRecord, AtmLogSnapshot, AtmObservabilityHealth,
    AtmObservabilityHealthState, CommandEvent, LogFieldMatch, LogLevelFilter, LogOrder,
    LogTailSession, ObservabilityPort,
};
use chrono::{DateTime, Utc};
use sc_observability::{ConsoleSink, Logger, LoggerConfig, SinkRegistration};
use sc_observability_types::{
    ActionName, CorrelationId, DiagnosticInfo, Level, LogEvent, LogQuery, OutcomeLabel,
    ProcessIdentity, QueryError, SchemaVersion, ServiceName, TargetCategory, Timestamp,
};
use serde_json::Map;
use time::OffsetDateTime;

const ATM_SERVICE_NAME: &str = "atm";
const ATM_COMMAND_TARGET: &str = "atm.command";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConsoleLogRoute {
    Disabled,
    Stderr,
}
/// ATM CLI observability handle.
///
/// Clone is intentionally not derived; see rationale below.
///
/// `Clone` is intentionally not implemented because the concrete adapter owns a
/// boxed trait object without a shared-clone contract.
pub struct CliObservability {
    inner: Box<dyn ObservabilityPort + Send + Sync>,
}

impl std::fmt::Debug for CliObservability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CliObservability").finish_non_exhaustive()
    }
}

impl CliObservability {
    fn concrete_for_home(
        home_dir: &Path,
        console_log_route: ConsoleLogRoute,
    ) -> Result<Self, AtmError> {
        let adapter = ScObservabilityAdapter::new(home_dir, console_log_route)?;
        Ok(Self {
            inner: Box::new(adapter),
        })
    }

    pub fn fallback() -> Self {
        Self::concrete_for_home(
            &std::env::temp_dir().join("atm-bootstrap-observability"),
            ConsoleLogRoute::Disabled,
        )
        .unwrap_or_else(|_| Self {
            inner: Box::new(atm_core::observability::NullObservability),
        })
    }

    pub fn emit_fatal_error(&self, stage: &'static str, error: &(dyn std::error::Error + 'static)) {
        let (code, message) = if let Some(atm_error) = error.downcast_ref::<AtmError>() {
            (atm_error.code, atm_error.to_string())
        } else {
            (AtmErrorCode::MessageValidationFailed, error.to_string())
        };

        let identity = std::env::var("ATM_IDENTITY").unwrap_or_else(|_| "unknown".to_string());
        let team = std::env::var("ATM_TEAM").unwrap_or_else(|_| "unknown".to_string());
        if let Err(emit_error) = self.emit(CommandEvent {
            command: "atm",
            action: stage,
            outcome: "error",
            team,
            agent: identity.clone(),
            sender: identity,
            message_id: None,
            requires_ack: false,
            dry_run: false,
            task_id: None,
            error_code: Some(code),
            error_message: Some(message),
        }) {
            eprintln!("{}", fatal_emit_failure_message(stage, &emit_error));
        }
    }

    #[cfg(any(test, feature = "test-util"))]
    fn static_health(health: AtmObservabilityHealth) -> Self {
        Self {
            inner: Box::new(StaticHealthObservability { health }),
        }
    }
}

pub fn init(stderr_logs: bool) -> Result<CliObservability> {
    let home_dir = home::atm_home()?;
    #[cfg(any(test, feature = "test-util"))]
    if let Some(override_health) = test_health_override(&home_dir) {
        return Ok(override_health);
    }
    let console_log_route = if stderr_logs {
        ConsoleLogRoute::Stderr
    } else {
        ConsoleLogRoute::Disabled
    };
    Ok(CliObservability::concrete_for_home(
        &home_dir,
        console_log_route,
    )?)
}

impl ObservabilityPort for CliObservability {
    fn emit(&self, event: CommandEvent) -> Result<(), AtmError> {
        self.inner.emit(event)
    }

    fn query(&self, req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
        self.inner.query(req)
    }

    fn follow(&self, req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
        self.inner.follow(req)
    }

    fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
        self.inner.health()
    }
}

struct ScObservabilityAdapter {
    logger: Logger,
    service_name: ServiceName,
    target_category: TargetCategory,
}

#[cfg(any(test, feature = "test-util"))]
struct StaticHealthObservability {
    health: AtmObservabilityHealth,
}

impl observability::sealed::Sealed for CliObservability {}
impl observability::sealed::Sealed for ScObservabilityAdapter {}
#[cfg(any(test, feature = "test-util"))]
impl observability::sealed::Sealed for StaticHealthObservability {}

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

#[cfg(any(test, feature = "test-util"))]
impl ObservabilityPort for StaticHealthObservability {
    fn emit(&self, _event: CommandEvent) -> Result<(), AtmError> {
        Ok(())
    }

    fn query(&self, _req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
        Ok(AtmLogSnapshot::default())
    }

    fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
        Ok(LogTailSession::empty())
    }

    fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
        Ok(self.health.clone())
    }
}

fn log_root(home_dir: &Path) -> PathBuf {
    home_dir.join(".local").join("share")
}

fn fatal_emit_failure_message(stage: &str, emit_error: &AtmError) -> String {
    format!("ATM fatal diagnostic emission failed during {stage}: {emit_error}")
}

#[cfg(any(test, feature = "test-util"))]
fn test_health_override(home_dir: &Path) -> Option<CliObservability> {
    // Keep the CLI integration harness deterministic for doctor/log surfaces
    // without depending on induced file-sink failures inside sc-observability.
    let state = std::env::var("ATM_TEST_OBSERVABILITY_HEALTH").ok()?;
    let logging_state = match state.as_str() {
        "healthy" => AtmObservabilityHealthState::Healthy,
        "degraded" => AtmObservabilityHealthState::Degraded,
        "unavailable" => AtmObservabilityHealthState::Unavailable,
        _ => return None,
    };
    let query_state = std::env::var("ATM_TEST_OBSERVABILITY_QUERY_STATE")
        .ok()
        .as_deref()
        .and_then(parse_health_state)
        .unwrap_or(logging_state);
    let detail = std::env::var("ATM_TEST_OBSERVABILITY_DETAIL")
        .ok()
        .filter(|value| !value.is_empty());

    Some(CliObservability::static_health(AtmObservabilityHealth {
        active_log_path: Some(log_root(home_dir).join("logs").join("atm.log.jsonl")),
        logging_state,
        query_state: Some(query_state),
        detail,
    }))
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
    if field_match.key.trim().is_empty() {
        return Err(AtmError::observability_query(
            "ATM log field match key must not be empty",
        ));
    }

    Ok(sc_observability_types::LogFieldMatch::equals(
        field_match.key,
        field_match.value,
    ))
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
    Ok(AtmLogRecord {
        timestamp: map_timestamp_back(event.timestamp)?,
        severity: map_level_back(event.level),
        service: event.service.to_string(),
        target: Some(event.target.to_string()),
        action: Some(event.action.to_string()),
        message: event.message,
        fields: event.fields,
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

#[cfg(any(test, feature = "test-util"))]
fn parse_health_state(value: &str) -> Option<AtmObservabilityHealthState> {
    match value {
        "healthy" => Some(AtmObservabilityHealthState::Healthy),
        "degraded" => Some(AtmObservabilityHealthState::Degraded),
        "unavailable" => Some(AtmObservabilityHealthState::Unavailable),
        _ => None,
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
    use atm_core::error::AtmError;
    use atm_core::observability::{
        AtmLogQuery, LogLevelFilter, LogMode, LogOrder, ObservabilityPort,
    };
    use sc_observability_types::Level;
    use serial_test::serial;
    use tempfile::TempDir;

<<<<<<< ours
    use super::{CliObservability, ConsoleLogRoute, level_for_outcome, log_root};
||||||| base
    use super::{CliObservability, level_for_outcome, log_root};
=======
    use super::{CliObservability, fatal_emit_failure_message, level_for_outcome, log_root};

    struct FailingEmitObservability;

    impl atm_core::observability::sealed::Sealed for FailingEmitObservability {}

    impl ObservabilityPort for FailingEmitObservability {
        fn emit(&self, _event: atm_core::observability::CommandEvent) -> Result<(), AtmError> {
            Err(AtmError::observability_emit("synthetic emit failure"))
        }

        fn query(
            &self,
            _req: AtmLogQuery,
        ) -> Result<atm_core::observability::AtmLogSnapshot, AtmError> {
            Ok(atm_core::observability::AtmLogSnapshot::default())
        }

        fn follow(
            &self,
            _req: AtmLogQuery,
        ) -> Result<atm_core::observability::LogTailSession, AtmError> {
            Ok(atm_core::observability::LogTailSession::empty())
        }

        fn health(&self) -> Result<atm_core::observability::AtmObservabilityHealth, AtmError> {
            Ok(atm_core::observability::AtmObservabilityHealth {
                active_log_path: None,
                logging_state: atm_core::observability::AtmObservabilityHealthState::Unavailable,
                query_state: Some(
                    atm_core::observability::AtmObservabilityHealthState::Unavailable,
                ),
                detail: Some("synthetic".to_string()),
            })
        }
    }
>>>>>>> theirs

    fn query(order: LogOrder) -> AtmLogQuery {
        AtmLogQuery {
            mode: LogMode::Snapshot,
            levels: vec![LogLevelFilter::Info],
            field_matches: vec![],
            since: None,
            until: None,
            limit: None,
            order,
        }
    }

    fn event(message_id: Option<&str>) -> atm_core::observability::CommandEvent {
        atm_core::observability::CommandEvent {
            command: "send",
            action: "send",
            outcome: "sent",
            team: "atm-dev".to_string(),
            agent: "arch-ctm".to_string(),
            sender: "arch-ctm".to_string(),
            message_id: message_id.map(|value| value.parse().expect("legacy message id")),
            requires_ack: false,
            dry_run: false,
            task_id: Some("TASK-1".to_string()),
            error_code: None,
            error_message: None,
        }
    }

    #[test]
    #[serial]
    fn concrete_adapter_emits_queries_follows_and_reports_health() {
        let tempdir = TempDir::new().expect("tempdir");
        let observability =
            CliObservability::concrete_for_home(tempdir.path(), ConsoleLogRoute::Disabled)
                .expect("concrete adapter");

        observability
            .emit(event(Some("550e8400-e29b-41d4-a716-446655440000")))
            .expect("emit backlog");

        let initial = observability
            .query(query(LogOrder::OldestFirst))
            .expect("initial query");
        assert_eq!(initial.records.len(), 1);
        assert_eq!(initial.records[0].service, "atm");
        assert_eq!(initial.records[0].action.as_deref(), Some("send"));
        assert_eq!(
            initial.records[0].fields["command"],
            serde_json::Value::String("send".to_string())
        );

        let health = observability.health().expect("health");
        assert_eq!(
            health.logging_state,
            atm_core::observability::AtmObservabilityHealthState::Healthy
        );
        assert_eq!(
            health.query_state,
            Some(atm_core::observability::AtmObservabilityHealthState::Healthy)
        );
        assert_eq!(
            health.active_log_path,
            Some(log_root(tempdir.path()).join("logs").join("atm.log.jsonl"))
        );
        assert!(health.detail.is_none());

        let mut follow = observability
            .follow(AtmLogQuery {
                mode: LogMode::Tail,
                ..query(LogOrder::OldestFirst)
            })
            .expect("follow");
        observability
            .emit(event(Some("550e8400-e29b-41d4-a716-446655440001")))
            .expect("emit followed");

        let followed = follow.poll().expect("follow poll");
        assert!(
            followed.records.iter().any(|record| {
                record.fields.get("message_id")
                    == Some(&serde_json::Value::String(
                        "550e8400-e29b-41d4-a716-446655440001".to_string(),
                    ))
            }),
            "follow poll should include the newly emitted record even if the shared tail surface also returns the prior backlog entry"
        );
    }

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

    #[test]
    fn cli_observability_is_debuggable() {
        let observability = CliObservability {
            inner: Box::new(atm_core::observability::NullObservability),
        };
        let debug = format!("{observability:?}");
        assert!(debug.contains("CliObservability"));
    }

    #[test]
    fn fatal_emit_failure_message_mentions_stage_and_error() {
        let message = fatal_emit_failure_message(
            "service",
            &AtmError::observability_emit("synthetic emit failure"),
        );
        assert!(message.contains("ATM fatal diagnostic emission failed during service"));
        assert!(message.contains("synthetic emit failure"));
    }

    #[test]
    fn emit_fatal_error_executes_secondary_failure_path_without_panicking() {
        let observability = CliObservability {
            inner: Box::new(FailingEmitObservability),
        };
        observability.emit_fatal_error("service", &AtmError::validation("boom"));
    }
}
