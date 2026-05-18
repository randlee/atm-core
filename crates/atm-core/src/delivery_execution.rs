use std::path::Path;

use crate::boundary::ClaudeCompatibilityDeliveryMode;
use crate::config::AtmConfig;
use crate::delivery_plan::{
    DeliveryPlan, DeliveryPlanDisposition, DeliveryTarget, LogicalMessage, NotificationTarget,
    ReplyDeliveryPlan,
};
use crate::delivery_policy::{
    DeliveryEventFamily, DeliveryHarnessPath, claude_append_failure_transition_names,
    persisted_success_transition_names, sqlite_failure_transition_names,
};
use crate::error::AtmError;
use crate::observability::ObservabilityPort;
use crate::schema::{AtmMessageId, MessageEnvelope};
use crate::send::{PostSendHookContext, WarningEntry};
use crate::service_runtime::RetainedServiceRuntime;
use crate::types::{AgentName, TaskId, TeamName};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeliveryExecutionDisposition {
    Delivered,
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

pub(crate) trait ClaudeInboxWriter {
    fn append_claude_inbox_message(
        &self,
        inbox_path: &Path,
        recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
        message: &MessageEnvelope,
    ) -> Result<(), AtmError>;
    fn append_claude_message_set(
        &self,
        inbox_path: &Path,
        recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
        disposition: DeliveryPlanDisposition,
        messages: &[LogicalMessage],
    ) -> Result<(), AtmError>;
}

impl<T> ClaudeInboxWriter for T
where
    T: RetainedServiceRuntime + ?Sized,
{
    fn append_claude_inbox_message(
        &self,
        inbox_path: &Path,
        _recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
        message: &MessageEnvelope,
    ) -> Result<(), AtmError> {
        self.append_compat_inbox_message(inbox_path, message)
    }

    fn append_claude_message_set(
        &self,
        inbox_path: &Path,
        _recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
        disposition: DeliveryPlanDisposition,
        messages: &[LogicalMessage],
    ) -> Result<(), AtmError> {
        let mode = claude_compatibility_delivery_mode_for_disposition(disposition)?;
        self.append_compat_inbox_message_set(
            inbox_path,
            mode,
            &messages
                .iter()
                .map(|message| message.envelope.clone())
                .collect::<Vec<_>>(),
        )
    }
}

pub(crate) trait PostSendNotificationExecutor {
    fn execute_post_send_notification(
        &self,
        warnings: &mut Vec<WarningEntry>,
        config: Option<&AtmConfig>,
        recipient: &crate::send::ResolvedRecipient,
        recipient_pane_id: Option<&str>,
        notification: &NotificationTarget,
    );
}

impl<T> PostSendNotificationExecutor for T
where
    T: RetainedServiceRuntime + ?Sized,
{
    fn execute_post_send_notification(
        &self,
        warnings: &mut Vec<WarningEntry>,
        config: Option<&AtmConfig>,
        recipient: &crate::send::ResolvedRecipient,
        recipient_pane_id: Option<&str>,
        notification: &NotificationTarget,
    ) {
        self.maybe_run_post_send_hook(
            warnings,
            config,
            PostSendHookContext {
                sender: &notification.sender,
                sender_team: notification.sender_team.as_ref(),
                recipient,
                recipient_pane_id,
                message_id: notification.message_id,
                requires_ack: notification.requires_ack,
                is_ack: notification.is_ack,
                task_id: notification.task_id.as_ref(),
            },
        );
    }
}

pub(crate) trait NonClaudeOutboundDeliveryWriter {
    fn deliver_non_claude_payloads(
        &self,
        recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
        messages: &[LogicalMessage],
    ) -> Result<(), AtmError>;
}

impl<T> NonClaudeOutboundDeliveryWriter for T
where
    T: RetainedServiceRuntime + ?Sized,
{
    fn deliver_non_claude_payloads(
        &self,
        recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
        messages: &[LogicalMessage],
    ) -> Result<(), AtmError> {
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
    R: ClaudeInboxWriter + NonClaudeOutboundDeliveryWriter + PostSendNotificationExecutor,
{
    execute_messages(
        runtime,
        config,
        ExecutionView {
            disposition: plan.disposition,
            delivery_target: &plan.delivery_target,
            recipient: &plan.recipient,
            recipient_pane_id: plan.recipient_pane_id.as_deref(),
            messages: &plan.messages,
            notifications: &plan.notifications,
        },
    )
}

pub(crate) fn execute_reply_delivery_plan<R>(
    runtime: &R,
    config: Option<&AtmConfig>,
    plan: &ReplyDeliveryPlan,
) -> Result<DeliveryExecutionResult, AtmError>
where
    R: ClaudeInboxWriter + NonClaudeOutboundDeliveryWriter + PostSendNotificationExecutor,
{
    execute_messages(
        runtime,
        config,
        ExecutionView {
            disposition: plan.disposition,
            delivery_target: &plan.delivery_target,
            recipient: &plan.recipient,
            recipient_pane_id: plan.recipient_pane_id.as_deref(),
            messages: &plan.messages,
            notifications: &plan.notifications,
        },
    )
}

struct ExecutionView<'a> {
    disposition: DeliveryPlanDisposition,
    delivery_target: &'a DeliveryTarget,
    recipient: &'a crate::send::ResolvedRecipient,
    recipient_pane_id: Option<&'a str>,
    messages: &'a [LogicalMessage],
    notifications: &'a [NotificationTarget],
}

fn execute_messages<R>(
    runtime: &R,
    config: Option<&AtmConfig>,
    view: ExecutionView<'_>,
) -> Result<DeliveryExecutionResult, AtmError>
where
    R: ClaudeInboxWriter + NonClaudeOutboundDeliveryWriter + PostSendNotificationExecutor,
{
    validate_delivery_target(view.delivery_target)?;
    let mut result = DeliveryExecutionResult::delivered();

    match view.delivery_target {
        DeliveryTarget::ClaudeCode {
            inbox_path,
            recipient: snapshot,
        } => execute_claude_delivery(
            runtime,
            view.disposition,
            inbox_path,
            snapshot,
            view.messages,
            &mut result,
        )?,
        DeliveryTarget::NonClaude { recipient } => {
            runtime.deliver_non_claude_payloads(recipient, view.messages)?;
        }
    }

    for notification in view.notifications {
        runtime.execute_post_send_notification(
            &mut result.warnings,
            config,
            view.recipient,
            view.recipient_pane_id,
            notification,
        );
    }

    Ok(result)
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

pub(crate) fn emit_reply_delivery_plan_transitions(
    observability: &dyn ObservabilityPort,
    context: DeliveryTransitionContext<'_>,
    plan: &ReplyDeliveryPlan,
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
        (
            DeliveryPlanDisposition::Persisted,
            DeliveryExecutionDisposition::AppendDegraded,
            DeliveryHarnessPath::ClaudeCode,
        ) => claude_append_failure_transition_names().to_vec(),
        (
            DeliveryPlanDisposition::Persisted,
            DeliveryExecutionDisposition::AppendDegraded,
            DeliveryHarnessPath::NonClaude,
        ) => {
            return Err(AtmError::validation(
                "append-degraded delivery is unsupported for DeliveryHarnessPath::NonClaude",
            )
            .with_recovery(
                "Route non-Claude delivery through the state-machine-owned non-Claude outbound path instead of attempting Claude compatibility append semantics.",
            ));
        }
        (_, DeliveryExecutionDisposition::Delivered, harness) => {
            persisted_success_transition_names(context.family, harness)
        }
    };
    for transition in transitions {
        observability.emit(crate::observability::CommandEvent {
            command: "delivery_policy",
            action: context.family.action_name(),
            outcome: transition,
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
    match (target, target.recipient_snapshot().harness) {
        (DeliveryTarget::ClaudeCode { .. }, DeliveryHarnessPath::ClaudeCode)
        | (DeliveryTarget::NonClaude { .. }, DeliveryHarnessPath::NonClaude) => Ok(()),
        (DeliveryTarget::ClaudeCode { recipient, .. }, DeliveryHarnessPath::NonClaude) => {
            Err(AtmError::validation(format!(
                "unsupported delivery plan target: ClaudeCode target for non-Claude harness {}@{}",
                recipient.agent, recipient.team
            ))
            .with_recovery(
                "Build the delivery plan through the state machine so non-Claude recipients stay on the non-Claude outbound path.",
            ))
        }
        (DeliveryTarget::NonClaude { recipient }, DeliveryHarnessPath::ClaudeCode) => {
            Err(AtmError::validation(format!(
                "unsupported delivery plan target: NonClaude target for Claude Code harness {}@{}",
                recipient.agent, recipient.team
            ))
            .with_recovery(
                "Build the delivery plan through the state machine so Claude Code recipients stay on the compatibility inbox append path.",
            ))
        }
    }
}

fn execute_claude_delivery<R: ClaudeInboxWriter + ?Sized>(
    runtime: &R,
    disposition: DeliveryPlanDisposition,
    inbox_path: &Path,
    recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
    messages: &[LogicalMessage],
    result: &mut DeliveryExecutionResult,
) -> Result<(), AtmError> {
    if disposition == DeliveryPlanDisposition::SqliteFailedRecovered {
        runtime.append_claude_message_set(inbox_path, recipient, disposition, messages)?;
        return Ok(());
    }

    for (index, message) in messages.iter().enumerate() {
        if let Err(error) =
            runtime.append_claude_inbox_message(inbox_path, recipient, &message.envelope)
        {
            result.disposition = DeliveryExecutionDisposition::AppendDegraded;
            result
                .warnings
                .push(build_append_warning(disposition, recipient, index, error));
        }
    }
    Ok(())
}

fn claude_compatibility_delivery_mode_for_disposition(
    disposition: DeliveryPlanDisposition,
) -> Result<ClaudeCompatibilityDeliveryMode, AtmError> {
    match disposition {
        DeliveryPlanDisposition::SqliteFailedRecovered => {
            Ok(ClaudeCompatibilityDeliveryMode::RecoveredLogicalMessageSet)
        }
        _ => Err(AtmError::validation(
            "claude_compatibility_delivery_mode_for_disposition only accepts DeliveryPlanDisposition::SqliteFailedRecovered",
        )
        .with_recovery(
            "Call the recovered Claude message-set seam only for SqliteFailedRecovered delivery plans; persisted Claude delivery stays on the single-message append path.",
        )),
    }
}

fn build_append_warning(
    disposition: DeliveryPlanDisposition,
    recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
    index: usize,
    error: AtmError,
) -> WarningEntry {
    let ordinal = index + 1;
    let (message, recovery) = if disposition == DeliveryPlanDisposition::Persisted {
        (
            format!(
                "warning: compatibility append degraded for {}@{} message[{ordinal}]: {error}",
                recipient.agent, recipient.team
            ),
            Some(
                "SQLite persistence succeeded. Post-send-hook fallback remains available for notification degradation.",
            ),
        )
    } else {
        (
            format!(
                "error: degraded Claude Code delivery append failed for {}@{} message[{ordinal}] after SQLite failure: {error}",
                recipient.agent, recipient.team
            ),
            Some(
                "ATM still executed the shared notification path, but degraded delivery is incomplete and the Claude Code compatibility surface must be repaired immediately.",
            ),
        )
    };
    WarningEntry::new(message, recovery)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use serde_json::Map;

    use super::{
        ClaudeInboxWriter, DeliveryExecutionDisposition, DeliveryTransitionContext,
        NonClaudeOutboundDeliveryWriter, PostSendNotificationExecutor,
        emit_delivery_plan_transitions, execute_delivery_plan,
    };
    use crate::config::AtmConfig;
    use crate::delivery_plan::{
        DeliveryPlan, DeliveryPlanDisposition, DeliveryTarget, LogicalMessage,
    };
    use crate::delivery_policy::{
        DeliveryEventFamily, DeliveryHarnessPath, DeliveryRecipientSnapshot,
    };
    use crate::error::AtmError;
    use crate::observability::{
        AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState,
        CommandEvent, LogTailSession, ObservabilityPort,
    };
    use crate::schema::{AtmMessageId, MessageEnvelope};
    use crate::send::{ResolvedRecipient, WarningEntry};
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, IsoTimestamp, TeamName};

    struct NoopRuntime;

    impl ClaudeInboxWriter for NoopRuntime {
        fn append_claude_inbox_message(
            &self,
            _inbox_path: &Path,
            _recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
            _message: &MessageEnvelope,
        ) -> Result<(), AtmError> {
            Ok(())
        }

        fn append_claude_message_set(
            &self,
            _inbox_path: &Path,
            _recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
            _disposition: DeliveryPlanDisposition,
            _messages: &[LogicalMessage],
        ) -> Result<(), AtmError> {
            Ok(())
        }
    }

    impl PostSendNotificationExecutor for NoopRuntime {
        fn execute_post_send_notification(
            &self,
            _warnings: &mut Vec<WarningEntry>,
            _config: Option<&AtmConfig>,
            _recipient: &ResolvedRecipient,
            _recipient_pane_id: Option<&str>,
            _notification: &crate::delivery_plan::NotificationTarget,
        ) {
        }
    }

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
                detail: Some("test observer".to_string()),
            })
        }
    }

    fn logical_message() -> LogicalMessage {
        logical_message_with_text("hello")
    }

    fn logical_message_with_text(text: &str) -> LogicalMessage {
        LogicalMessage::new(
            MessageEnvelope {
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

    #[derive(Default)]
    struct RecordingRuntime {
        single_append_texts: std::sync::Mutex<Vec<String>>,
        message_set_texts: std::sync::Mutex<Vec<Vec<String>>>,
        fail_message_set: bool,
        fail_single_append_indexes: std::sync::Mutex<Vec<usize>>,
        single_append_calls: std::sync::Mutex<usize>,
    }

    impl RecordingRuntime {
        fn with_message_set_failure() -> Self {
            Self {
                fail_message_set: true,
                ..Self::default()
            }
        }

        fn with_single_append_failures(indexes: &[usize]) -> Self {
            Self {
                fail_single_append_indexes: std::sync::Mutex::new(indexes.to_vec()),
                ..Self::default()
            }
        }
    }

    impl ClaudeInboxWriter for RecordingRuntime {
        fn append_claude_inbox_message(
            &self,
            _inbox_path: &Path,
            _recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
            message: &MessageEnvelope,
        ) -> Result<(), AtmError> {
            let mut call_count = self.single_append_calls.lock().expect("call count");
            let current_index = *call_count;
            *call_count += 1;

            if self
                .fail_single_append_indexes
                .lock()
                .expect("fail indexes")
                .contains(&current_index)
            {
                return Err(AtmError::mailbox_write(format!(
                    "single append failure for message[{}]",
                    current_index + 1
                )));
            }

            self.single_append_texts
                .lock()
                .expect("single appends")
                .push(message.text.clone());
            Ok(())
        }

        fn append_claude_message_set(
            &self,
            _inbox_path: &Path,
            _recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
            _disposition: DeliveryPlanDisposition,
            messages: &[LogicalMessage],
        ) -> Result<(), AtmError> {
            self.message_set_texts.lock().expect("message sets").push(
                messages
                    .iter()
                    .map(|message| message.envelope.text.clone())
                    .collect(),
            );
            if self.fail_message_set {
                return Err(AtmError::mailbox_write(
                    "recovered Claude logical message set export failed",
                ));
            }
            Ok(())
        }
    }

    impl PostSendNotificationExecutor for RecordingRuntime {
        fn execute_post_send_notification(
            &self,
            _warnings: &mut Vec<WarningEntry>,
            _config: Option<&AtmConfig>,
            _recipient: &ResolvedRecipient,
            _recipient_pane_id: Option<&str>,
            _notification: &crate::delivery_plan::NotificationTarget,
        ) {
        }
    }

    impl NonClaudeOutboundDeliveryWriter for RecordingRuntime {
        fn deliver_non_claude_payloads(
            &self,
            _recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
            _messages: &[LogicalMessage],
        ) -> Result<(), AtmError> {
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
        let team = Box::leak(Box::new(TeamName::from_validated(TEST_TEAM)));
        let agent = Box::leak(Box::new(AgentName::from_validated("recipient")));
        let sender = Box::leak(Box::new(AgentName::from_validated(TEST_SENDER)));
        DeliveryTransitionContext {
            family: DeliveryEventFamily::NewMessage,
            team,
            agent,
            sender,
            message_id,
            task_id: None,
        }
    }

    #[test]
    fn execute_delivery_plan_rejects_claude_target_for_non_claude_harness() {
        let runtime = NoopRuntime;
        let message = logical_message();
        let plan = DeliveryPlan::new(
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
        assert!(
            error
                .message
                .contains("ClaudeCode target for non-Claude harness")
        );
    }

    #[test]
    fn emit_delivery_plan_transitions_rejects_non_claude_append_degraded_state() {
        let observability = RecordingObservability::default();
        let message = logical_message();
        let message_id = message.message_id();
        let plan = DeliveryPlan::new(
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
                .contains("unsupported for DeliveryHarnessPath::NonClaude")
        );
    }

    #[test]
    fn sqlite_failure_for_claude_requires_full_logical_message_set_delivery() {
        let runtime = RecordingRuntime::with_message_set_failure();
        let plan = DeliveryPlan::new(
            DeliveryPlanDisposition::SqliteFailedRecovered,
            DeliveryTarget::ClaudeCode {
                inbox_path: PathBuf::from("recipient.jsonl"),
                recipient: recipient_snapshot(DeliveryHarnessPath::ClaudeCode),
            },
            ResolvedRecipient {
                agent: AgentName::from_validated("recipient"),
                team: TeamName::from_validated(TEST_TEAM),
            },
            None,
            vec![
                logical_message_with_text("message[1]"),
                logical_message_with_text("message[2]"),
            ],
            Vec::new(),
        );

        let error = execute_delivery_plan(&runtime, None, &plan).expect_err("hard failure");
        assert!(error.message.contains("logical message set export failed"));
        assert_eq!(
            runtime.message_set_texts.lock().expect("message sets").len(),
            1
        );
        assert!(
            runtime
                .single_append_texts
                .lock()
                .expect("single appends")
                .is_empty()
        );
    }

    #[test]
    fn sqlite_failure_for_claude_does_not_emit_message1_without_message2() {
        let runtime = RecordingRuntime::with_message_set_failure();
        let plan = DeliveryPlan::new(
            DeliveryPlanDisposition::SqliteFailedRecovered,
            DeliveryTarget::ClaudeCode {
                inbox_path: PathBuf::from("recipient.jsonl"),
                recipient: recipient_snapshot(DeliveryHarnessPath::ClaudeCode),
            },
            ResolvedRecipient {
                agent: AgentName::from_validated("recipient"),
                team: TeamName::from_validated(TEST_TEAM),
            },
            None,
            vec![
                logical_message_with_text("original message"),
                logical_message_with_text("companion error"),
            ],
            Vec::new(),
        );

        let _ = execute_delivery_plan(&runtime, None, &plan).expect_err("hard failure");
        assert_eq!(
            *runtime.message_set_texts.lock().expect("message sets"),
            vec![vec![
                "original message".to_string(),
                "companion error".to_string()
            ]]
        );
        assert!(
            runtime
                .single_append_texts
                .lock()
                .expect("single appends")
                .is_empty()
        );
    }

    #[test]
    fn persisted_claude_append_degradation_remains_explicit_and_warning_typed() {
        let runtime = RecordingRuntime::with_single_append_failures(&[0]);
        let plan = DeliveryPlan::new(
            DeliveryPlanDisposition::Persisted,
            DeliveryTarget::ClaudeCode {
                inbox_path: PathBuf::from("recipient.jsonl"),
                recipient: recipient_snapshot(DeliveryHarnessPath::ClaudeCode),
            },
            ResolvedRecipient {
                agent: AgentName::from_validated("recipient"),
                team: TeamName::from_validated(TEST_TEAM),
            },
            None,
            vec![
                logical_message_with_text("persisted first"),
                logical_message_with_text("persisted second"),
            ],
            Vec::new(),
        );

        let result = execute_delivery_plan(&runtime, None, &plan).expect("append degraded");
        assert_eq!(
            result.disposition,
            DeliveryExecutionDisposition::AppendDegraded
        );
        assert_eq!(result.warnings.len(), 1);
        assert!(
            result.warnings[0]
                .message
                .contains("warning: compatibility append degraded")
        );
        assert_eq!(
            result.warnings[0].recovery.as_deref(),
            Some(
                "SQLite persistence succeeded. Post-send-hook fallback remains available for notification degradation."
            )
        );
        assert_eq!(
            *runtime.single_append_texts.lock().expect("single appends"),
            vec!["persisted second".to_string()]
        );
    }
}
