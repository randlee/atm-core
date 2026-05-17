use std::path::Path;

use crate::config::AtmConfig;
use crate::delivery_plan::{
    DeliveryPlan, DeliveryPlanDisposition, DeliveryTarget, LogicalMessage, NotificationTarget,
    ReplyDeliveryPlan,
};
use crate::error::AtmError;
use crate::schema::MessageEnvelope;
use crate::send::{PostSendHookContext, WarningEntry};
use crate::service_runtime::RetainedServiceRuntime;

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

pub(crate) trait ClaudeInboxWriter {
    fn append_claude_inbox_message(
        &self,
        inbox_path: &Path,
        recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
        message: &MessageEnvelope,
    ) -> Result<(), AtmError>;
}

impl<T> ClaudeInboxWriter for T
where
    T: RetainedServiceRuntime + ?Sized,
{
    fn append_claude_inbox_message(
        &self,
        inbox_path: &Path,
        recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
        message: &MessageEnvelope,
    ) -> Result<(), AtmError> {
        self.append_compat_inbox_message(inbox_path, recipient, message)
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

pub(crate) fn execute_delivery_plan<R>(
    runtime: &R,
    config: Option<&AtmConfig>,
    plan: &DeliveryPlan,
) -> Result<DeliveryExecutionResult, AtmError>
where
    R: ClaudeInboxWriter + PostSendNotificationExecutor,
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
    R: ClaudeInboxWriter + PostSendNotificationExecutor,
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
    R: ClaudeInboxWriter + PostSendNotificationExecutor,
{
    let mut result = DeliveryExecutionResult::delivered();

    if let DeliveryTarget::ClaudeCode {
        inbox_path,
        recipient: snapshot,
    } = view.delivery_target
    {
        execute_claude_delivery(
            runtime,
            view.disposition,
            inbox_path,
            snapshot,
            view.messages,
            &mut result,
        );
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

fn execute_claude_delivery<R: ClaudeInboxWriter + ?Sized>(
    runtime: &R,
    disposition: DeliveryPlanDisposition,
    inbox_path: &Path,
    recipient: &crate::delivery_policy::DeliveryRecipientSnapshot,
    messages: &[LogicalMessage],
    result: &mut DeliveryExecutionResult,
) {
    for (index, message) in messages.iter().enumerate() {
        if let Err(error) =
            runtime.append_claude_inbox_message(inbox_path, recipient, &message.envelope)
        {
            result.disposition = DeliveryExecutionDisposition::AppendDegraded;
            result
                .warnings
                .push(build_append_warning(disposition, recipient, index, error));
            if disposition == DeliveryPlanDisposition::SqliteFailedRecovered {
                break;
            }
        }
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
