use crate::config::AtmConfig;
use crate::delivery_plan::{DeliveryPlan, DeliveryPlanDisposition, LogicalMessage};
use crate::delivery_policy::{
    DeliveryEventFamily, persisted_success_transition_names, sqlite_failure_transition_names,
};
use crate::error::AtmError;
use crate::observability::ObservabilityPort;
use crate::schema::AtmMessageId;
use crate::send::WarningEntry;
use crate::service_runtime::RetainedServiceRuntime;
use crate::types::{AgentName, TaskId, TeamName};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeliveryExecutionDisposition {
    Delivered,
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
    _config: Option<&AtmConfig>,
    plan: &DeliveryPlan,
) -> Result<DeliveryExecutionResult, AtmError>
where
    R: NonClaudeOutboundDeliveryWriter,
{
    let crate::delivery_plan::DeliveryTarget::NonClaude { recipient } = &plan.delivery_target;
    runtime.deliver_non_claude_payloads(recipient, &plan.messages)?;
    Ok(DeliveryExecutionResult::delivered())
}

pub(crate) fn emit_delivery_plan_transitions(
    observability: &dyn ObservabilityPort,
    context: DeliveryTransitionContext<'_>,
    plan: &DeliveryPlan,
    _execution: &DeliveryExecutionResult,
) -> Result<(), AtmError> {
    let transitions = match plan.disposition {
        DeliveryPlanDisposition::SqliteFailedRecovered => {
            sqlite_failure_transition_names(plan.delivery_target.harness_path()).to_vec()
        }
        DeliveryPlanDisposition::Persisted => {
            persisted_success_transition_names(context.family, plan.delivery_target.harness_path())
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

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::{
        DeliveryExecutionDisposition, DeliveryTransitionContext, NonClaudeOutboundDeliveryWriter,
        emit_delivery_plan_transitions, execute_delivery_plan,
    };
    use crate::delivery_plan::{
        DeliveryPlan, DeliveryPlanDisposition, DeliveryPlanKind, DeliveryTarget, LogicalMessage,
    };
    use crate::delivery_policy::{
        DeliveryEventFamily, DeliveryHarnessPath, DeliveryRecipientSnapshot,
    };
    use crate::error::AtmError;
    use crate::observability::{
        AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState,
        CommandEvent, LogTailSession, ObservabilityPort,
    };
    use crate::schema::{AtmMessageId, InboxMessage};
    use crate::send::ResolvedRecipient;
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
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
                source_chat_id: None,
                text: text.to_string(),
                timestamp: IsoTimestamp::now(),
                read: false,
                source_team: Some(TeamName::from_validated(TEST_TEAM)),
                destination_chat_id: None,
                summary: Some(text.to_string()),
                message_id: Some(AtmMessageId::new()),
                requires_ack: false,
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
        delivered_texts: std::sync::Mutex<Vec<String>>,
    }

    impl crate::boundary::sealed::Sealed for RecordingRuntime {}

    impl NonClaudeOutboundDeliveryWriter for RecordingRuntime {
        fn deliver_non_claude_payloads(
            &self,
            _recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
            messages: &[LogicalMessage],
        ) -> Result<(), AtmError> {
            self.delivered_texts
                .lock()
                .expect("deliveries")
                .extend(messages.iter().map(|message| message.envelope.text.clone()));
            Ok(())
        }
    }

    fn recipient_snapshot(harness: DeliveryHarnessPath) -> DeliveryRecipientSnapshot {
        DeliveryRecipientSnapshot {
            agent: AgentName::from_validated("recipient"),
            team: TeamName::from_validated(TEST_TEAM),
            harness,
            recipient_pane_id: None,
            local_tmux_post_send: false,
            graft_post_send: false,
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

    #[test]
    fn execute_delivery_plan_allows_non_claude_target_for_claude_harness() {
        let runtime = NoopRuntime;
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
            vec![message],
            Vec::new(),
        );

        let result = execute_delivery_plan(&runtime, None, &plan).expect("delivery");
        assert_eq!(result.disposition, DeliveryExecutionDisposition::Delivered);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn emit_delivery_plan_transitions_use_non_claude_path_for_persisted_delivery() {
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
            vec![message],
            Vec::new(),
        );

        emit_delivery_plan_transitions(
            &observability,
            transition_context(message_id),
            &plan,
            &super::DeliveryExecutionResult::delivered(),
        )
        .expect("persisted transitions");
        let events = observability.events.lock().expect("events");
        assert!(events.iter().any(|event| {
            event.command == "delivery_policy"
                && event.outcome.as_str() == "delivery_policy.new_message.non_claude_original"
        }));
    }

    #[test]
    fn execute_delivery_plan_routes_recovered_message_sets_through_non_claude_outbound() {
        let runtime = RecordingRuntime::default();
        let plan = DeliveryPlan::new(
            DeliveryPlanKind::Send,
            DeliveryPlanDisposition::SqliteFailedRecovered,
            DeliveryTarget::NonClaude {
                recipient: recipient_snapshot(DeliveryHarnessPath::ClaudeCode),
            },
            ResolvedRecipient {
                agent: AgentName::from_validated("recipient"),
                team: TeamName::from_validated(TEST_TEAM),
            },
            vec![
                logical_message_with_text("original message"),
                logical_message_with_text("companion error"),
            ],
            Vec::new(),
        );

        let result = execute_delivery_plan(&runtime, None, &plan).expect("delivery");
        assert_eq!(result.disposition, DeliveryExecutionDisposition::Delivered);
        assert!(result.warnings.is_empty());
        assert_eq!(
            *runtime.delivered_texts.lock().expect("deliveries"),
            vec![
                "original message".to_string(),
                "companion error".to_string()
            ]
        );
    }
}
