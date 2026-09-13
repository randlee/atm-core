use std::sync::Arc;

use atm_core::api::RequestDeadline;
use atm_core::boundary::{
    BuiltInPostSendDispatch, PromptHandoff, PromptTrigger, TaskStore,
    built_in_nudge_template_kind_from_post_send_event,
};
use atm_core::error::{AtmError, AtmErrorCode};
use atm_core::types::IsoTimestamp;

use crate::BoundedBlockingBridge;

pub(crate) async fn record_prompt_handoff(
    bridge: &BoundedBlockingBridge,
    deadline: RequestDeadline,
    store: Result<Arc<dyn TaskStore + Send + Sync>, AtmError>,
    dispatch: &BuiltInPostSendDispatch,
    trigger: PromptTrigger,
    at: IsoTimestamp,
) {
    let Some(transition) = dispatch.event.task_transition else {
        return;
    };
    let kind = built_in_nudge_template_kind_from_post_send_event(&dispatch.event, dispatch.kind);
    let store = match store {
        Ok(store) => store,
        Err(error) => {
            log_failure(dispatch, kind, trigger, "storage", &error);
            return;
        }
    };
    let Some(task_id) = dispatch.event.task_id.clone() else {
        let error = AtmError::mailbox_write("task-linked prompt has no task id");
        log_failure(dispatch, kind, trigger, "storage", &error);
        return;
    };
    if deadline.expired() {
        let error = AtmError::new(
            AtmErrorCode::InternalError,
            "prompt-handoff recording deadline expired before bridge admission",
        );
        log_failure(dispatch, kind, trigger, "timeout", &error);
        return;
    }
    let handoff = PromptHandoff {
        team: dispatch.event.recipient_team.clone(),
        agent: dispatch.event.recipient.clone(),
        message_key: dispatch.event.message_id.into(),
        kind,
        task_id,
        attempt: match transition {
            atm_core::boundary::TaskTransition::Reminder { attempt } => attempt,
            _ => 0,
        },
        trigger,
        at,
    };
    if let Err(error) = bridge
        .run(deadline, move || store.record_prompt_handoff(&handoff))
        .await
    {
        let reason = failure_reason(&error);
        log_failure(dispatch, kind, trigger, reason, &error);
    }
}

pub(crate) fn failure_reason(error: &atm_core::error::AtmError) -> &'static str {
    match error.code() {
        atm_core::error::AtmErrorCode::BlockingBridgeDeadlineBeforeStart => "saturated",
        atm_core::error::AtmErrorCode::BlockingBridgeDeadlineAfterStart => "timeout",
        _ => "storage",
    }
}

pub(crate) fn log_failure(
    dispatch: &BuiltInPostSendDispatch,
    kind: atm_core::boundary::BuiltInNudgeTemplateKind,
    trigger: PromptTrigger,
    reason: &'static str,
    error: &AtmError,
) {
    tracing::error!(
        subsystem = "prompt_handoff",
        action = "prompt_handoff_record_failed",
        reason,
        message_id = %dispatch.event.message_id,
        kind = %kind,
        trigger = trigger.as_str(),
        error_code = error.code().as_str(),
        error = %error,
        "Task-linked prompt was emitted but its audit record failed"
    );
}
