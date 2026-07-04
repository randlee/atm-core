use serde::Serialize;

use crate::config::AtmConfig;
use crate::delivery_plan::{
    DeliveryPlan, DeliveryPlanDisposition, DeliveryTarget, LogicalMessage, NotificationTarget,
};
use crate::delivery_policy::{
    DeliveryEventFamily, DeliveryHarnessPath, persisted_success_transition_names,
    sqlite_failure_transition_names,
};
use crate::error::AtmError;
use crate::observability::ObservabilityPort;
use crate::protocol::{NotificationEvent, NotificationKind};
use crate::schema::AtmMessageId;
use crate::send::WarningEntry;
use crate::service_runtime::RetainedServiceRuntime;
use crate::service_runtime::append_notification_log;
use crate::types::{AgentName, TaskId, TeamName};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeliveryExecutionDisposition {
    Delivered,
    #[allow(
        dead_code,
        reason = "Phase AD obsolete: historical Claude mailbox compatibility only."
    )]
    AppendDegraded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeliveryExecutionResult {
    pub(crate) disposition: DeliveryExecutionDisposition,
    pub(crate) warnings: Vec<WarningEntry>,
}

impl DeliveryExecutionResult {
    fn delivered() -> Self {
        Self {
            disposition: DeliveryExecutionDisposition::Delivered,
            warnings: Vec::new(),
        }
    }
}

pub(crate) struct DeliveryTransitionContext<'a> {
    pub(crate) family: DeliveryEventFamily,
    pub(crate) team: &'a TeamName,
    pub(crate) agent: &'a AgentName,
    pub(crate) sender: &'a AgentName,
    pub(crate) message_id: AtmMessageId,
    pub(crate) task_id: Option<TaskId>,
}

#[derive(Debug, Serialize)]
struct DeliveryNotificationDetail<'a> {
    sender: String,
    sender_team: Option<String>,
    message_id: String,
    requires_ack: bool,
    is_ack: bool,
    task_id: Option<String>,
    recipient_pane_id: Option<&'a str>,
}

pub(crate) fn deliver_notifications(
    warnings: &mut Vec<WarningEntry>,
    recipient: &crate::send::ResolvedRecipient,
    recipient_pane_id: Option<&str>,
    notifications: &[NotificationTarget],
) {
    for notification in notifications {
        let event = notification_event_from_target(recipient, recipient_pane_id, notification);
        if let Err(error) = append_notification_log(&event) {
            tracing::warn!(
                subsystem = "delivery_execution",
                action = "deliver_notifications",
                outcome = "failed",
                recipient = %recipient.agent,
                team = %recipient.team,
                %error,
                "notification delivery failed"
            );
            warnings.push(WarningEntry::new(
                format!(
                    "warning: notification delivery failed for {}@{} code={}: {error}",
                    recipient.agent,
                    recipient.team,
                    error.code.as_str(),
                ),
                error.primary_recovery().map(str::to_owned),
            ));
        }
    }
}

pub(crate) trait NonClaudeOutboundDeliveryWriter: crate::boundary::sealed::Sealed {
    fn deliver_non_claude_payloads(
        &self,
        recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
        messages: &[LogicalMessage],
    ) -> Result<(), AtmError>;
}

impl<T> NonClaudeOutboundDeliveryWriter for T
where
    T: RetainedServiceRuntime + crate::boundary::sealed::Sealed + ?Sized,
{
    fn deliver_non_claude_payloads(
        &self,
        recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
        messages: &[LogicalMessage],
    ) -> Result<(), AtmError> {
        // NonClaudeOutbound is still defined in terms of owned envelopes at
        // the retained runtime boundary, so this path clones until that
        // boundary contract changes.
        RetainedServiceRuntime::deliver_non_claude_payloads(
            self,
            recipient,
            &messages
                .iter()
                .map(|message| message.envelope.clone())
                .collect::<Vec<_>>(),
        )
    }
}

pub(crate) fn execute_delivery_plan<R>(
    runtime: &R,
    config: Option<&AtmConfig>,
    plan: &DeliveryPlan,
) -> Result<DeliveryExecutionResult, AtmError>
where
    R: NonClaudeOutboundDeliveryWriter,
{
    execute_messages(
        runtime,
        config,
        ExecutionView {
            delivery_target: &plan.delivery_target,
            recipient: &plan.recipient,
            recipient_pane_id: plan.recipient_pane_id.as_deref(),
            messages: &plan.messages,
            notifications: &plan.notifications,
        },
    )
}

pub(crate) use execute_delivery_plan as execute_reply_delivery_plan;

struct ExecutionView<'a> {
    delivery_target: &'a DeliveryTarget,
    recipient: &'a crate::send::ResolvedRecipient,
    recipient_pane_id: Option<&'a str>,
    messages: &'a [LogicalMessage],
    notifications: &'a [NotificationTarget],
}

fn execute_messages<R>(
    runtime: &R,
    _config: Option<&AtmConfig>,
    view: ExecutionView<'_>,
) -> Result<DeliveryExecutionResult, AtmError>
where
    R: NonClaudeOutboundDeliveryWriter,
{
    validate_delivery_target(view.delivery_target)?;
    let mut result = DeliveryExecutionResult::delivered();

    match view.delivery_target {
        DeliveryTarget::ClaudeCode { recipient, .. } => {
            return Err(retired_claude_delivery_target_error(recipient));
        }
        DeliveryTarget::NonClaude { recipient } => {
            runtime.deliver_non_claude_payloads(recipient, view.messages)?;
        }
    }

    deliver_notifications(
        &mut result.warnings,
        view.recipient,
        view.recipient_pane_id,
        view.notifications,
    );

    Ok(result)
}

fn notification_event_from_target(
    recipient: &crate::send::ResolvedRecipient,
    recipient_pane_id: Option<&str>,
    notification: &NotificationTarget,
) -> NotificationEvent {
    let detail = DeliveryNotificationDetail {
        sender: notification.sender.to_string(),
        sender_team: notification.sender_team.as_ref().map(ToString::to_string),
        message_id: notification.message_id.to_string(),
        requires_ack: notification.requires_ack,
        is_ack: notification.is_ack,
        task_id: notification.task_id.as_ref().map(ToString::to_string),
        recipient_pane_id,
    };
    NotificationEvent {
        kind: NotificationKind::Delivery,
        detail: serde_json::to_string(&detail)
            .expect("delivery notification detail must serialize to valid JSON"),
        team: Some(recipient.team.clone()),
        agent: Some(recipient.agent.clone()),
    }
}

pub(crate) fn emit_delivery_plan_transitions(
    observability: &dyn ObservabilityPort,
    context: DeliveryTransitionContext<'_>,
    plan: &DeliveryPlan,
    execution: &DeliveryExecutionResult,
) -> Result<(), AtmError> {
    emit_plan_transitions(
        observability,
        context,
        plan.disposition,
        plan.delivery_target.harness_path(),
        execution.disposition,
    )
}

pub(crate) use emit_delivery_plan_transitions as emit_reply_delivery_plan_transitions;

fn emit_plan_transitions(
    observability: &dyn ObservabilityPort,
    context: DeliveryTransitionContext<'_>,
    disposition: DeliveryPlanDisposition,
    harness: DeliveryHarnessPath,
    execution_disposition: DeliveryExecutionDisposition,
) -> Result<(), AtmError> {
    let transitions = match (disposition, execution_disposition, harness) {
        (DeliveryPlanDisposition::SqliteFailedRecovered, _, harness) => {
            sqlite_failure_transition_names(harness).to_vec()
        }
        (_, DeliveryExecutionDisposition::AppendDegraded, _) => {
            return Err(AtmError::validation(
                "append-degraded delivery is retired from the accepted runtime",
            )
            .with_recovery(
                "Route delivery through the non-Claude outbound boundary and remove any remaining Claude inbox-append assumptions.",
            ));
        }
        (_, DeliveryExecutionDisposition::Delivered, harness) => {
            persisted_success_transition_names(context.family, harness)
        }
    };
    for transition in transitions {
        observability.emit(crate::observability::CommandEvent {
            command: "delivery_policy",
            action: crate::observability::action_name(context.family.action_name()),
            outcome: crate::observability::outcome_label(transition),
            team: context.team.clone(),
            agent: context.agent.clone(),
            sender: context.sender.clone(),
            message_id: Some(context.message_id),
            requires_ack: false,
            dry_run: false,
            task_id: context.task_id.clone(),
            error_code: None,
            error_message: None,
        })?;
    }
    Ok(())
}

fn validate_delivery_target(target: &DeliveryTarget) -> Result<(), AtmError> {
    match target {
        DeliveryTarget::ClaudeCode { recipient, .. } => {
            Err(retired_claude_delivery_target_error(recipient))
        }
        DeliveryTarget::NonClaude { .. } => Ok(()),
    }
}

fn retired_claude_delivery_target_error(
    recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
) -> AtmError {
    AtmError::validation(format!(
        "retired delivery plan target: ClaudeCode target for {}@{}",
        recipient.agent, recipient.team
    ))
    .with_recovery(
        "Rebuild the delivery plan so accepted runtime delivery always routes through the non-Claude outbound boundary.",
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use serde_json::{Map, Value};

    use super::{
        DeliveryExecutionDisposition, DeliveryTransitionContext, NonClaudeOutboundDeliveryWriter,
        emit_delivery_plan_transitions, execute_delivery_plan,
    };
    use crate::delivery_plan::{
        DeliveryPlan, DeliveryPlanDisposition, DeliveryPlanKind, DeliveryTarget, LogicalMessage,
        NotificationTarget,
    };
    use crate::delivery_policy::{
        DeliveryEventFamily, DeliveryHarnessPath, DeliveryRecipientSnapshot,
    };
    use crate::error::AtmError;
    use crate::observability::{
        AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState,
        CommandEvent, LogTailSession, ObservabilityPort,
    };
    use crate::protocol::{NotificationEvent, NotificationKind};
    use crate::schema::{AtmMessageId, InboxMessage};
    use crate::send::ResolvedRecipient;
    use crate::test_support::{EnvGuard, TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, IsoTimestamp, TeamName};

    struct NoopRuntime;

    impl crate::boundary::sealed::Sealed for NoopRuntime {}

    impl NonClaudeOutboundDeliveryWriter for NoopRuntime {
        fn deliver_non_claude_payloads(
            &self,
            _recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
            _messages: &[LogicalMessage],
        ) -> Result<(), AtmError> {
            Ok(())
        }
    }

    // Mutex required: observability captures may be mutated from multiple test
    // threads when delivery execution is exercised under concurrent send flows.
    #[derive(Default)]
    struct RecordingObservability {
        events: std::sync::Mutex<Vec<CommandEvent>>,
    }

    impl crate::boundary::sealed::Sealed for RecordingObservability {}

    impl ObservabilityPort for RecordingObservability {
        fn emit(&self, event: CommandEvent) -> Result<(), AtmError> {
            self.events.lock().expect("events").push(event);
            Ok(())
        }

        fn query(&self, _req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
            Ok(AtmLogSnapshot::default())
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
                detail: Some("test observer".to_string()),
            })
        }
    }

    fn logical_message() -> LogicalMessage {
        logical_message_with_text("hello")
    }

    fn logical_message_with_text(text: &str) -> LogicalMessage {
        LogicalMessage::new(
            InboxMessage {
                from: AgentName::from_validated(TEST_SENDER),
                text: text.to_string(),
                timestamp: IsoTimestamp::now(),
                read: false,
                source_team: Some(TeamName::from_validated(TEST_TEAM)),
                summary: Some(text.to_string()),
                message_id: Some(AtmMessageId::new()),
                pending_ack_at: None,
                acknowledged_at: None,
                acknowledges_message_id: None,
                parent_message_id: None,
                thread_mode: None,
                expires_at: None,
                task_id: None,
                extra: Map::new(),
            },
            false,
            false,
        )
        .expect("logical message")
    }

    fn notification_detail(event: &NotificationEvent) -> Value {
        serde_json::from_str(&event.detail).expect("structured notification detail")
    }

    #[derive(Default)]
    struct RecordingRuntime {
        // The execution-path tests mutate these fields from trait-method calls
        // on `&self`, so the test double needs interior mutability to record
        // side effects without changing the production executor signatures.
        non_claude_delivery_texts: std::sync::Mutex<Vec<Vec<String>>>,
    }

    impl crate::boundary::sealed::Sealed for RecordingRuntime {}

    impl NonClaudeOutboundDeliveryWriter for RecordingRuntime {
        fn deliver_non_claude_payloads(
            &self,
            _recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
            messages: &[LogicalMessage],
        ) -> Result<(), AtmError> {
            self.non_claude_delivery_texts
                .lock()
                .expect("non-claude delivery texts")
                .push(
                    messages
                        .iter()
                        .map(|message| message.envelope.text.clone())
                        .collect(),
                );
            Ok(())
        }
    }

    fn recipient_snapshot(harness: DeliveryHarnessPath) -> DeliveryRecipientSnapshot {
        DeliveryRecipientSnapshot {
            agent: AgentName::from_validated("recipient"),
            team: TeamName::from_validated(TEST_TEAM),
            harness,
            recipient_pane_id: None,
            roster_backed: true,
        }
    }

    fn transition_context(message_id: AtmMessageId) -> DeliveryTransitionContext<'static> {
        static TEAM: std::sync::LazyLock<TeamName> =
            std::sync::LazyLock::new(|| TeamName::from_validated(TEST_TEAM));
        static AGENT: std::sync::LazyLock<AgentName> =
            std::sync::LazyLock::new(|| AgentName::from_validated("recipient"));
        static SENDER: std::sync::LazyLock<AgentName> =
            std::sync::LazyLock::new(|| AgentName::from_validated(TEST_SENDER));
        DeliveryTransitionContext {
            family: DeliveryEventFamily::NewMessage,
            team: &TEAM,
            agent: &AGENT,
            sender: &SENDER,
            message_id,
            task_id: None,
        }
    }

    fn notification_log_path(home_dir: &Path) -> PathBuf {
        crate::home::host_runtime_dir_from_home(home_dir).join("notifications.jsonl")
    }

    fn read_logged_notifications(home_dir: &Path) -> Vec<NotificationEvent> {
        fs::read_to_string(notification_log_path(home_dir))
            .expect("notifications")
            .lines()
            .map(|line| serde_json::from_str(line).expect("notification event"))
            .collect()
    }

    fn install_home_env(home_dir: &Path) -> EnvGuard {
        EnvGuard::set_many([
            ("HOME", Some(home_dir.to_str().expect("utf8 home"))),
            ("USERPROFILE", None),
            ("ATM_LOG_DIR", None),
        ])
    }

    #[test]
    fn execute_delivery_plan_rejects_claude_target_for_non_claude_harness() {
        let runtime = NoopRuntime;
        let message = logical_message();
        let plan = DeliveryPlan::new(
            DeliveryPlanKind::Send,
            DeliveryPlanDisposition::Persisted,
            DeliveryTarget::ClaudeCode {
                inbox_path: PathBuf::from("recipient.jsonl"),
                recipient: recipient_snapshot(DeliveryHarnessPath::NonClaude),
            },
            ResolvedRecipient {
                agent: AgentName::from_validated("recipient"),
                team: TeamName::from_validated(TEST_TEAM),
            },
            None,
            vec![message],
            Vec::new(),
        );

        let error = execute_delivery_plan(&runtime, None, &plan).expect_err("fail closed");
        assert!(error.is_validation());
        assert!(error.message.contains("retired delivery plan target"));
    }

    #[test]
    #[serial_test::serial(env)]
    fn execute_delivery_plan_allows_non_claude_target_for_claude_harness() {
        let runtime = NoopRuntime;
        let tempdir = tempfile::tempdir().expect("tempdir");
        let home_dir = tempdir.path().join("home");
        fs::create_dir_all(&home_dir).expect("home dir");
        let _env = install_home_env(&home_dir);
        let message = logical_message();
        let plan = DeliveryPlan::new(
            DeliveryPlanKind::Send,
            DeliveryPlanDisposition::Persisted,
            DeliveryTarget::NonClaude {
                recipient: recipient_snapshot(DeliveryHarnessPath::ClaudeCode),
            },
            ResolvedRecipient {
                agent: AgentName::from_validated("recipient"),
                team: TeamName::from_validated(TEST_TEAM),
            },
            None,
            vec![message],
            Vec::new(),
        );

        let result = execute_delivery_plan(&runtime, None, &plan).expect("non-claude delivery");
        assert_eq!(result.disposition, DeliveryExecutionDisposition::Delivered);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn emit_delivery_plan_transitions_rejects_non_claude_append_degraded_state() {
        let observability = RecordingObservability::default();
        let message = logical_message();
        let message_id = message.message_id();
        let plan = DeliveryPlan::new(
            DeliveryPlanKind::Send,
            DeliveryPlanDisposition::Persisted,
            DeliveryTarget::NonClaude {
                recipient: recipient_snapshot(DeliveryHarnessPath::NonClaude),
            },
            ResolvedRecipient {
                agent: AgentName::from_validated("recipient"),
                team: TeamName::from_validated(TEST_TEAM),
            },
            None,
            vec![message],
            Vec::new(),
        );

        let error = emit_delivery_plan_transitions(
            &observability,
            transition_context(message_id),
            &plan,
            &super::DeliveryExecutionResult {
                disposition: DeliveryExecutionDisposition::AppendDegraded,
                warnings: Vec::new(),
            },
        )
        .expect_err("reject impossible append-degraded non-claude transition");
        assert!(error.is_validation());
        assert!(
            error
                .message
                .contains("append-degraded delivery is retired")
        );
    }

    #[test]
    fn emit_reply_delivery_plan_transitions_rejects_non_claude_append_degraded_state() {
        let observability = RecordingObservability::default();
        let message = logical_message();
        let message_id = message.message_id();
        let plan = DeliveryPlan::new(
            DeliveryPlanKind::Reply,
            DeliveryPlanDisposition::Persisted,
            DeliveryTarget::NonClaude {
                recipient: recipient_snapshot(DeliveryHarnessPath::NonClaude),
            },
            ResolvedRecipient {
                agent: AgentName::from_validated("recipient"),
                team: TeamName::from_validated(TEST_TEAM),
            },
            None,
            vec![message],
            Vec::new(),
        );

        let error = super::emit_reply_delivery_plan_transitions(
            &observability,
            transition_context(message_id),
            &plan,
            &super::DeliveryExecutionResult {
                disposition: DeliveryExecutionDisposition::AppendDegraded,
                warnings: Vec::new(),
            },
        )
        .expect_err("reject impossible append-degraded non-claude reply transition");
        assert!(error.is_validation());
        assert!(
            error
                .message
                .contains("append-degraded delivery is retired")
        );
    }

    #[test]
    #[serial_test::serial(env)]
    fn delivery_notifications_append_directly_to_notification_log() {
        let runtime = RecordingRuntime::default();
        let tempdir = tempfile::tempdir().expect("tempdir");
        let home_dir = tempdir.path().join("home");
        fs::create_dir_all(&home_dir).expect("home dir");
        let _env = install_home_env(&home_dir);
        let message_id = AtmMessageId::new();
        let mut plan = DeliveryPlan::new(
            DeliveryPlanKind::Send,
            DeliveryPlanDisposition::Persisted,
            DeliveryTarget::NonClaude {
                recipient: recipient_snapshot(DeliveryHarnessPath::NonClaude),
            },
            ResolvedRecipient {
                agent: AgentName::from_validated("recipient"),
                team: TeamName::from_validated(TEST_TEAM),
            },
            Some(crate::types::PaneId::new("pane-1").expect("pane")),
            vec![logical_message()],
            Vec::new(),
        );
        plan.notifications = vec![NotificationTarget {
            sender: AgentName::from_validated(TEST_SENDER),
            sender_team: Some(TeamName::from_validated(TEST_TEAM)),
            message_id,
            requires_ack: true,
            is_ack: false,
            task_id: Some("task-123".parse().expect("task id")),
        }];

        let result = execute_delivery_plan(&runtime, None, &plan).expect("delivery");
        assert!(result.warnings.is_empty());
        let events = read_logged_notifications(&home_dir);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, NotificationKind::Delivery);
        assert_eq!(
            events[0].team.as_ref().map(TeamName::as_str),
            Some(TEST_TEAM)
        );
        assert_eq!(
            events[0].agent.as_ref().map(AgentName::as_str),
            Some("recipient")
        );
        let detail = notification_detail(&events[0]);
        assert_eq!(
            detail.get("sender").and_then(Value::as_str),
            Some(TEST_SENDER)
        );
        assert_eq!(
            detail.get("sender_team").and_then(Value::as_str),
            Some(TEST_TEAM)
        );
        assert_eq!(
            detail.get("requires_ack").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            detail.get("task_id").and_then(Value::as_str),
            Some("task-123")
        );
        assert_eq!(
            detail.get("recipient_pane_id").and_then(Value::as_str),
            Some("pane-1")
        );
    }

    #[test]
    #[serial_test::serial(env)]
    fn notification_log_failure_is_explicit_in_delivery_warnings() {
        let runtime = RecordingRuntime::default();
        let tempdir = tempfile::tempdir().expect("tempdir");
        let blocking_home = tempdir.path().join("home-file");
        fs::write(&blocking_home, "not a directory").expect("blocking home");
        let _env = install_home_env(&blocking_home);
        let mut plan = DeliveryPlan::new(
            DeliveryPlanKind::Send,
            DeliveryPlanDisposition::Persisted,
            DeliveryTarget::NonClaude {
                recipient: recipient_snapshot(DeliveryHarnessPath::NonClaude),
            },
            ResolvedRecipient {
                agent: AgentName::from_validated("recipient"),
                team: TeamName::from_validated(TEST_TEAM),
            },
            None,
            vec![logical_message()],
            Vec::new(),
        );
        plan.notifications = vec![NotificationTarget {
            sender: AgentName::from_validated(TEST_SENDER),
            sender_team: Some(TeamName::from_validated(TEST_TEAM)),
            message_id: AtmMessageId::new(),
            requires_ack: false,
            is_ack: false,
            task_id: None,
        }];

        let result = execute_delivery_plan(&runtime, None, &plan).expect("delivery");
        assert_eq!(result.warnings.len(), 1);
        assert!(
            result.warnings[0]
                .message
                .contains("warning: notification delivery failed for recipient@test-team")
        );
        assert!(
            result.warnings[0]
                .message
                .contains("code=ATM_MAILBOX_WRITE_FAILED")
        );
        let recovery = result.warnings[0]
            .recovery
            .as_deref()
            .expect("notification recovery");
        assert!(recovery.contains("writable"));
    }

    #[test]
    #[serial_test::serial(env)]
    fn notification_log_failure_does_not_reopen_hook_helper_bypass() {
        let runtime = RecordingRuntime::default();
        let tempdir = tempfile::tempdir().expect("tempdir");
        let blocking_home = tempdir.path().join("home-file");
        fs::write(&blocking_home, "not a directory").expect("blocking home");
        let _env = install_home_env(&blocking_home);
        let mut plan = DeliveryPlan::new(
            DeliveryPlanKind::Send,
            DeliveryPlanDisposition::Persisted,
            DeliveryTarget::NonClaude {
                recipient: recipient_snapshot(DeliveryHarnessPath::NonClaude),
            },
            ResolvedRecipient {
                agent: AgentName::from_validated("recipient"),
                team: TeamName::from_validated(TEST_TEAM),
            },
            None,
            vec![logical_message()],
            Vec::new(),
        );
        plan.notifications = vec![NotificationTarget {
            sender: AgentName::from_validated(TEST_SENDER),
            sender_team: Some(TeamName::from_validated(TEST_TEAM)),
            message_id: AtmMessageId::new(),
            requires_ack: false,
            is_ack: false,
            task_id: None,
        }];

        let result = execute_delivery_plan(&runtime, None, &plan).expect("delivery");
        assert_eq!(result.disposition, DeliveryExecutionDisposition::Delivered);
        assert_eq!(
            *runtime
                .non_claude_delivery_texts
                .lock()
                .expect("non-claude delivery texts"),
            vec![vec!["hello".to_string()]]
        );
        assert_eq!(result.warnings.len(), 1);
        assert!(
            result.warnings[0]
                .message
                .contains("notification delivery failed")
        );
    }
}
