use atm_core::caller_context::{CallerContextOverrides, resolve_cli_caller_context};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::observability::{
    AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, CommandEvent, LogTailSession,
    ObservabilityPort, action_name, outcome_label,
};
use atm_core::types::{AgentName, TeamName};

use crate::constants::ATM_SERVICE_NAME;
/// Structured CLI-owned observability construction options.
///
/// L.5 intentionally keeps the release surface narrow: one explicit
/// construction entry point without introducing a broader builder or unified
/// observer abstraction.
#[allow(
    unfulfilled_lint_expectations,
    reason = "This release-surface options type is live in normal builds even though the dead-code expectation remains documented for narrower test configurations."
)]
#[expect(
    dead_code,
    reason = "CliObservabilityOptions is a release-surface construction option even when some binaries do not exercise every field."
)]
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

    /// Test-bootstrap escape hatch; production paths must use
    /// `CliObservability::new`.
    pub fn fallback() -> Self {
        Self {
            inner: Box::new(atm_core::observability::NullObservability),
        }
    }

    pub fn report_fatal_error(
        &self,
        stage: &'static str,
        error: &(dyn std::error::Error + 'static),
    ) {
        let (code, message) = if let Some(atm_error) = error.downcast_ref::<AtmError>() {
            (atm_error.code(), atm_error.to_string())
        } else {
            (AtmErrorCode::InternalError, error.to_string())
        };

        let fallback_agent: AgentName = match "unknown".parse() {
            Ok(agent) => agent,
            Err(_) => return,
        };
        let fallback_team: TeamName = match "unknown".parse() {
            Ok(team) => team,
            Err(_) => return,
        };
        let caller_context = resolve_cli_caller_context(CallerContextOverrides::default()).ok();
        let agent = caller_context
            .as_ref()
            .map(|context| context.caller_identity.clone())
            .unwrap_or_else(|| fallback_agent.clone());
        let team = caller_context
            .map(|context| context.caller_team)
            .unwrap_or(fallback_team);
        if let Err(emit_error) = self.emit(CommandEvent {
            command: ATM_SERVICE_NAME,
            action: action_name(stage),
            outcome: outcome_label("error"),
            team,
            agent: agent.clone(),
            sender: agent,
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

    pub(crate) fn emit_command_event(&self, event: CommandEvent) {
        let command = event.command;
        let action = event.action.as_str().to_string();
        if let Err(emit_error) = self.emit(event) {
            eprintln!(
                "{}",
                command_emit_failure_message(command, &action, &emit_error)
            );
        }
    }

    /// Test-only helper for injecting a synthetic observability port without
    /// exposing the boxed inner field to production callers.
    #[cfg(test)]
    pub(crate) fn from_test_port(port: impl ObservabilityPort + Send + Sync + 'static) -> Self {
        Self {
            inner: Box::new(port),
        }
    }

    #[allow(
        unfulfilled_lint_expectations,
        reason = "The explicit constructor remains part of the production surface even when the dead-code expectation is not triggered in this build graph."
    )]
    #[expect(
        dead_code,
        reason = "CliObservability::new is retained as the explicit production constructor even when some tests bootstrap through alternate seams."
    )]
    pub fn new(
        home_dir: &std::path::Path,
        options: CliObservabilityOptions,
    ) -> Result<Self, AtmError> {
        Ok(Self::from_boxed_port(crate::new_adapter_port(
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
impl atm_core::boundary::sealed::Sealed for CliObservability {}

fn fatal_emit_failure_message(stage: &str, emit_error: &AtmError) -> String {
    format!("ATM fatal diagnostic emission failed during {stage}: {emit_error}")
}

fn command_emit_failure_message(command: &str, action: &str, emit_error: &AtmError) -> String {
    format!("ATM command observability emit failed for {command}/{action}: {emit_error}")
}

#[cfg(test)]
mod tests {
    use atm_core::error::AtmError;
    use atm_core::observability::{
        AtmLogQuery, AtmObservabilityHealth, AtmObservabilityHealthState, CommandEvent,
        LogLevelFilter, LogMode, LogOrder, LogTailSession, ObservabilityPort,
    };
    use atm_core::test_support::{EnvGuard, TEST_SENDER, TEST_TEAM};
    use serial_test::serial;
    use tempfile::TempDir;

    use super::{
        CliObservability, CliObservabilityOptions, command_emit_failure_message,
        fatal_emit_failure_message,
    };

    struct FailingEmitObservability;

    impl atm_core::boundary::sealed::Sealed for FailingEmitObservability {}

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
                maintenance: None,
                diagnostic: None,
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
            action: atm_core::observability::action_name("send"),
            outcome: atm_core::observability::outcome_label("sent"),
            team: TEST_TEAM.parse().expect("team"),
            agent: TEST_SENDER.parse().expect("agent"),
            sender: TEST_SENDER.parse().expect("agent"),
            message_id: message_id.map(|value| value.parse().expect("message id")),
            requires_ack: false,
            dry_run: false,
            task_id: Some("TASK-1".parse().expect("task id")),
            error_code: None,
            error_message: None,
        }
    }

    #[test]
    #[serial(env)]
    fn concrete_adapter_uses_host_scoped_default_log_path() {
        let tempdir = TempDir::new().expect("tempdir");
        let _env = EnvGuard::set_many([
            ("ATM_LOG", Some("info")),
            ("ATM_LOG_DIR", None),
            ("HOME", Some(tempdir.path().to_str().expect("utf8 path"))),
        ]);
        let observability =
            CliObservability::new(tempdir.path(), CliObservabilityOptions::default())
                .expect("concrete adapter");

        observability
            .emit(event(Some("01KRFK5QTF2R6NRS3Q0F8Z9K0S")))
            .expect("emit backlog");

        let health = observability.health().expect("health");
        assert_eq!(
            health.active_log_path,
            Some(
                tempdir
                    .path()
                    .join(".atm")
                    .join("logs")
                    .join("atm.log.jsonl")
            )
        );
    }

    #[test]
    #[serial(env)]
    fn concrete_adapter_emits_queries_follows_and_reports_health() {
        let tempdir = TempDir::new().expect("tempdir");
        let log_dir = tempdir.path().join(".atm").join("logs");
        let _env = EnvGuard::set_many([
            ("ATM_LOG", Some("info")),
            ("ATM_LOG_DIR", Some(log_dir.to_str().expect("utf8 path"))),
            ("HOME", Some(tempdir.path().to_str().expect("utf8 path"))),
        ]);
        let observability =
            CliObservability::new(tempdir.path(), CliObservabilityOptions::default())
                .expect("concrete adapter");

        observability
            .emit(event(Some("01KRFK5QTF2R6NRS3Q0F8Z9K0S")))
            .expect("emit backlog");

        let initial = observability
            .query(query(LogOrder::OldestFirst))
            .expect("initial query");
        assert_eq!(initial.records.len(), 1);
        assert_eq!(initial.records[0].service.as_str(), "atm");
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
        assert_eq!(health.active_log_path, Some(log_dir.join("atm.log.jsonl")));
        let detail = health
            .detail
            .as_deref()
            .expect("maintenance detail should be projected");
        assert!(detail.contains("maintenance state="));
        assert!(detail.contains("rotated_files_total="));
        assert!(detail.contains("pruned_files_total="));
        assert!(detail.contains("last_pass_at="));

        let mut follow = observability
            .follow(AtmLogQuery {
                mode: LogMode::Tail,
                ..query(LogOrder::OldestFirst)
            })
            .expect("follow");
        observability
            .emit(event(Some("01KRFK5QTF2R6NRS3Q0F8Z9K0T")))
            .expect("emit followed");

        let followed_message_id = "01KRFK5QTF2R6NRS3Q0F8Z9K0T"
            .parse::<atm_core::schema::AtmMessageId>()
            .expect("message id")
            .to_string();
        let followed = follow.poll().expect("follow poll");
        assert!(
            followed.records.iter().any(|record| {
                record
                    .fields
                    .get("message_id")
                    .and_then(atm_core::observability::LogFieldValue::as_str)
                    == Some(followed_message_id.as_str())
            }),
            "follow poll should include the newly emitted normalized message id even if the shared tail surface also returns the prior backlog entry"
        );
    }

    #[test]
    #[serial(env)]
    fn concrete_adapter_fails_closed_when_atm_log_dir_is_invalid() {
        let tempdir = TempDir::new().expect("tempdir");
        let _env = EnvGuard::set_many([
            ("ATM_LOG", Some("info")),
            ("ATM_LOG_DIR", Some("relative/logs")),
            ("HOME", Some(tempdir.path().to_str().expect("utf8 path"))),
        ]);

        let error = CliObservability::new(tempdir.path(), CliObservabilityOptions::default())
            .expect_err("invalid ATM_LOG_DIR should fail closed");
        assert!(error.is_config());
        assert!(error.message().contains("absolute path"));
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
    fn command_emit_failure_message_mentions_command_action_and_error() {
        let message = command_emit_failure_message(
            "send",
            "send",
            &AtmError::observability_emit("synthetic emit failure"),
        );
        assert!(message.contains("send/send"));
        assert!(message.contains("synthetic emit failure"));
    }

    #[test]
    fn emit_fatal_error_executes_secondary_failure_path_without_panicking() {
        let observability = CliObservability::from_test_port(FailingEmitObservability);
        observability.report_fatal_error("service", &AtmError::validation("boom"));
    }
}
