use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::observability::{
    self, AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, CommandEvent, LogTailSession,
    ObservabilityPort,
};

/// Structured CLI-owned observability construction options.
///
/// L.5 intentionally keeps the release surface narrow: one explicit
/// construction entry point without introducing a broader builder or unified
/// observer abstraction.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CliObservabilityOptions {
    pub stderr_logs: bool,
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
    pub(crate) fn from_boxed_port(inner: Box<dyn ObservabilityPort + Send + Sync>) -> Self {
        Self { inner }
    }

    pub fn fallback() -> Self {
        #[cfg(test)]
        if let Ok(observability) = Self::new(
            &std::env::temp_dir().join("atm-bootstrap-observability"),
            CliObservabilityOptions::default(),
        ) {
            return observability;
        }

        Self {
            inner: Box::new(atm_core::observability::NullObservability),
        }
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

    /// Test-only helper for injecting a synthetic observability port without
    /// exposing the boxed inner field to production callers.
    #[cfg(test)]
    fn from_test_port(port: impl ObservabilityPort + Send + Sync + 'static) -> Self {
        Self {
            inner: Box::new(port),
        }
    }

    #[cfg(test)]
    pub fn new(
        home_dir: &std::path::Path,
        options: CliObservabilityOptions,
    ) -> Result<Self, AtmError> {
        Ok(Self::from_boxed_port(crate::new_adapter_port_for_tests(
            home_dir,
            options.stderr_logs,
        )?))
    }
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

// L.5 dispositions:
// - UX-002 retained: boxed trait-object dispatch remains acceptable for
//   initial release because it keeps CLI bootstrap simple without forcing a
//   wider unified observer abstraction.
// - BP-001 retained: the sealed boundary remains in place so external crates
//   cannot bypass the intended ATM-owned adapter contract with arbitrary
//   ObservabilityPort impls.
// - UNI-003 retained as a defer decision: DoctorCommand injectability does not
//   participate in the ObservabilityPort contract; defer injectability to a
//   future sprint unless a concrete testing or feature need appears.
impl observability::sealed::Sealed for CliObservability {}

fn fatal_emit_failure_message(stage: &str, emit_error: &AtmError) -> String {
    format!("ATM fatal diagnostic emission failed during {stage}: {emit_error}")
}

#[cfg(test)]
mod tests {
    use atm_core::error::AtmError;
    use atm_core::observability::{
        AtmLogQuery, AtmObservabilityHealth, AtmObservabilityHealthState, CommandEvent,
        LogLevelFilter, LogMode, LogOrder, LogTailSession, ObservabilityPort,
    };
    use serial_test::serial;
    use tempfile::TempDir;

    use super::{CliObservability, CliObservabilityOptions, fatal_emit_failure_message};

    struct FailingEmitObservability;

    impl atm_core::observability::sealed::Sealed for FailingEmitObservability {}

    impl ObservabilityPort for FailingEmitObservability {
        fn emit(&self, _event: CommandEvent) -> Result<(), AtmError> {
            Err(AtmError::observability_emit("synthetic emit failure"))
        }

        fn query(
            &self,
            _req: AtmLogQuery,
        ) -> Result<atm_core::observability::AtmLogSnapshot, AtmError> {
            Ok(atm_core::observability::AtmLogSnapshot::default())
        }

        fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
            Ok(LogTailSession::empty())
        }

        fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
            Ok(AtmObservabilityHealth {
                active_log_path: None,
                logging_state: AtmObservabilityHealthState::Unavailable,
                query_state: Some(AtmObservabilityHealthState::Unavailable),
                detail: Some("synthetic".to_string()),
            })
        }
    }

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

    fn event(message_id: Option<&str>) -> CommandEvent {
        CommandEvent {
            command: "send",
            action: "send",
            outcome: "sent",
            team: "atm-dev".to_string(),
            agent: "arch-ctm".to_string(),
            sender: "arch-ctm".to_string(),
            message_id: message_id.map(|value| value.parse().expect("legacy message id")),
            requires_ack: false,
            dry_run: false,
            task_id: Some("TASK-1".parse().expect("task id")),
            error_code: None,
            error_message: None,
        }
    }

    #[test]
    #[serial]
    fn concrete_adapter_emits_queries_follows_and_reports_health() {
        let tempdir = TempDir::new().expect("tempdir");
        let observability =
            CliObservability::new(tempdir.path(), CliObservabilityOptions::default())
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
            initial.records[0]
                .fields
                .get("command")
                .and_then(atm_core::observability::LogFieldValue::as_str),
            Some("send")
        );

        let health = observability.health().expect("health");
        assert_eq!(health.logging_state, AtmObservabilityHealthState::Healthy);
        assert_eq!(
            health.query_state,
            Some(AtmObservabilityHealthState::Healthy)
        );
        assert_eq!(
            health.active_log_path,
            Some(
                tempdir
                    .path()
                    .join(".local")
                    .join("share")
                    .join("logs")
                    .join("atm.log.jsonl")
            )
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
                record
                    .fields
                    .get("message_id")
                    .and_then(atm_core::observability::LogFieldValue::as_str)
                    == Some("550e8400-e29b-41d4-a716-446655440001")
            }),
            "follow poll should include the newly emitted record even if the shared tail surface also returns the prior backlog entry"
        );
    }

    #[test]
    fn cli_observability_is_debuggable() {
        let observability =
            CliObservability::from_test_port(atm_core::observability::NullObservability);
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
        let observability = CliObservability::from_test_port(FailingEmitObservability);
        observability.emit_fatal_error("service", &AtmError::validation("boom"));
    }
}
